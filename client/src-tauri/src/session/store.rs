//! Persistent storage for the desktop session (server URL + refresh token).
//!
//! The production store is the OS keyring; tests use [`MemoryTokenStore`].

use thiserror::Error;

/// The part of a session that survives an app restart.
#[derive(Clone, PartialEq, Eq)]
pub struct StoredSession {
    pub server_url: String,
    pub refresh_token: String,
}

impl std::fmt::Debug for StoredSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoredSession")
            .field("server_url", &self.server_url)
            .field("refresh_token", &"[REDACTED]")
            .finish()
    }
}

/// Failure of the backing credential store.
#[derive(Debug, Error)]
pub enum StoreError {
    #[error("credential store unavailable: {0}")]
    Backend(String),
}

/// Where the refresh token is kept between app starts.
pub trait TokenStore: Send + Sync {
    /// The stored session, if any.
    fn load(&self) -> Result<Option<StoredSession>, StoreError>;
    /// Store (or replace) the session.
    fn save(&self, s: &StoredSession) -> Result<(), StoreError>;
    /// Forget the refresh token for `server_url`.
    fn clear(&self, server_url: &str) -> Result<(), StoreError>;
}

// ============================================================================
// OS keyring
// ============================================================================

/// Keyring service name. Kept from earlier releases so existing logins survive.
const KEYRING_SERVICE: &str = "voicechat";
/// Entry holding the URL of the last server the user signed in to.
const SERVER_URL_ENTRY: &str = "server_url";

fn token_entry(server_url: &str) -> String {
    // One entry per server URL, so several servers can coexist.
    format!("refresh_token:{server_url}")
}

fn backend(e: &keyring_core::Error) -> StoreError {
    StoreError::Backend(e.to_string())
}

/// Token store backed by the platform keyring (Keychain, Credential Manager,
/// Secret Service).
#[derive(Debug, Default)]
pub struct KeyringTokenStore;

impl KeyringTokenStore {
    fn read(user: &str) -> Result<Option<String>, StoreError> {
        let entry = keyring_core::Entry::new(KEYRING_SERVICE, user).map_err(|e| backend(&e))?;
        match entry.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring_core::Error::NoEntry) => Ok(None),
            Err(e) => Err(backend(&e)),
        }
    }

    fn write(user: &str, value: &str) -> Result<(), StoreError> {
        keyring_core::Entry::new(KEYRING_SERVICE, user)
            .and_then(|entry| entry.set_password(value))
            .map_err(|e| backend(&e))
    }
}

impl TokenStore for KeyringTokenStore {
    fn load(&self) -> Result<Option<StoredSession>, StoreError> {
        let Some(server_url) = Self::read(SERVER_URL_ENTRY)? else {
            return Ok(None);
        };
        Ok(
            Self::read(&token_entry(&server_url))?.map(|refresh_token| StoredSession {
                server_url,
                refresh_token,
            }),
        )
    }

    fn save(&self, s: &StoredSession) -> Result<(), StoreError> {
        Self::write(&token_entry(&s.server_url), &s.refresh_token)?;
        Self::write(SERVER_URL_ENTRY, &s.server_url)
    }

    fn clear(&self, server_url: &str) -> Result<(), StoreError> {
        let entry = keyring_core::Entry::new(KEYRING_SERVICE, &token_entry(server_url))
            .map_err(|e| backend(&e))?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring_core::Error::NoEntry) => Ok(()),
            Err(e) => Err(backend(&e)),
        }
    }
}

// ============================================================================
// In-memory store (tests)
// ============================================================================

/// In-memory token store for tests; `save` can be told to fail to simulate a
/// machine without a working keyring.
#[cfg(test)]
#[derive(Debug, Default)]
pub struct MemoryTokenStore {
    session: std::sync::Mutex<Option<StoredSession>>,
    fail_save: std::sync::atomic::AtomicBool,
}

#[cfg(test)]
impl MemoryTokenStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_session(server_url: &str, refresh_token: &str) -> Self {
        let store = Self::default();
        *store.session.lock().unwrap() = Some(StoredSession {
            server_url: server_url.to_string(),
            refresh_token: refresh_token.to_string(),
        });
        store
    }

    pub fn set_fail_save(&self, fail: bool) {
        self.fail_save
            .store(fail, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn get(&self) -> Option<StoredSession> {
        self.session.lock().unwrap().clone()
    }
}

#[cfg(test)]
impl TokenStore for MemoryTokenStore {
    fn load(&self) -> Result<Option<StoredSession>, StoreError> {
        Ok(self.get())
    }

    fn save(&self, s: &StoredSession) -> Result<(), StoreError> {
        if self.fail_save.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(StoreError::Backend("no keyring available".into()));
        }
        *self.session.lock().unwrap() = Some(s.clone());
        Ok(())
    }

    fn clear(&self, server_url: &str) -> Result<(), StoreError> {
        let mut session = self.session.lock().unwrap();
        if session.as_ref().is_some_and(|s| s.server_url == server_url) {
            *session = None;
        }
        Ok(())
    }
}
