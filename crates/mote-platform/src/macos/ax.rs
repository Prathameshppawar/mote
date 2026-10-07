//! Thin, safe helpers over the macOS Accessibility (AXUIElement) API.

use std::ffi::c_void;
use std::ptr::{self, NonNull};

use objc2_application_services::{AXError, AXUIElement, AXValue, AXValueType};
use objc2_core_foundation::{
    CFBoolean, CFDictionary, CFHash, CFNumber, CFRange, CFRetained, CFString, CFType, CGPoint, CGRect, CGSize,
};

/// A retained accessibility element that may be moved between threads.
///
/// SAFETY: AXUIElementRef is an immutable CF object; the Accessibility API is
/// documented to be callable from any thread (requests are IPC to the target
/// application), and CFRetain/CFRelease are thread-safe.
pub struct Element(pub CFRetained<AXUIElement>);

unsafe impl Send for Element {}
unsafe impl Sync for Element {}

/// Messaging timeout for every element, so an unresponsive application can
/// never stall the observer for long.
const MESSAGING_TIMEOUT_SECS: f32 = 0.25;

impl Element {
    pub fn system_wide() -> Self {
        // SAFETY: plain constructor without preconditions.
        let element = unsafe { AXUIElement::new_system_wide() };
        // SAFETY: valid element; a timeout is a plain number.
        unsafe { element.set_messaging_timeout(MESSAGING_TIMEOUT_SECS) };
        Self(element)
    }

    pub fn application(pid: i32) -> Self {
        // SAFETY: any pid is accepted; calls on a dead pid simply fail.
        let element = unsafe { AXUIElement::new_application(pid) };
        // SAFETY: valid element.
        unsafe { element.set_messaging_timeout(MESSAGING_TIMEOUT_SECS) };
        Self(element)
    }

    /// Copies an attribute value. `Err` carries the AX error code.
    pub fn attribute(&self, name: &str) -> Result<CFRetained<CFType>, AXError> {
        let name = CFString::from_str(name);
        let mut value: *const CFType = ptr::null();
        // SAFETY: `value` is a valid out-pointer; on success the API returns a
        // +1 retained reference which we adopt below.
        let error = unsafe { self.0.copy_attribute_value(&name, NonNull::from(&mut value)) };
        if error != AXError::Success {
            return Err(error);
        }
        match NonNull::new(value.cast_mut()) {
            // SAFETY: success with a non-null value means we own one reference.
            Some(ptr) => Ok(unsafe { CFRetained::from_raw(ptr) }),
            None => Err(AXError::NoValue),
        }
    }

    pub fn string(&self, name: &str) -> Option<String> {
        self.attribute(name).ok()?.downcast::<CFString>().ok().map(|s| s.to_string())
    }

    pub fn bool(&self, name: &str) -> Option<bool> {
        self.attribute(name).ok()?.downcast::<CFBoolean>().ok().map(|b| b.as_bool())
    }

    pub fn number(&self, name: &str) -> Option<i64> {
        self.attribute(name).ok()?.downcast::<CFNumber>().ok().and_then(|n| n.as_i64())
    }

    pub fn element(&self, name: &str) -> Result<Element, AXError> {
        let value = self.attribute(name)?;
        let element = value.downcast::<AXUIElement>().map_err(|_| AXError::Failure)?;
        // SAFETY: valid element.
        unsafe { element.set_messaging_timeout(MESSAGING_TIMEOUT_SECS) };
        Ok(Element(element))
    }

    pub fn range(&self, name: &str) -> Option<CFRange> {
        let value = self.attribute(name).ok()?.downcast::<AXValue>().ok()?;
        ax_value_get(&value, AXValueType::CFRange, CFRange { location: 0, length: 0 })
    }

    pub fn point(&self, name: &str) -> Option<CGPoint> {
        let value = self.attribute(name).ok()?.downcast::<AXValue>().ok()?;
        ax_value_get(&value, AXValueType::CGPoint, CGPoint { x: 0.0, y: 0.0 })
    }

    pub fn size(&self, name: &str) -> Option<CGSize> {
        let value = self.attribute(name).ok()?.downcast::<AXValue>().ok()?;
        ax_value_get(&value, AXValueType::CGSize, CGSize { width: 0.0, height: 0.0 })
    }

    fn parameterized(&self, name: &str, parameter: &CFType) -> Option<CFRetained<CFType>> {
        let name = CFString::from_str(name);
        let mut value: *const CFType = ptr::null();
        // SAFETY: valid out-pointer; +1 reference adopted on success.
        let error = unsafe { self.0.copy_parameterized_attribute_value(&name, parameter, NonNull::from(&mut value)) };
        if error != AXError::Success {
            return None;
        }
        // SAFETY: success with a non-null value means we own one reference.
        NonNull::new(value.cast_mut()).map(|ptr| unsafe { CFRetained::from_raw(ptr) })
    }

