//! COM apartment handling and the UI Automation client object.
//!
//! Every thread that calls into UI Automation lazily joins the process-wide
//! multithreaded apartment (MTA), once per thread. The UI Automation client is
//! created once and shared by all MTA threads: interface pointers living in the
//! MTA may be used from any MTA thread without marshaling, and sharing means no
//! per-thread client has to be released when a (thread-pool) thread exits.
//!
//! Nothing here runs COM code at thread exit. Windows runs thread-local
//! destructors under the loader lock, where releasing COM objects or calling
//! `CoUninitialize` can deadlock; a thread that joined the MTA therefore stays
//! in it until it exits, and the system reclaims its COM state then.

use std::cell::Cell;
use std::mem::{size_of, ManuallyDrop};
use std::sync::{Mutex, OnceLock, PoisonError};

use ::windows::core::{Interface, Result};
use ::windows::Win32::Foundation::RPC_E_CHANGED_MODE;
use ::windows::Win32::System::Com::{
    CoCreateInstance, CoGetApartmentType, CoIncrementMTAUsage, CoInitializeEx, APTTYPE, APTTYPEQUALIFIER, APTTYPE_MTA,
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, SAFEARRAY,
};
use ::windows::Win32::System::Ole::{
    SafeArrayAccessData, SafeArrayDestroy, SafeArrayGetDim, SafeArrayGetElemsize, SafeArrayGetLBound,
    SafeArrayGetUBound, SafeArrayGetVartype, SafeArrayUnaccessData,
};
use ::windows::Win32::System::Variant::VARENUM;
use ::windows::Win32::UI::Accessibility::{CUIAutomation, CUIAutomation8, IUIAutomation, IUIAutomation2};
use mote_core::platform::PlatformError;

/// How long UI Automation waits for an application to hand over an element.
const CONNECTION_TIMEOUT_MS: u32 = 300;
/// How long UI Automation waits for an application to answer a request about an element.
const TRANSACTION_TIMEOUT_MS: u32 = 500;
/// Upper bound on the elements copied out of a SAFEARRAY.
const MAX_ARRAY_ELEMENTS: usize = 4_096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Apartment {
    /// The thread is in the process-wide multithreaded apartment.
    Multithreaded,
    /// The thread was already initialized as single-threaded by someone else (e.g. a UI thread).
    SingleThreaded,
}

thread_local! {
    /// The apartment of the calling thread once this module has initialized COM on it.
    /// Deliberately has no destructor (see the module documentation).
    static APARTMENT: Cell<Option<Apartment>> = const { Cell::new(None) };
}

/// Initializes COM on the calling thread on first use.
fn ensure_initialized() -> Result<Apartment> {
    APARTMENT.with(|cell| {
        if let Some(apartment) = cell.get() {
            return Ok(apartment);
        }
        // SAFETY: CoInitializeEx has no preconditions besides the reserved pointer being null.
        let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        let apartment = if hr.is_ok() {
            // S_OK, or S_FALSE when the thread already was in the MTA.
            Apartment::Multithreaded
        } else if hr == RPC_E_CHANGED_MODE {
            Apartment::SingleThreaded
        } else {
            return Err(hr.into());
        };
        cell.set(Some(apartment));
        Ok(apartment)
    })
}

/// Keeps the MTA alive for the rest of the process, so the shared client stays
/// valid even after every thread that joined the MTA has exited.
fn keep_mta_alive() -> bool {
    static KEPT_ALIVE: OnceLock<bool> = OnceLock::new();
    *KEPT_ALIVE.get_or_init(|| {
        // SAFETY: CoIncrementMTAUsage has no preconditions. The cookie is intentionally never
        // passed to CoDecrementMTAUsage: the MTA must outlive the shared client.
        match unsafe { CoIncrementMTAUsage() } {
            Ok(_cookie) => true,
            Err(error) => {
                tracing::debug!(hresult = %super::hresult(&error), "CoIncrementMTAUsage failed");
                false
            }
        }
    })
}

