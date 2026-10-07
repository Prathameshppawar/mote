//! Secret storage in the OS credential store.
//!
//! The Groq API key lives in the macOS Keychain or Windows Credential Manager,
//! never in the database, settings file or logs, and is never sent to the UI.

use thiserror::Error;

pub const GROQ_ACCOUNT: &str = "groq-api-key";

#[derive(Debug, Error)]
pub enum SecretError {
    #[error("the system credential store is unavailable: {0}")]
    Unavailable(String),
}

pub trait SecretStore: Send + Sync {
    fn get(&self, account: &str) -> Result<Option<String>, SecretError>;
    fn set(&self, account: &str, secret: &str) -> Result<(), SecretError>;
    fn delete(&self, account: &str) -> Result<(), SecretError>;
}

/// The OS credential store, scoped to Mote's bundle identifier.
pub struct KeyringStore {
    service: String,
}

impl KeyringStore {
    pub fn new(service: impl Into<String>) -> Self {
        Self { service: service.into() }
    }

    fn entry(&self, account: &str) -> Result<keyring::Entry, SecretError> {
        keyring::Entry::new(&self.service, account).map_err(|e| SecretError::Unavailable(describe(&e)))
    }
}

/// Describes a keyring error without echoing any secret.
fn describe(error: &keyring::Error) -> String {
    match error {
        keyring::Error::NoStorageAccess(_) => "access to the credential store was denied".into(),
        keyring::Error::PlatformFailure(_) => "the credential store reported an error".into(),
        keyring::Error::TooLong(..) | keyring::Error::Invalid(..) | keyring::Error::BadEncoding(_) => {
            "the credential could not be stored".into()
        }
        _ => "unexpected credential store error".into(),
    }
}

impl SecretStore for KeyringStore {
    fn get(&self, account: &str) -> Result<Option<String>, SecretError> {
        match self.entry(account)?.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(SecretError::Unavailable(describe(&e))),
        }
    }

    fn set(&self, account: &str, secret: &str) -> Result<(), SecretError> {
        self.entry(account)?.set_password(secret).map_err(|e| SecretError::Unavailable(describe(&e)))
    }

    fn delete(&self, account: &str) -> Result<(), SecretError> {
        match self.entry(account)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(SecretError::Unavailable(describe(&e))),
        }
    }
}

/// In-memory store for tests.
#[cfg(test)]
#[derive(Default)]
pub struct MemoryStore {
    secrets: std::sync::Mutex<std::collections::HashMap<String, String>>,
}

#[cfg(test)]
impl SecretStore for MemoryStore {
    fn get(&self, account: &str) -> Result<Option<String>, SecretError> {
        Ok(self.secrets.lock().unwrap_or_else(std::sync::PoisonError::into_inner).get(account).cloned())
    }

    fn set(&self, account: &str, secret: &str) -> Result<(), SecretError> {
        self.secrets.lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(account.into(), secret.into());
        Ok(())
    }

    fn delete(&self, account: &str) -> Result<(), SecretError> {
        self.secrets.lock().unwrap_or_else(std::sync::PoisonError::into_inner).remove(account);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_store_roundtrip() {
        let store = MemoryStore::default();
        assert_eq!(store.get(GROQ_ACCOUNT).unwrap(), None);
        store.set(GROQ_ACCOUNT, "gsk_x").unwrap();
        assert_eq!(store.get(GROQ_ACCOUNT).unwrap().as_deref(), Some("gsk_x"));
        store.delete(GROQ_ACCOUNT).unwrap();
        store.delete(GROQ_ACCOUNT).unwrap();
        assert_eq!(store.get(GROQ_ACCOUNT).unwrap(), None);
    }
}
