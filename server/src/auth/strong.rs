//! Strong authenticators (spec B.0): active passkeys and active key devices.
//!
//! Task 3 defines the reference types; Task 8 adds the queries. LP adds the
//! passkey arms.

use uuid::Uuid;

/// The kind of a strong authenticator.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AuthenticatorKind {
    Passkey,
    KeyDevice,
}

impl AuthenticatorKind {
    /// The stored and wire form (`authenticator_type`).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Passkey => "passkey",
            Self::KeyDevice => "key_device",
        }
    }

    /// Parse the stored form.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "passkey" => Some(Self::Passkey),
            "key_device" => Some(Self::KeyDevice),
            _ => None,
        }
    }
}

/// One passkey or key device, by row id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AuthenticatorRef {
    pub kind: AuthenticatorKind,
    pub id: Uuid,
}

impl AuthenticatorRef {
    #[must_use]
    pub const fn passkey(id: Uuid) -> Self {
        Self {
            kind: AuthenticatorKind::Passkey,
            id,
        }
    }

    #[must_use]
    pub const fn key_device(id: Uuid) -> Self {
        Self {
            kind: AuthenticatorKind::KeyDevice,
            id,
        }
    }

    /// The stored and wire form of the kind.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        self.kind.as_str()
    }

    /// From the stored `(authenticator_type, authenticator_id)` pair.
    #[must_use]
    pub fn from_columns(kind: Option<&str>, id: Option<Uuid>) -> Option<Self> {
        Some(Self {
            kind: AuthenticatorKind::parse(kind?)?,
            id: id?,
        })
    }
}
