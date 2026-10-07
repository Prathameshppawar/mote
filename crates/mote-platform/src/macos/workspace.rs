//! Running applications via NSWorkspace / NSRunningApplication.

use objc2::rc::Retained;
use objc2_app_kit::{NSApplicationActivationOptions, NSApplicationActivationPolicy, NSRunningApplication, NSWorkspace};

use mote_core::platform::{AppInfo, PlatformError};

fn app_info(app: &NSRunningApplication) -> Option<AppInfo> {
    let pid = app.processIdentifier();
    let id = app.bundleIdentifier().map(|s| s.to_string());
    let name = app.localizedName().map(|s| s.to_string());
    let id = id.or_else(|| name.clone())?;
    let name = name.unwrap_or_else(|| id.clone());
    Some(AppInfo { id, name, pid: u32::try_from(pid).ok() })
}

/// The frontmost application.
pub fn frontmost() -> Option<AppInfo> {
    let app = NSWorkspace::sharedWorkspace().frontmostApplication()?;
    app_info(&app)
}

/// Names of the frameworks bundled with the application `pid` (such as
/// `Electron Framework.framework`), which tell how it renders web content.
pub fn bundled_frameworks(pid: i32) -> Vec<String> {
    let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid) else { return Vec::new() };
    let Some(path) = app.bundleURL().and_then(|url| url.path()) else { return Vec::new() };
    let dir = std::path::Path::new(&path.to_string()).join("Contents/Frameworks");
    std::fs::read_dir(dir)
        .map(|entries| entries.filter_map(Result::ok).map(|e| e.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default()
}

/// The application with process id `pid`.
pub fn by_pid(pid: i32) -> Option<AppInfo> {
    let app = NSRunningApplication::runningApplicationWithProcessIdentifier(pid)?;
    app_info(&app)
}

/// Regular (Dock) applications, sorted by name.
pub fn running() -> Vec<AppInfo> {
    let apps = NSWorkspace::sharedWorkspace().runningApplications();
    let mut out: Vec<AppInfo> = apps
        .iter()
        .filter(|a| a.activationPolicy() == NSApplicationActivationPolicy::Regular)
        .filter_map(|a| app_info(&a))
        .collect();
    out.sort_by_key(|a| a.name.to_lowercase());
    out.dedup_by(|a, b| a.id == b.id);
    out
}

/// Brings the application to the front.
pub fn activate(app: &AppInfo) -> Result<(), PlatformError> {
    let running: Option<Retained<NSRunningApplication>> = match app.pid {
        Some(pid) => i32::try_from(pid).ok().and_then(NSRunningApplication::runningApplicationWithProcessIdentifier),
        None => None,
    };
    let running = running.ok_or_else(|| PlatformError::Failed("application is no longer running".into()))?;
    #[allow(deprecated)]
    let ok = running.activateWithOptions(NSApplicationActivationOptions::ActivateIgnoringOtherApps);
    if ok {
        Ok(())
    } else {
        Err(PlatformError::Failed("macOS refused to activate the application".into()))
    }
}
