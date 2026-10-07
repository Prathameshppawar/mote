//! Logging: daily rotated files in the app log directory plus stderr.
//!
//! Mote logs metadata only (states, counts, durations, error kinds). Typed text,
//! clipboard content, window titles, prompts, completions and API keys are
//! never passed to the logger.

use std::path::Path;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, EnvFilter};

/// Initializes logging; keep the guard alive for the lifetime of the app.
pub fn init(log_dir: &Path) -> Option<WorkerGuard> {
    let filter = EnvFilter::try_from_env("MOTE_LOG")
        .unwrap_or_else(|_| EnvFilter::new("info,mote_desktop_lib=info,mote_core=info,tao=warn,wry=warn"));
    let file = tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("mote")
        .filename_suffix("log")
        .max_log_files(7)
        .build(log_dir)
        .ok();
    let (writer, guard) = match file {
        Some(appender) => {
            let (writer, guard) = tracing_appender::non_blocking(appender);
            (Some(writer), Some(guard))
        }
        None => (None, None),
    };
    let file_layer = writer.map(|w| fmt::layer().with_ansi(false).with_target(true).with_writer(w));
    let stderr_layer = fmt::layer().with_target(false).with_writer(std::io::stderr);
    let _ = tracing_subscriber::registry().with(filter).with(file_layer).with(stderr_layer).try_init();
    guard
}
