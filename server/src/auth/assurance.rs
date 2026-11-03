//! Session assurance (spec B.4, CC4): fixed when a session is created and
//! never changed.

use serde::Serialize;

/// `full` iff a passkey assertion (UV) or a device-key signature of an
/// active key device created the session; every other session, including
/// every session from before the upgrade, is `probation`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Assurance {
    Full,
    Probation,
}

impl Assurance {
    /// The stored and wire form.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Probation => "probation",
        }
    }

    /// Parse the stored form; anything unknown is `probation` (fail safe).
    #[must_use]
    pub fn parse(s: &str) -> Self {
        if s == "full" {
            Self::Full
        } else {
            Self::Probation
        }
    }
}
