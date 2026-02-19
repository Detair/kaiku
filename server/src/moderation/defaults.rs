//! Built-in Word Lists
//!
//! Embeds default filter word lists from the `wordlists/` directory.
//! Each category has keywords (plain text) and patterns (regex).

use super::filter_types::FilterCategory;

static SLURS_TXT: &str = include_str!("wordlists/slurs.txt");
static HATE_SPEECH_TXT: &str = include_str!("wordlists/hate_speech.txt");
static SPAM_PATTERNS_TXT: &str = include_str!("wordlists/spam_patterns.txt");
static ABUSIVE_TXT: &str = include_str!("wordlists/abusive.txt");

/// Parse a word list file into keywords and regex patterns.
///
/// Lines starting with `#` are comments.
/// Lines starting with `regex:` are regex patterns.
/// All other non-empty lines are plain keywords.
fn parse_wordlist(content: &str) -> (Vec<&str>, Vec<&str>) {
    let mut keywords = Vec::new();
    let mut patterns = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(pattern) = line.strip_prefix("regex:") {
            let pattern = pattern.trim();
            if !pattern.is_empty() {
                patterns.push(pattern);
            }
        } else {
            keywords.push(line);
        }
    }

    (keywords, patterns)
}

/// Get the raw text for a built-in category.
fn category_text(category: FilterCategory) -> &'static str {
    match category {
        FilterCategory::Slurs => SLURS_TXT,
        FilterCategory::HateSpeech => HATE_SPEECH_TXT,
        FilterCategory::Spam => SPAM_PATTERNS_TXT,
        FilterCategory::AbusiveLanguage => ABUSIVE_TXT,
        FilterCategory::Custom => "",
    }
}

/// Get default keywords for a built-in category.
pub fn default_keywords(category: FilterCategory) -> Vec<&'static str> {
    let (keywords, _) = parse_wordlist(category_text(category));
    keywords
}

/// Get default regex patterns for a built-in category.
pub fn default_patterns(category: FilterCategory) -> Vec<&'static str> {
    let (_, patterns) = parse_wordlist(category_text(category));
    patterns
}
