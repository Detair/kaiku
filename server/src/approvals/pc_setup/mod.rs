//! "Set up this PC" (spec 2026-11-06-this-pc-setup-design, SM5): a probation
//! session on a PC gets a passkey after the user's key phone scans a QR and
//! approves a `pc_setup` challenge (kind byte 6).
//!
//! **SECURITY CRITICAL.** This module holds the vocabulary and limits of the
//! attempt; the lifecycle (start, scan, approve, code entry) follows in the
//! attempt modules. Locking (spec "Locking (normative)", added to B.7): every
//! transition takes the account lock, then the requesting session row, then
//! the attempt, then the enrolment. Attempts stay out of the session-ending
//! cascade, so a stale attempt is expired by the next transition or the
//! sweep, never advanced.

use chrono::Duration;

/// The QR token changes this often while the attempt is `waiting`.
pub const QR_ROTATE_SECS: i64 = 60;
/// The longest an attempt stays in `waiting` or `claimed`.
pub const ATTEMPT_MINUTES: i64 = 10;
/// Lifetime of the `pc_setup` challenge, from the scan.
pub const CHALLENGE_TTL: Duration = Duration::minutes(2);
/// The code is valid this long from the approval.
pub const CODE_MINUTES: i64 = 5;
/// Wrong codes that end the attempt.
pub const MAX_CODE_TRIES: i16 = 3;
/// The code is this many digits.
pub const CODE_DIGITS: usize = 6;
/// The enrolment lives this long from the right code.
pub const ENROLLMENT_MINUTES: i64 = 10;

/// SM5 states. `waiting`, `claimed`, `code` and `enrolled` are live; every
/// other state is terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AttemptState {
    Waiting,
    Claimed,
    Code,
    Enrolled,
    Completed,
    Refused,
    Reported,
    Failed,
    Cancelled,
    Expired,
}

impl AttemptState {
    /// Every state, live ones first.
    pub const ALL: [Self; 10] = [
        Self::Waiting,
        Self::Claimed,
        Self::Code,
        Self::Enrolled,
        Self::Completed,
        Self::Refused,
        Self::Reported,
        Self::Failed,
        Self::Cancelled,
        Self::Expired,
    ];

    /// The database / JSON form.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Waiting => "waiting",
            Self::Claimed => "claimed",
            Self::Code => "code",
            Self::Enrolled => "enrolled",
            Self::Completed => "completed",
            Self::Refused => "refused",
            Self::Reported => "reported",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Expired => "expired",
        }
    }

    /// Parse the database / JSON form.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|st| st.as_str() == s)
    }

    /// No transition leaves a terminal state.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        !matches!(
            self,
            Self::Waiting | Self::Claimed | Self::Code | Self::Enrolled
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attempt_states_follow_sm5() {
        let live: Vec<_> = AttemptState::ALL
            .into_iter()
            .filter(|s| !s.is_terminal())
            .collect();
        assert_eq!(
            live,
            [
                AttemptState::Waiting,
                AttemptState::Claimed,
                AttemptState::Code,
                AttemptState::Enrolled
            ]
        );
        for s in AttemptState::ALL {
            assert_eq!(AttemptState::parse(s.as_str()), Some(s));
        }
        assert_eq!(AttemptState::parse("approved"), None);
    }
}
