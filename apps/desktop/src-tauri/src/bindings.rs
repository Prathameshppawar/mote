//! Exports TypeScript definitions for every IPC type into `apps/desktop/src/bindings`.
//!
//! Regenerate with:
//! `cargo test -p mote-desktop --features bindings export_bindings`
//! CI fails if the committed bindings are out of date.

use ts_rs::{Config, TS};

use mote_core::context::ContextEvent;
use mote_core::engine::{EngineStatus, OverlayView};
use mote_core::platform::{AppInfo, PermissionState, PermissionStatus};
use mote_core::privacy::{ExclusionKind, ExclusionRule};
use mote_core::prompts::{EnhanceStyle, TransformAction};
use mote_core::providers::types::{HealthReport, ModelRole};
use mote_core::settings::{Settings, SettingsError};
use mote_core::usage::pricing::ModelPricing;

use crate::commands::{AppInfoResponse, ModelOption, ProviderStatus, UsageResponse};
use crate::diagnostics::Diagnostics;
use crate::error::CommandError;
use crate::palette::{
    ApplyMode, PaletteApplyRequest, PaletteApplyResult, PaletteContext, PaletteRunRequest, PaletteRunResult,
    PaletteSource,
};
use crate::shell::OverlayPayload;
use crate::updates::{UpdateState, UpdateStatus};

#[test]
fn export_bindings() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../src/bindings");
    let cfg = Config::new().with_out_dir(dir).with_large_int("number");
    macro_rules! export {
        ($($t:ty),* $(,)?) => { $( <$t>::export_all(&cfg).expect(stringify!($t)); )* };
    }
    export!(
        Settings,
        SettingsError,
        CommandError,
        AppInfoResponse,
        AppInfo,
        ProviderStatus,
        HealthReport,
        ModelOption,
        ModelRole,
        UsageResponse,
        ModelPricing,
        ExclusionRule,
        ExclusionKind,
        ContextEvent,
        PermissionStatus,
        PermissionState,
        Diagnostics,
        EngineStatus,
        OverlayView,
        OverlayPayload,
        TransformAction,
        EnhanceStyle,
        UpdateStatus,
        UpdateState,
        PaletteContext,
        PaletteRunRequest,
        PaletteRunResult,
        PaletteApplyRequest,
        PaletteApplyResult,
        PaletteSource,
        ApplyMode,
    );
}
