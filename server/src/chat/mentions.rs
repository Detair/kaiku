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

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use uuid::Uuid;

use crate::api::AppState;
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

/// Replace inline code spans with a space (`CommonMark` backtick runs).
///
/// A run of N backticks opens a span that closes at the next run of exactly N
/// backticks; everything in between is code. A run without a matching closer
/// is literal text. Runs in linear time: per-length cursors only move forward.
fn strip_inline_code(text: &str) -> String {
    let bytes = text.as_bytes();
    // (start, end) byte ranges of every backtick run.
    let mut runs: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'`' {
            let start = i;
            while i < bytes.len() && bytes[i] == b'`' {
                i += 1;
            }
            runs.push((start, i));
        } else {
            i += 1;
        }
    }
    if runs.len() < 2 {
        return text.to_owned();
    }

    // Run indices grouped by run length, in order.
    let mut by_len: HashMap<usize, Vec<usize>> = HashMap::new();
    for (idx, &(start, end)) in runs.iter().enumerate() {
        by_len.entry(end - start).or_default().push(idx);
    }
    let mut cursor: HashMap<usize, usize> = HashMap::new();

    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    let mut r = 0;
    while r < runs.len() {
        let (start, end) = runs[r];
        let len = end - start;
        let list = &by_len[&len];
        let pos = cursor.entry(len).or_insert(0);
        while *pos < list.len() && list[*pos] <= r {
            *pos += 1;
        }
        if let Some(&closer) = list.get(*pos) {
            out.push_str(&text[copied..start]);
            out.push(' ');
            copied = runs[closer].1;
            r = closer + 1;
        } else {
            r += 1;
        }
    }
    out.push_str(&text[copied..]);
    out
}

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
    let text = strip_inline_code(&without_fences);

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
/// either direction are dropped. Blocks come from one batched Postgres
/// `friendships` query, the source of truth (the Redis block cache is not
/// consulted); if that query fails, there are no targets (fail closed).
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
    candidates.retain(|&uid| uid != author_id && seen.insert(uid));
    if candidates.is_empty() {
        return Vec::new();
    }

    // Postgres is the source of truth for blocks (the Redis cache expires).
    // One batched query covers both directions; any error fails closed.
    let blocked: HashSet<Uuid> = match sqlx::query_scalar::<_, Uuid>(
        "SELECT CASE WHEN requester_id = $1 THEN addressee_id ELSE requester_id END \
         FROM friendships \
         WHERE status = 'blocked' \
           AND ((requester_id = $1 AND addressee_id = ANY($2)) \
             OR (addressee_id = $1 AND requester_id = ANY($2)))",
    )
    .bind(author_id)
    .bind(&candidates)
    .fetch_all(&state.db)
    .await
    {
        Ok(ids) => ids.into_iter().collect(),
        Err(e) => {
            tracing::warn!(error = %e, "Block lookup failed; no mention targets");
            return Vec::new();
        }
    };

    let mut targets = Vec::new();
    for uid in candidates {
        if targets.len() >= MENTION_EVERYONE_PUSH_CAP {
            break;
        }
        if blocked.contains(&uid) {
            continue;
        }
        if permissions::require_channel_access(&state.db, uid, channel.id)
            .await
            .is_ok()
        {
            targets.push(uid);
        }
    }

    targets
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::types::{detect_mention_type, MentionType};

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
        assert_eq!(detect_mention_type("@author", Some("author")), None);
        assert_eq!(detect_mention_type("@Author", Some("author")), None);
        assert_eq!(
            detect_mention_type("@author", Some("someone")),
            Some(MentionType::Direct)
        );
    }

    #[test]
    fn length_edges_accepted() {
        let max = "b".repeat(32);
        assert_eq!(
            extract_mentions(&format!("@abc @{max}")).usernames,
            ["abc".to_string(), max]
        );
    }

    #[test]
    fn double_backtick_span_is_code() {
        let m = extract_mentions("``@anna`` and ``@everyone``");
        assert!(m.usernames.is_empty());
        assert!(!m.everyone);
    }

    #[test]
    fn embedded_backtick_stays_in_span() {
        assert!(extract_mentions("`` a ` @anna ``").usernames.is_empty());
        assert_eq!(extract_mentions("`` a ` b `` @ben").usernames, ["ben"]);
    }

    #[test]
    fn unclosed_backtick_run_is_literal() {
        assert_eq!(extract_mentions("`` @anna").usernames, ["anna"]);
        assert_eq!(extract_mentions("` @anna").usernames, ["anna"]);
        assert_eq!(extract_mentions("``x` @anna").usernames, ["anna"]);
    }

    #[test]
    fn single_backticks_do_not_close_double() {
        assert_eq!(
            extract_mentions("``code` `more`` @cleo").usernames,
            ["cleo"]
        );
    }
}
