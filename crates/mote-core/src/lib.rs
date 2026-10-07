//! # mote-core
//!
//! The platform-agnostic heart of Mote. Nothing in this crate talks to the
//! operating system, the network or the database directly: those capabilities
//! arrive through traits ([`platform::PlatformAdapter`],
//! [`providers::ModelProvider`], [`usage::UsageSink`]) so the logic here can be
//! tested deterministically.
//!
//! Pipeline:
//!
//! ```text
//! OS observation → context events → context manager → context window
//!     → intent engine → assistance engine → provider → suggestion → shell
//! ```

pub mod ai;
pub mod assistance;
pub mod completion;
pub mod context;
pub mod engine;
pub mod intent;
pub mod language;
pub mod observer;
pub mod platform;
pub mod privacy;
pub mod prompts;
pub mod providers;
pub mod settings;
pub mod spelling;
#[cfg(any(test, feature = "testing"))]
pub mod testing;
pub mod text;
pub mod usage;