    /// Text in a UTF-16 range, via `AXStringForRange`.
    pub fn string_for_range(&self, location: usize, length: usize) -> Option<String> {
        let range = range_value(location, length)?;
        self.parameterized("AXStringForRange", &range)?.downcast::<CFString>().ok().map(|s| s.to_string())
    }

    /// Screen bounds of a UTF-16 range, via `AXBoundsForRange`.
    pub fn bounds_for_range(&self, location: usize, length: usize) -> Option<CGRect> {
        let range = range_value(location, length)?;
        let value = self.parameterized("AXBoundsForRange", &range)?.downcast::<AXValue>().ok()?;
        ax_value_get(&value, AXValueType::CGRect, CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(0.0, 0.0)))
    }

    pub fn set_bool(&self, name: &str, value: bool) -> bool {
        let name = CFString::from_str(name);
        // SAFETY: CFBoolean is a valid CFType for boolean attributes.
        unsafe { self.0.set_attribute_value(&name, CFBoolean::new(value)) == AXError::Success }
    }

    /// Sets the selected text range (used to select all without keystrokes).
    pub fn set_selected_range(&self, location: usize, length: usize) -> bool {
        let Some(range) = range_value(location, length) else { return false };
        let name = CFString::from_str("AXSelectedTextRange");
        // SAFETY: an AXValue wrapping a CFRange is the documented value type.
        unsafe { self.0.set_attribute_value(&name, &range) == AXError::Success }
    }

    pub fn pid(&self) -> Option<i32> {
        let mut pid: libc::pid_t = 0;
        // SAFETY: valid out-pointer.
        let error = unsafe { self.0.pid(NonNull::from(&mut pid)) };
        (error == AXError::Success).then_some(pid)
    }

    /// Child elements (`AXChildren`).
    pub fn children(&self) -> Vec<Element> {
        let Ok(value) = self.attribute("AXChildren") else { return Vec::new() };
        let Ok(array) = value.downcast::<objc2_core_foundation::CFArray>() else { return Vec::new() };
        // SAFETY: every element of a CFArray is a CF object, so viewing the
        // elements as CFType is sound; each is then type-checked by downcast.
        let array: CFRetained<objc2_core_foundation::CFArray<CFType>> = unsafe { CFRetained::cast_unchecked(array) };
        (0..array.len())
            .filter_map(|i| array.get(i))
            .filter_map(|item| item.downcast::<AXUIElement>().ok())
            .map(|element| {
                // SAFETY: valid element.
                unsafe { element.set_messaging_timeout(MESSAGING_TIMEOUT_SECS) };
                Element(element)
            })
            .collect()
    }

    /// CFHash of the element: equal elements (the same UI object) hash equally.
    pub fn hash(&self) -> u64 {
        CFHash(Some(&self.0)) as u64
    }
}

fn range_value(location: usize, length: usize) -> Option<CFRetained<AXValue>> {
    let range = CFRange { location: isize::try_from(location).ok()?, length: isize::try_from(length).ok()? };
    // SAFETY: the pointer refers to a live CFRange matching the declared type.
    unsafe { AXValue::new(AXValueType::CFRange, NonNull::from(&range).cast::<c_void>()) }
}

fn ax_value_get<T>(value: &AXValue, kind: AXValueType, mut out: T) -> Option<T> {
    // SAFETY: `out` has the layout of `kind` (CFRange/CGPoint/CGSize/CGRect),
    // and AXValueGetValue only writes when the types match.
    let ok = unsafe { value.value(kind, NonNull::from(&mut out).cast::<c_void>()) };
    ok.then_some(out)
}

/// Whether this process is trusted for Accessibility.
pub fn is_trusted() -> bool {
    // SAFETY: no preconditions.
    unsafe { objc2_application_services::AXIsProcessTrusted() }
}

/// Asks macOS to show the Accessibility permission prompt; returns current trust.
pub fn prompt_for_trust() -> bool {
    // SAFETY: reading a framework-provided constant.
    let key: &CFString = unsafe { objc2_application_services::kAXTrustedCheckOptionPrompt };
    let options = CFDictionary::<CFString, CFBoolean>::from_slices(&[key], &[CFBoolean::new(true)]);
    // SAFETY: the options dictionary has CFString keys and CFBoolean values as documented.
    unsafe { objc2_application_services::AXIsProcessTrustedWithOptions(Some(options.as_opaque())) }
}

#[link(name = "Carbon", kind = "framework")]
extern "C" {
    fn IsSecureEventInputEnabled() -> u8;
}

/// Whether macOS Secure Event Input is on (a password field or secure terminal
/// has keyboard focus). While on, Mote neither reads nor types.
pub fn secure_input_enabled() -> bool {
    // SAFETY: argument-less Carbon query.
    unsafe { IsSecureEventInputEnabled() != 0 }
}
