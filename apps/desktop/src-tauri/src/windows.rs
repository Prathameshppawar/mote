//! Window management: the main window (settings, onboarding, usage), the
//! suggestion overlay and the command palette.

use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, PhysicalPosition, PhysicalSize, WebviewUrl,
    WebviewWindow, WebviewWindowBuilder,
};

use mote_core::platform::{CoordinateSpace, Rect};

pub const MAIN: &str = "main";
pub const OVERLAY: &str = "overlay";
pub const PALETTE: &str = "palette";

/// Sections of the main window that can be opened directly.
pub const SECTIONS: &[&str] = &[
    "onboarding",
    "general",
    "providers",
    "models",
    "completion",
    "writing",
    "context",
    "privacy",
    "usage",
    "keyboard",
    "excluded",
    "activity",
    "diagnostics",
    "about",
];

/// Opens (or focuses) the main window at `section`.
pub fn show_main(app: &AppHandle, section: Option<&str>) -> tauri::Result<()> {
    let section = section.filter(|s| SECTIONS.contains(s));
    #[cfg(target_os = "macos")]
    {
        // Appear in the Dock and the app switcher while the window is open.
        let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
    }
    if let Some(window) = app.get_webview_window(MAIN) {
        window.show()?;
        window.unminimize()?;
        window.set_focus()?;
        if let Some(section) = section {
            window.emit("navigate", section)?;
        }
        return Ok(());
    }
    let url = format!("index.html#/{}", section.unwrap_or(""));
    let window = WebviewWindowBuilder::new(app, MAIN, WebviewUrl::App(url.into()))
        .title("Mote")
        .inner_size(1060.0, 720.0)
        .min_inner_size(860.0, 580.0)
        .center()
        .focused(true)
        .build()?;
    let handle = app.clone();
    window.on_window_event(move |event| {
        if let tauri::WindowEvent::Destroyed = event {
            #[cfg(target_os = "macos")]
            {
                let _ = handle.set_activation_policy(tauri::ActivationPolicy::Accessory);
            }
            #[cfg(not(target_os = "macos"))]
            let _ = &handle;
        }
    });
    window.set_focus()?;
    Ok(())
}

/// Creates the (hidden, click-through, non-focusable) suggestion overlay.
pub fn create_overlay(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    let window = WebviewWindowBuilder::new(app, OVERLAY, WebviewUrl::App("overlay.html".into()))
        .title("Mote suggestion")
        .inner_size(360.0, 40.0)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible_on_all_workspaces(true)
        .visible(false)
        .focused(false)
        .focusable(false)
        .build()?;
    window.set_ignore_cursor_events(true)?;
    Ok(window)
}

/// Returns the palette window, creating it on first use.
pub fn palette(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    if let Some(window) = app.get_webview_window(PALETTE) {
        return Ok(window);
    }
    WebviewWindowBuilder::new(app, PALETTE, WebviewUrl::App("palette.html".into()))
        .title("Mote")
        .inner_size(680.0, 500.0)
        .decorations(false)
        .transparent(true)
        .shadow(true)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible(false)
        .build()
}

/// The pointer position in the coordinates monitors are looked up in: logical
/// points on macOS (where tao reports points × the primary display's scale),
/// physical pixels elsewhere.
pub fn cursor_point(app: &AppHandle) -> Option<(f64, f64)> {
    let cursor = app.cursor_position().ok()?;
    #[cfg(target_os = "macos")]
    {
        let scale = app.primary_monitor().ok().flatten().map_or(1.0, |m| m.scale_factor());
        Some((cursor.x / scale, cursor.y / scale))
    }
    #[cfg(not(target_os = "macos"))]
    {
        Some((cursor.x, cursor.y))
    }
}

/// How many anchor-space units one CSS pixel spans at a point: 1 for logical
/// points, the monitor's scale factor for physical pixels.
pub fn units_per_css_pixel(app: &AppHandle, space: CoordinateSpace, x: f64, y: f64) -> f64 {
    match space {
        CoordinateSpace::LogicalPoints => 1.0,
        CoordinateSpace::PhysicalPixels => {
            app.monitor_from_point(x, y).ok().flatten().map_or(1.0, |m| m.scale_factor()).max(1.0)
        }
    }
}

