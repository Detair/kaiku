//! Content Filter Engine
//!
//! Hybrid Aho-Corasick + regex engine for content filtering.
//! Aho-Corasick handles keyword matching (fast path), regex handles
//! pattern-based rules.

use aho_corasick::AhoCorasick;
use regex::Regex;
use uuid::Uuid;

use super::defaults;
use super::filter_types::{
    FilterAction, FilterCategory, FilterMatch, FilterResult, GuildFilterConfig, GuildFilterPattern,
};

/// Metadata for a keyword in the Aho-Corasick automaton.
#[derive(Debug)]
struct KeywordMeta {
    category: FilterCategory,
    action: FilterAction,
}

/// A compiled regex pattern with metadata.
#[derive(Debug)]
struct CompiledPattern {
    id: Option<Uuid>,
    regex: Regex,
    category: FilterCategory,
    action: FilterAction,
    source: String,
}

/// Content filter engine combining Aho-Corasick keyword matching with regex patterns.
pub struct FilterEngine {
    keyword_matcher: Option<AhoCorasick>,
    keyword_meta: Vec<KeywordMeta>,
    keyword_strings: Vec<String>,
    regex_patterns: Vec<CompiledPattern>,
}

impl FilterEngine {
    /// Build a filter engine from guild config and custom patterns.
    ///
    /// Loads enabled built-in categories, merges with custom patterns,
    /// and compiles the Aho-Corasick automaton and regex patterns.
    pub fn build(
        configs: &[GuildFilterConfig],
        custom_patterns: &[GuildFilterPattern],
    ) -> Result<Self, String> {
        let mut keywords: Vec<String> = Vec::new();
        let mut keyword_meta: Vec<KeywordMeta> = Vec::new();
        let mut regex_patterns: Vec<CompiledPattern> = Vec::new();

        // Load enabled built-in categories
        for config in configs {
            if !config.enabled {
                continue;
            }

            // Add keywords from built-in lists
            for kw in defaults::default_keywords(config.category) {
                keywords.push(kw.to_lowercase());
                keyword_meta.push(KeywordMeta {
                    category: config.category,
                    action: config.action,
                });
            }

            // Add regex patterns from built-in lists
            for pat in defaults::default_patterns(config.category) {
                match Regex::new(pat) {
                    Ok(regex) => {
                        regex_patterns.push(CompiledPattern {
                            id: None,
                            regex,
                            category: config.category,
                            action: config.action,
                            source: pat.to_string(),
                        });
                    }
                    Err(e) => {
                        tracing::warn!(
                            pattern = pat,
                            error = %e,
                            "Failed to compile built-in regex pattern, skipping"
                        );
                    }
                }
            }
        }

        // Add custom guild patterns
        for pattern in custom_patterns {
            if !pattern.enabled {
                continue;
            }

            if pattern.is_regex {
                match Regex::new(&pattern.pattern) {
                    Ok(regex) => {
                        regex_patterns.push(CompiledPattern {
                            id: Some(pattern.id),
                            regex,
                            category: FilterCategory::Custom,
                            action: FilterAction::Block,
                            source: pattern.pattern.clone(),
                        });
                    }
                    Err(e) => {
                        tracing::warn!(
                            pattern_id = %pattern.id,
                            pattern = %pattern.pattern,
                            error = %e,
                            "Failed to compile custom regex pattern, skipping"
                        );
                    }
                }
            } else {
                keywords.push(pattern.pattern.to_lowercase());
                keyword_meta.push(KeywordMeta {
                    category: FilterCategory::Custom,
                    action: FilterAction::Block,
                });
            }
        }

        // Build Aho-Corasick automaton if we have keywords
        let keyword_matcher = if keywords.is_empty() {
            None
        } else {
            Some(
                AhoCorasick::builder()
                    .ascii_case_insensitive(true)
                    .build(&keywords)
                    .map_err(|e| format!("Failed to build Aho-Corasick automaton: {e}"))?,
            )
        };

        Ok(Self {
            keyword_matcher,
            keyword_meta,
            keyword_strings: keywords,
            regex_patterns,
        })
    }