/// Creates a UI Automation client with short timeouts, so an unresponsive
/// application cannot block the caller for long.
fn create_automation() -> Result<IUIAutomation> {
    // SAFETY: COM is initialized on this thread (callers go through `ensure_initialized`)
    // and there is no outer object.
    let created = unsafe { CoCreateInstance::<_, IUIAutomation>(&CUIAutomation8, None, CLSCTX_INPROC_SERVER) };
    let automation = match created {
        Ok(automation) => automation,
        // CUIAutomation8 (which also implements IUIAutomation2) exists since Windows 8.
        // SAFETY: as above.
        Err(_) => unsafe { CoCreateInstance::<_, IUIAutomation>(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }?,
    };
    if let Ok(automation2) = automation.cast::<IUIAutomation2>() {
        // SAFETY: COM calls on a live interface pointer with plain integer arguments.
        let configured = unsafe {
            automation2
                .SetConnectionTimeout(CONNECTION_TIMEOUT_MS)
                .and_then(|()| automation2.SetTransactionTimeout(TRANSACTION_TIMEOUT_MS))
        };
        if let Err(error) = configured {
            tracing::debug!(hresult = %super::hresult(&error), "could not set UI Automation timeouts");
        }
    }
    Ok(automation)
}

fn failure(action: &str, error: &::windows::core::Error) -> PlatformError {
    PlatformError::Failed(format!("{action} failed ({})", super::hresult(error)))
}

/// The UI Automation client shared by every thread in the MTA.
#[derive(Default)]
pub(super) struct SharedAutomation {
    client: Mutex<Option<MtaClient>>,
}

impl SharedAutomation {
    /// A UI Automation client usable on the calling thread, initializing COM
    /// on the thread first if needed.
    pub(super) fn get(&self) -> std::result::Result<IUIAutomation, PlatformError> {
        let apartment = ensure_initialized().map_err(|error| failure("COM initialization", &error))?;
        if apartment == Apartment::Multithreaded && keep_mta_alive() {
            let mut client = self.client.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(shared) = client.as_ref() {
                return Ok(shared.get());
            }
            let created = create_automation().map_err(|error| failure("creating the UI Automation client", &error))?;
            *client = Some(MtaClient(ManuallyDrop::new(created.clone())));
            return Ok(created);
        }
        // A single-threaded apartment cannot use the MTA client directly: it gets its own,
        // released when the caller drops it.
        create_automation().map_err(|error| failure("creating the UI Automation client", &error))
    }
}

/// A UI Automation client created in the MTA.
struct MtaClient(ManuallyDrop<IUIAutomation>);

// SAFETY: the client was created on a thread in the process-wide MTA, which `keep_mta_alive`
// keeps alive for the rest of the process. COM allows MTA interface pointers to be used from
// any thread in the MTA; `SharedAutomation::get` hands out references only to threads that
// joined the MTA, and `Drop` releases the client only on an MTA thread.
unsafe impl Send for MtaClient {}

impl MtaClient {
    fn get(&self) -> IUIAutomation {
        IUIAutomation::clone(&self.0)
    }
}

impl Drop for MtaClient {
    fn drop(&mut self) {
        if current_thread_in_mta() {
            // SAFETY: the client is dropped exactly once, here, on a thread in the MTA.
            unsafe { ManuallyDrop::drop(&mut self.0) };
        }
        // Otherwise the reference is leaked on purpose: releasing an MTA object from another
        // apartment is not allowed, and this happens at most once per adapter.
    }
}

/// Whether the calling thread is in the MTA (explicitly or implicitly).
fn current_thread_in_mta() -> bool {
    let mut kind = APTTYPE::default();
    let mut qualifier = APTTYPEQUALIFIER::default();
    // SAFETY: both out-pointers refer to valid, writable locals.
    let queried = unsafe { CoGetApartmentType(&mut kind, &mut qualifier) };
    queried.is_ok() && kind == APTTYPE_MTA
}

/// Owns a SAFEARRAY returned by a COM call and destroys it when dropped.
pub(super) struct SafeArray(*mut SAFEARRAY);

impl SafeArray {
    /// Takes ownership of `array`.
    ///
    /// # Safety
    ///
    /// `array` must be null or a valid SAFEARRAY that the caller owns (for
    /// example the out-value of a COM call) and that nothing else destroys.
    pub(super) unsafe fn from_raw(array: *mut SAFEARRAY) -> Self {
        Self(array)
    }

    /// Copies the elements of a one-dimensional array of `T` tagged `vartype`.
    /// Returns an empty vector for anything else.
    pub(super) fn to_vec<T: Copy>(&self, vartype: VARENUM) -> Vec<T> {
        let array = self.0;
        if array.is_null() {
            return Vec::new();
        }
        // SAFETY: `array` is a valid SAFEARRAY owned by `self` (see `from_raw`); these calls
        // only read its descriptor.
        let (dimensions, element_size, actual_type) =
            unsafe { (SafeArrayGetDim(array), SafeArrayGetElemsize(array), SafeArrayGetVartype(array)) };
        if dimensions != 1
            || usize::try_from(element_size).ok() != Some(size_of::<T>())
            || actual_type.is_ok_and(|actual| actual != vartype)
        {
            return Vec::new();
        }
        // SAFETY: as above; the array has exactly one dimension.
        let bounds = unsafe { (SafeArrayGetLBound(array, 1), SafeArrayGetUBound(array, 1)) };
        let (Ok(lower), Ok(upper)) = bounds else {
            return Vec::new();
        };
        let available = usize::try_from(i64::from(upper) - i64::from(lower) + 1).unwrap_or(0);
        let count = available.min(MAX_ARRAY_ELEMENTS);
        if count == 0 {
            return Vec::new();
        }
        let mut data = std::ptr::null_mut();
        // SAFETY: `data` is a valid out-pointer; a successful access is balanced below.
        if unsafe { SafeArrayAccessData(array, &mut data) }.is_err() {
            return Vec::new();
        }
        let elements = data.cast::<T>();
        let values = if elements.is_null() || !elements.is_aligned() {
            Vec::new()
        } else {
            // SAFETY: while accessed, the array's data holds `available >= count` elements of
            // `size_of::<T>()` bytes each (checked above), and the pointer is non-null and aligned.
            unsafe { std::slice::from_raw_parts(elements, count) }.to_vec()
        };
        // SAFETY: balances the successful SafeArrayAccessData above.
        let _ = unsafe { SafeArrayUnaccessData(array) };
        values
    }
}

impl Drop for SafeArray {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: the array is owned by `self` (see `from_raw`) and destroyed exactly once.
            let _ = unsafe { SafeArrayDestroy(self.0) };
        }
    }
}