/// Shows the palette centred on the monitor under the mouse pointer.
pub fn show_palette(app: &AppHandle) -> tauri::Result<()> {
    let window = palette(app)?;
    let size = window.outer_size()?;
    let monitor = cursor_point(app).and_then(|(x, y)| app.monitor_from_point(x, y).ok().flatten());
    match monitor.or(app.primary_monitor()?) {
        Some(monitor) => {
            let area = monitor.work_area();
            let x = area.position.x
                + (i32::try_from(area.size.width).unwrap_or(0) - i32::try_from(size.width).unwrap_or(0)) / 2;
            let y = area.position.y + i32::try_from(area.size.height).unwrap_or(0) / 5;
            window.set_position(PhysicalPosition::new(x, y))?;
        }
        None => window.center()?,
    }
    window.show()?;
    window.set_focus()?;
    window.emit("palette-open", ())?;
    Ok(())
}

pub fn hide_palette(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(PALETTE) {
        let _ = window.hide();
    }
}

/// Where the overlay goes relative to the caret.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Placement {
    /// On the caret's line, starting at the caret (ghost text).
    Inline,
    /// Just below the caret's line.
    Below,
}

/// Computes the overlay's top-left corner in the anchor's coordinate space.
/// `height` is in the same units; `unit` is one CSS pixel in those units.
pub fn overlay_origin(anchor: Rect, placement: Placement, height: f64, unit: f64) -> (f64, f64) {
    match placement {
        Placement::Inline => (anchor.x + unit, anchor.y + anchor.height / 2.0 - height / 2.0),
        Placement::Below => (anchor.x - 10.0 * unit, anchor.y + anchor.height + 6.0 * unit),
    }
}

/// Clamps a rectangle (in the given coordinate space) into the work area of
/// the monitor containing its origin.
pub fn clamp_to_monitor(app: &AppHandle, space: CoordinateSpace, x: f64, y: f64, w: f64, h: f64) -> (f64, f64) {
    let Ok(monitors) = app.available_monitors() else { return (x, y) };
    for monitor in monitors {
        let scale = match space {
            CoordinateSpace::LogicalPoints => monitor.scale_factor(),
            CoordinateSpace::PhysicalPixels => 1.0,
        };
        let area = monitor.work_area();
        let (left, top) = (f64::from(area.position.x) / scale, f64::from(area.position.y) / scale);
        let (width, height) = (f64::from(area.size.width) / scale, f64::from(area.size.height) / scale);
        if x >= left - 1.0 && x <= left + width && y >= top - 1.0 && y <= top + height {
            let cx = x.clamp(left, (left + width - w).max(left));
            let cy = y.clamp(top, (top + height - h).max(top));
            return (cx, cy);
        }
    }
    (x, y)
}

/// Moves and resizes the overlay. Position and size are both in `space`
/// units, following the platform adapter.
pub fn place_overlay(
    window: &WebviewWindow,
    space: CoordinateSpace,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> tauri::Result<()> {
    match space {
        CoordinateSpace::LogicalPoints => {
            window.set_position(LogicalPosition::new(x, y))?;
            window.set_size(LogicalSize::new(w, h))
        }
        CoordinateSpace::PhysicalPixels => {
            window.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32))?;
            window.set_size(PhysicalSize::new(w.round().max(1.0) as u32, h.round().max(1.0) as u32))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_origins() {
        let anchor = Rect { x: 100.0, y: 200.0, width: 1.0, height: 20.0 };
        assert_eq!(overlay_origin(anchor, Placement::Inline, 30.0, 1.0), (101.0, 195.0));
        assert_eq!(overlay_origin(anchor, Placement::Below, 30.0, 1.0), (90.0, 226.0));
        // At 150% on a physical-pixel platform, offsets scale with the display.
        assert_eq!(overlay_origin(anchor, Placement::Below, 45.0, 1.5), (85.0, 229.0));
    }

    #[test]
    fn sections_are_known() {
        assert!(SECTIONS.contains(&"usage"));
        assert!(!SECTIONS.contains(&"../etc"));
    }
}