    /// Check content against all active filters.
    ///
    /// Runs Aho-Corasick first (fast path), then regex patterns.
    /// Returns all matches with the highest-priority action determining `blocked`.
    pub fn check(&self, content: &str) -> FilterResult {
        let mut matches = Vec::new();
        let content_lower = content.to_lowercase();

        // Aho-Corasick keyword matching
        if let Some(ref matcher) = self.keyword_matcher {
            // Track which keyword indices already matched to avoid duplicates
            let mut seen = std::collections::HashSet::new();

            for mat in matcher.find_iter(&content_lower) {
                let idx = mat.pattern().as_usize();
                if seen.insert(idx) {
                    let meta = &self.keyword_meta[idx];
                    matches.push(FilterMatch {
                        category: meta.category,
                        action: meta.action,
                        matched_pattern: self.keyword_strings[idx].clone(),
                        custom_pattern_id: None,
                    });
                }
            }
        }

        // Regex pattern matching
        for pattern in &self.regex_patterns {
            if pattern.regex.is_match(content) {
                matches.push(FilterMatch {
                    category: pattern.category,
                    action: pattern.action,
                    matched_pattern: pattern.source.clone(),
                    custom_pattern_id: pattern.id,
                });
            }
        }

        let blocked = matches.iter().any(|m| m.action == FilterAction::Block);

        FilterResult { blocked, matches }
    }

