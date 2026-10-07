//! Context engine: structured events about what the user is doing.
//!
//! ```text
//! OS observation → context events → context manager → context window
//! ```
//!
//! The pipeline is deterministic. Content (clipboard text, window titles) stays
//! in memory for a bounded time; only metadata events are persisted.

pub mod clipboard;
pub mod events;
pub mod insights;
pub mod manager;

pub use events::{ContextEvent, ContextEventKind};
pub use manager::{ContextManager, ContextWindow};
