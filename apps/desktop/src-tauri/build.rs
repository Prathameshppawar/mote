//! Generates per-command IPC permissions so each window can be granted only
//! the commands it needs (see `capabilities/`).

const COMMANDS: &[&str] = &[
    // Main window
    "get_app_info",
    "get_update_status",
    "check_for_updates",
    "install_update",
    "get_settings",
    "save_settings",
    "get_provider_status",
    "set_api_key",
    "clear_api_key",
    "test_connection",
    "list_models",
    "get_usage_dashboard",
    "list_pricing",
    "save_pricing",
    "delete_pricing",
    "reset_pricing",
    "clear_usage_history",
    "clear_context",
    "reset_local_data",
    "list_exclusions",
    "add_exclusion",
    "remove_exclusion",
    "list_running_apps",
    "get_always_excluded",
    "get_recent_activity",
    "get_permission_status",
    "request_accessibility_permission",
    "open_accessibility_settings",
    "get_diagnostics",
    "copy_diagnostics",
    "set_paused",
    "complete_onboarding",
    "get_engine_status",
    // Command palette
    "palette_context",
    "palette_run",
    "palette_cancel",
    "palette_apply",
    "palette_close",
    "palette_open_main",
    // Overlay
    "overlay_ready",
];

fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("failed to run tauri-build");
}