    /// Returns true if this engine has no active filters.
    pub const fn is_empty(&self) -> bool {
        self.keyword_matcher.is_none() && self.regex_patterns.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_config(
        category: FilterCategory,
        action: FilterAction,
        enabled: bool,
    ) -> GuildFilterConfig {
        GuildFilterConfig {
            id: Uuid::new_v4(),
            guild_id: Uuid::new_v4(),
            category,
            enabled,
            action,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    fn make_custom_pattern(pattern: &str, is_regex: bool) -> GuildFilterPattern {
        GuildFilterPattern {
            id: Uuid::new_v4(),
            guild_id: Uuid::new_v4(),
            pattern: pattern.to_string(),
            is_regex,
            description: None,
            enabled: true,
            created_by: Uuid::new_v4(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn empty_engine_allows_everything() {
        let engine = FilterEngine::build(&[], &[]).unwrap();
        let result = engine.check("hello world");
        assert!(!result.blocked);
        assert!(result.matches.is_empty());
        assert!(engine.is_empty());
    }

    #[test]
    fn custom_keyword_blocks() {
        let pattern = make_custom_pattern("badword", false);
        let engine = FilterEngine::build(&[], &[pattern]).unwrap();

        let result = engine.check("this has a badword in it");
        assert!(result.blocked);
        assert_eq!(result.matches.len(), 1);
        assert_eq!(result.matches[0].matched_pattern, "badword");
    }

    #[test]
    fn custom_keyword_case_insensitive() {
        let pattern = make_custom_pattern("BadWord", false);
        let engine = FilterEngine::build(&[], &[pattern]).unwrap();

        let result = engine.check("BADWORD is here");
        assert!(result.blocked);
    }

    #[test]
    fn custom_regex_blocks() {
        let pattern = make_custom_pattern(r"(?i)free\s+money", true);
        let engine = FilterEngine::build(&[], &[pattern]).unwrap();

        let result = engine.check("get FREE MONEY now!");
        assert!(result.blocked);
        assert_eq!(result.matches.len(), 1);
    }

    #[test]
    fn disabled_pattern_skipped() {
        let mut pattern = make_custom_pattern("badword", false);
        pattern.enabled = false;
        let engine = FilterEngine::build(&[], &[pattern]).unwrap();

        let result = engine.check("this has a badword");
        assert!(!result.blocked);
    }

    #[test]
    fn clean_content_passes() {
        let pattern = make_custom_pattern("badword", false);
        let engine = FilterEngine::build(&[], &[pattern]).unwrap();

        let result = engine.check("this is perfectly fine");
        assert!(!result.blocked);
        assert!(result.matches.is_empty());
    }

    #[test]
    fn invalid_regex_skipped() {
        let pattern = make_custom_pattern("[invalid", true);
        let engine = FilterEngine::build(&[], &[pattern]).unwrap();
        assert!(engine.is_empty());
    }

    #[test]
    fn builtin_spam_patterns() {
        let config = make_config(FilterCategory::Spam, FilterAction::Block, true);
        let engine = FilterEngine::build(&[config], &[]).unwrap();

        let result = engine.check("click here to claim your prize!");
        assert!(result.blocked);
    }

    // ------------------------------------------------------------------
    // Baseline wordlist coverage (TD-26): every built-in category must
    // compile, block its canonical cases, and pass innocent content.
    // ------------------------------------------------------------------

    #[test]
    fn all_builtin_categories_compile_together() {
        let configs: Vec<_> = [
            FilterCategory::Slurs,
            FilterCategory::HateSpeech,
            FilterCategory::Spam,
            FilterCategory::AbusiveLanguage,
        ]
        .into_iter()
        .map(|c| make_config(c, FilterAction::Block, true))
        .collect();
        let engine = FilterEngine::build(&configs, &[]).unwrap();
        assert!(!engine.is_empty(), "baseline lists must not be empty");
    }

    #[test]
    fn builtin_spam_blocks_scams_and_flood() {
        let config = make_config(FilterCategory::Spam, FilterAction::Block, true);
        let engine = FilterEngine::build(&[config], &[]).unwrap();

        for msg in [
            "FREE NITRO just click bit.ly/abc123",
            "double your crypto today",
            "verify your account now or lose access",
        ] {
            assert!(engine.check(msg).blocked, "should block: {msg}");
        }
        assert!(!engine.check("let's play tonight at 8").blocked);
    }

    #[test]
    fn builtin_abusive_blocks_harassment_and_evasion() {
        let config = make_config(FilterCategory::AbusiveLanguage, FilterAction::Block, true);
        let engine = FilterEngine::build(&[config], &[]).unwrap();

        for msg in [
            "just kill yourself already",
            "k y s",
            "k.y.s.",
            "i'll kill you",
            "you deserve to die",
        ] {
            assert!(engine.check(msg).blocked, "should block: {msg}");
        }
        // Innocent content containing risky substrings must pass
        for msg in [
            "the kyoto summit starts tomorrow", // contains k-y-s letters apart
            "that boss fight nearly killed me",
            "skys are clear tonight",
        ] {
            assert!(!engine.check(msg).blocked, "false positive on: {msg}");
        }
    }

    #[test]
    fn builtin_hate_speech_blocks_advocacy_not_numbers() {
        let config = make_config(FilterCategory::HateSpeech, FilterAction::Block, true);
        let engine = FilterEngine::build(&[config], &[]).unwrap();

        assert!(engine.check("sieg heil").blocked);
        assert!(engine.check("1488").blocked);
        // boundary anchoring: 1488 inside a longer number must pass
        assert!(!engine.check("order #314889 shipped").blocked);
        assert!(!engine.check("the year 1487 was uneventful").blocked);
    }

    #[test]
    fn builtin_slurs_block_with_evasion_but_not_scunthorpe() {
        let config = make_config(FilterCategory::Slurs, FilterAction::Block, true);
        let engine = FilterEngine::build(&[config], &[]).unwrap();

        // Canonical and leetspeak forms (constructed in-test, not stored)
        let canonical = ["n", "i", "g", "g", "e", "r"].concat();
        let leet = ["n", "1", "g", "g", "3", "r"].concat();
        let f_slur = ["f", "a", "g", "g", "o", "t"].concat();
        for msg in [canonical, leet, f_slur] {
            assert!(engine.check(&msg).blocked, "should block slur variant");
        }
        // Scunthorpe guards: innocent words containing risky substrings
        for msg in ["the spice must flow", "chinking sound of coins"] {
            assert!(!engine.check(msg).blocked, "false positive on: {msg}");
        }
    }

    #[test]
    fn disabled_config_skipped() {
        let config = make_config(FilterCategory::Spam, FilterAction::Block, false);
        let engine = FilterEngine::build(&[config], &[]).unwrap();

        let result = engine.check("click here to claim your prize!");
        assert!(!result.blocked);
    }
}