#[cfg(test)]
mod tests {
    use ::windows::Win32::System::Ole::SafeArrayCreateVector;
    use ::windows::Win32::System::Variant::{VT_I4, VT_R8};

    use super::*;

    #[test]
    fn com_initializes_once_per_thread() {
        let first = ensure_initialized().expect("COM initializes");
        assert_eq!(ensure_initialized().expect("COM initializes"), first, "cached per thread");
        let other = std::thread::spawn(|| ensure_initialized().map(|apartment| apartment == Apartment::Multithreaded));
        assert_eq!(other.join().ok().and_then(|result| result.ok()), Some(true), "a fresh thread joins the MTA");
    }

    #[test]
    fn the_automation_client_is_shared_between_mta_threads() {
        let shared = std::sync::Arc::new(SharedAutomation::default());
        let first = shared.get().expect("UI Automation is available");
        let again = shared.get().expect("UI Automation is available");
        assert_eq!(first.as_raw(), again.as_raw(), "same client on the same thread");
        let raw = first.as_raw() as usize;
        let other = shared.clone();
        let from_other_thread =
            std::thread::spawn(move || other.get().map(|client| client.as_raw() as usize).unwrap_or_default());
        assert_eq!(from_other_thread.join().unwrap_or_default(), raw, "same client on another MTA thread");
    }

    #[test]
    fn safe_arrays_are_copied_and_type_checked() {
        // SAFETY: SafeArrayCreateVector allocates a new array that we own.
        let raw = unsafe { SafeArrayCreateVector(VT_R8, 0, 4) };
        assert!(!raw.is_null());
        let mut data = std::ptr::null_mut();
        // SAFETY: the array was just created with 4 doubles; access is balanced below.
        unsafe {
            SafeArrayAccessData(raw, &mut data).expect("access");
            std::slice::from_raw_parts_mut(data.cast::<f64>(), 4).copy_from_slice(&[1.0, 2.0, 3.0, 4.0]);
            SafeArrayUnaccessData(raw).expect("unaccess");
        }
        // SAFETY: we own `raw` and hand it to the guard, which destroys it.
        let array = unsafe { SafeArray::from_raw(raw) };
        assert_eq!(array.to_vec::<f64>(VT_R8), vec![1.0, 2.0, 3.0, 4.0]);
        assert!(array.to_vec::<i32>(VT_I4).is_empty(), "element type mismatch");
        // SAFETY: a null array is allowed and owns nothing.
        assert!(unsafe { SafeArray::from_raw(std::ptr::null_mut()) }.to_vec::<f64>(VT_R8).is_empty());
    }
}
