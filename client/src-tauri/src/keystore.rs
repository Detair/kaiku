//! OS credential store selection for `keyring-core`.
//!
//! Replaces `keyring::use_native_store(true)`. The `keyring` meta-crate
//! unconditionally depends on `db-keystore` (sqlite via turso), which we never
//! use and which carries unfixed advisories. These are the same per-OS stores
//! it would pick: Secret Service (not kernel keyutils, which evicts on session
//! end) on Linux, the login Keychain on macOS, Credential Manager on Windows.
//! Each store is built with its default configuration, so service and user
//! naming (and thus already stored credentials) are unchanged.

use keyring_core::{set_default_store, Result};

/// Install the platform's native credential store as keyring-core's default.
/// Must run before any `Entry::new`.
#[allow(clippy::unnecessary_wraps)]
pub fn use_native_store() -> Result<()> {
    #[cfg(target_os = "linux")]
    set_default_store(dbus_secret_service_keyring_store::Store::new()?);
    #[cfg(target_os = "macos")]
    set_default_store(apple_native_keyring_store::keychain::Store::new()?);
    #[cfg(target_os = "windows")]
    set_default_store(windows_native_keyring_store::Store::new()?);
    Ok(())
}
