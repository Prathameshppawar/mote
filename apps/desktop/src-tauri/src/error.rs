//! Errors returned to the frontend over IPC.
//!
//! Messages are user-facing and never contain secrets or user content.

use serde::Serialize;

use mote_core::platform::PlatformError;
use mote_core::providers::ProviderError;
use mote_core::settings::SettingsError;
use mote_storage::StorageError;

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "bindings", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct CommandError {
    /// Stable machine-readable code.
    pub code: String,
    /// Actionable message for the user.
    pub message: String,
    /// Field-level validation problems, when `code` is `validation`.
    pub fields: Vec<SettingsError>,
}

impl CommandError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self { code: code.into(), message: message.into(), fields: Vec::new() }
    }

    pub fn validation(fields: Vec<SettingsError>) -> Self {
        Self { code: "validation".into(), message: "Some settings are invalid.".into(), fields }
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new("invalid_input", message)
    }
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl From<StorageError> for CommandError {
    fn from(error: StorageError) -> Self {
        match error {
            StorageError::Invalid(message) => Self::invalid(message),
            other => {
                tracing::error!(error = %other, "storage error");
                Self::new("storage", "Mote could not read or write its local database.")
            }
        }
    }
}

impl From<ProviderError> for CommandError {
    fn from(error: ProviderError) -> Self {
        Self::new(error.kind(), error.user_message())
    }
}

impl From<PlatformError> for CommandError {
    fn from(error: PlatformError) -> Self {
        let code = match error {
            PlatformError::PermissionDenied => "permission_denied",
            PlatformError::SecureInput => "secure_input",
            PlatformError::NoFocusedElement => "no_focus",
            PlatformError::NotSupported(_) => "not_supported",
            PlatformError::Failed(_) => "platform",
        };
        Self::new(code, error.to_string())
    }
}

pub type CommandResult<T> = Result<T, CommandError>;
