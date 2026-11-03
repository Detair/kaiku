//! Passkeys (`WebAuthn` L3, discoverable, UV required; spec B.1, F1, F7, C29).
//!
//! - `config`: the RP ID (`PUBLIC_URL`'s host), the web origin and the Android `apk-key-hash`
//!   origins.
//! - `rp`: `webauthn-rs` in Kaiku's profile.

pub mod ceremony;
pub mod config;
pub mod queries;
pub mod rp;
pub mod rp_pin;
