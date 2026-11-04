//! Password policy (spec B.1 "Passwords", CC8; NIST SP 800-63B §3.1.1.2).
//!
//! Passwords are optional. When one is set it must be 8..128 characters,
//! must not be on the offline blocklist (the `SecLists` top 100 000 breached
//! passwords, DC14) and must not contain a context word (`kaiku`, the
//! server name, the username or the display name; case-insensitive, words
//! of at least 4 characters). There are no composition rules.

use std::collections::HashSet;
use std::io::Read;
use std::sync::LazyLock;

/// What the password is checked against besides the blocklist.
#[derive(Debug, Clone, Copy)]
pub struct PolicyContext<'a> {
    pub username: &'a str,
    pub display_name: &'a str,
    pub server_name: &'a str,
}

/// Why a password was refused: 400 `PASSWORD_REJECTED {reason}` (C27).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasswordRejected {
    Length,
    Breached,
    Context,
}

impl PasswordRejected {
    /// The wire `reason`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Length => "length",
            Self::Breached => "breached",
            Self::Context => "context",
        }
    }
}

/// Minimum length in characters.
pub const MIN_CHARS: usize = 8;
/// Maximum length in characters.
pub const MAX_CHARS: usize = 128;
/// Context words shorter than this are ignored.
const MIN_CONTEXT_WORD: usize = 4;

/// The bundled blocklist: `SecLists`
/// `Passwords/Common-Credentials/xato-net-10-million-passwords-100000.txt`
/// (MIT, formerly `10-million-password-list-top-100000.txt`), gzip-compressed.
static BLOCKLIST_GZ: &[u8] = include_bytes!("../../data/password-blocklist-top100k.txt.gz");

/// The blocklist as lower-cased entries, loaded once.
static BLOCKLIST: LazyLock<HashSet<String>> = LazyLock::new(|| {
    let mut text = String::new();
    flate2::read::GzDecoder::new(BLOCKLIST_GZ)
        .read_to_string(&mut text)
        .expect("the bundled password blocklist is valid gzip UTF-8");
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_lowercase)
        .collect()
});

/// Load the blocklist now (startup), so the first password check does not
/// pay for it.
pub fn preload() {
    LazyLock::force(&BLOCKLIST);
}

/// The context words of `ctx`: `kaiku` and each name, whole and split at
/// whitespace, lower-cased, ≥ 4 characters.
fn context_words(ctx: &PolicyContext<'_>) -> Vec<String> {
    let mut out = vec!["kaiku".to_string()];
    for name in [ctx.username, ctx.display_name, ctx.server_name] {
        let lower = name.trim().to_lowercase();
        out.extend(lower.split_whitespace().map(str::to_string));
        out.push(lower.split_whitespace().collect::<String>());
        out.push(lower);
    }
    out.retain(|w| w.chars().count() >= MIN_CONTEXT_WORD);
    out.sort_unstable();
    out.dedup();
    out
}

/// Check `password` against the policy.
pub fn check(password: &str, ctx: &PolicyContext<'_>) -> Result<(), PasswordRejected> {
    let chars = password.chars().count();
    if !(MIN_CHARS..=MAX_CHARS).contains(&chars) {
        return Err(PasswordRejected::Length);
    }
    let lower = password.to_lowercase();
    if BLOCKLIST.contains(&lower) {
        return Err(PasswordRejected::Breached);
    }
    let squeezed: String = lower.split_whitespace().collect();
    if context_words(ctx)
        .iter()
        .any(|w| lower.contains(w.as_str()) || squeezed.contains(w.as_str()))
    {
        return Err(PasswordRejected::Context);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const CTX: PolicyContext<'static> = PolicyContext {
        username: "oma_erna",
        display_name: "Erna Müller",
        server_name: "Familie Müller",
    };

    #[test]
    fn length_8_to_128() {
        assert_eq!(check("short12", &CTX), Err(PasswordRejected::Length));
        assert_eq!(check("", &CTX), Err(PasswordRejected::Length));
        assert_eq!(check("zq8#vT4p", &CTX), Ok(()));
        let max: String = "zq8#vT4p".repeat(16);
        assert_eq!(max.chars().count(), 128);
        assert_eq!(check(&max, &CTX), Ok(()));
        let over = format!("{max}x");
        assert_eq!(check(&over, &CTX), Err(PasswordRejected::Length));
        // Characters, not bytes: 8 umlauts are 16 bytes.
        assert_eq!(check("öäüöäüöß", &CTX), Ok(()));
    }

    #[test]
    fn breached_password_rejected() {
        assert_eq!(check("iloveyou", &CTX), Err(PasswordRejected::Breached));
        assert_eq!(check("ILoveYou", &CTX), Err(PasswordRejected::Breached));
        assert_eq!(check("password1", &CTX), Err(PasswordRejected::Breached));
        assert_eq!(check("12345678", &CTX), Err(PasswordRejected::Breached));
    }

    #[test]
    fn context_words_rejected() {
        assert_eq!(check("Kaiku2026!", &CTX), Err(PasswordRejected::Context));
        assert_eq!(check("xxOMA_ERNAxx", &CTX), Err(PasswordRejected::Context));
        assert_eq!(
            check("ich-bin-müller-77", &CTX),
            Err(PasswordRejected::Context)
        );
        assert_eq!(
            check("FamilieMüller!", &CTX),
            Err(PasswordRejected::Context)
        );
        assert_eq!(
            check("erna-is-great-1", &CTX),
            Err(PasswordRejected::Context)
        );
        // Words under 4 characters do not count.
        let short = PolicyContext {
            username: "bob",
            display_name: "Al",
            server_name: "K",
        };
        assert_eq!(check("bob-al-k-zq8#vT4p", &short), Ok(()));
    }

    #[test]
    fn no_composition_rules() {
        assert_eq!(check("correct horse battery staple", &CTX), Ok(()));
        assert_eq!(check("zzqqxxvvyy", &CTX), Ok(()));
    }
}
