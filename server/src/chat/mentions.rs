//! @mention parsing and per-user mention targets (contract C21).
//!
//! A mention is `@username` where:
//! - the `@` starts the text or follows whitespace or one of `( [ { < " ' ,`;
//! - the username is `[A-Za-z0-9_]{3,32}` (compared lowercased) and is not directly followed by
//!   another letter, digit or `_`;
//! - it is not inside a fenced (```` ``` ````) or inline (`` ` ``) code span.
//!
//! `@everyone` and `@here` follow the same boundary rule. Push targets are
//! resolved per recipient by [`resolve_mention_targets`].

use std::collections::HashSet;
use std::sync::LazyLock;

use uuid::Uuid;

use crate::api::AppState;
use crate::social::block_cache;
use crate::{db, permissions};

/// Maximum number of distinct usernames taken from one message.
pub const MAX_MENTIONED_USERNAMES: usize = 20;

/// Maximum number of users one message can push via mentions.
pub const MENTION_EVERYONE_PUSH_CAP: usize = 100;

const MIN_USERNAME_LEN: usize = 3;
const MAX_USERNAME_LEN: usize = 32;

/// Fenced code blocks; an unclosed fence runs to the end of the text.
static FENCED_CODE_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"(?s)```.*?(?:```|\z)").expect("valid fence regex"));

/// Inline code spans.
static INLINE_CODE_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"`[^`]*`").expect("valid inline code regex"));

/// Mentions found in a message.
///
/// `usernames` are lowercased, deduplicated in first-seen order and capped at
/// [`MAX_MENTIONED_USERNAMES`]; `everyone`/`here` are never in the list.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct MentionSet {
    pub everyone: bool,
    pub here: bool,
    pub usernames: Vec<String>,
}

const fn is_boundary(c: char) -> bool {
    c.is_whitespace() || matches!(c, '(' | '[' | '{' | '<' | '"' | '\'' | ',')
}

const fn is_username_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Extract the mentions in `content` (C21 rules).
pub fn extract_mentions(content: &str) -> MentionSet {
    let without_fences = FENCED_CODE_RE.replace_all(content, " ");
    let text = INLINE_CODE_RE.replace_all(&without_fences, " ");

    let mut set = MentionSet::default();
    let mut prev: Option<char> = None;
    let mut chars = text.char_indices().peekable();

    while let Some((i, c)) = chars.next() {
        let at_boundary = prev.is_none_or(is_boundary);
        prev = Some(c);
        if c != '@' || !at_boundary {
            continue;
        }

        let start = i + 1;
        let mut end = start;
        while let Some(&(j, n)) = chars.peek() {
            if !is_username_char(n) {
                break;
            }
            end = j + n.len_utf8();
            prev = Some(n);
            chars.next();
        }
        // Reject a run glued to a non-ASCII letter/digit (e.g. `@anné`).
        if chars
            .peek()
            .is_some_and(|&(_, n)| n.is_alphanumeric() || n == '_')
        {
            continue;
        }

        let len = end - start;
        if !(MIN_USERNAME_LEN..=MAX_USERNAME_LEN).contains(&len) {
            continue;
        }
        let name = text[start..end].to_ascii_lowercase();
        match name.as_str() {
            "everyone" => set.everyone = true,
            "here" => set.here = true,
            _ => {
                if set.usernames.len() < MAX_MENTIONED_USERNAMES && !set.usernames.contains(&name) {
                    set.usernames.push(name);
                }
            }
        }
    }

    set
}

/// Users that a message by `author_id` in `channel` should push as a mention.
///
/// Only guild channels produce targets. Candidates are the mentioned
/// usernames plus, for `@everyone`, the guild members (bounded query). The
/// author, users without access to the channel and users with a block in
/// either direction are dropped; a failed block lookup also drops the user.
/// `@here` contributes nothing. The result is capped at
/// [`MENTION_EVERYONE_PUSH_CAP`].
#[tracing::instrument(skip(state, channel, mentions), fields(channel_id = %channel.id))]
pub async fn resolve_mention_targets(
    state: &AppState,
    channel: &db::Channel,
    author_id: Uuid,
    mentions: &MentionSet,
) -> Vec<Uuid> {
    let Some(guild_id) = channel.guild_id else {
        return Vec::new();
    };

    let mut candidates: Vec<Uuid> = Vec::new();

    if !mentions.usernames.is_empty() {
        match sqlx::query_scalar::<_, Uuid>("SELECT id FROM users WHERE username = ANY($1)")
            .bind(&mentions.usernames)
            .fetch_all(&state.db)
            .await
        {
            Ok(ids) => candidates.extend(ids),
            Err(e) => tracing::warn!(error = %e, "Failed to resolve mentioned usernames"),
        }
    }

    if mentions.everyone {
        match sqlx::query_scalar::<_, Uuid>(
            "SELECT user_id FROM guild_members WHERE guild_id = $1 \
             ORDER BY joined_at, user_id LIMIT 101",
        )
        .bind(guild_id)
        .fetch_all(&state.db)
        .await
        {
            Ok(ids) => candidates.extend(ids),
            Err(e) => tracing::warn!(error = %e, "Failed to load guild members for @everyone"),
        }
    }

    let mut seen = HashSet::new();
    let mut targets = Vec::new();
    for uid in candidates {
        if targets.len() >= MENTION_EVERYONE_PUSH_CAP {
            break;
        }
        if uid == author_id || !seen.insert(uid) {
            continue;
        }
        if permissions::require_channel_access(&state.db, uid, channel.id)
            .await
            .is_err()
        {
            continue;
        }
        match block_cache::is_blocked_either_direction(&state.redis, author_id, uid).await {
            Ok(false) => targets.push(uid),
            Ok(true) => {}
            Err(e) => {
                tracing::warn!(error = %e, "Block check failed; skipping mention target");
            }
        }
    }

    targets
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::types::detect_mention_type;

    #[test]
    fn plain_mentions() {
        assert_eq!(
            extract_mentions("hi @Fabian and @lukas!").usernames,
            ["fabian", "lukas"]
        );
    }

    #[test]
    fn email_is_not_a_mention() {
        assert!(extract_mentions("mail me: foo@bar.com")
            .usernames
            .is_empty());
    }

    #[test]
    fn code_is_not_a_mention() {
        assert!(extract_mentions("`@root` and\n```\n@admin\n```")
            .usernames
            .is_empty());
    }

    #[test]
    fn punctuation_boundaries() {
        assert_eq!(
            extract_mentions("(@anna), \"@ben\"").usernames,
            ["anna", "ben"]
        );
    }

    #[test]
    fn too_short_or_long_ignored() {
        let long = "a".repeat(33);
        assert!(extract_mentions(&format!("@ab @{long}"))
            .usernames
            .is_empty());
    }

    #[test]
    fn everyone_needs_boundary() {
        assert!(extract_mentions("@everyone dinner").everyone);
        assert!(!extract_mentions("x@everyone.com").everyone);
    }

    #[test]
    fn detect_mention_type_ignores_email() {
        assert_eq!(detect_mention_type("a@b.com", Some("me")), None);
    }

    #[test]
    fn detect_mention_type_skips_self() {
        assert_eq!(detect_mention_type("@me", Some("me")), None);
    }
}
