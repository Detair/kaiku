//! Validation logic for the preferences payload.

use uuid::Uuid;

use super::error::PreferencesError;

/// Maximum total size of the preferences JSON payload (64 KiB).
pub(super) const MAX_PREFERENCES_SIZE: usize = 65_536;

/// Limits for the focus section of preferences.
const MAX_FOCUS_MODES: usize = 10;
const MAX_VIP_ENTRIES: usize = 50;
const MAX_KEYWORDS: usize = 5;
/// Maximum length for VIP user/channel ID strings.
const MAX_ID_LEN: usize = 36; // UUID format: 8-4-4-4-12
/// Maximum length for emergency keyword strings.
const MAX_KEYWORD_LEN: usize = 30;
/// Minimum length for emergency keywords (prevents overly broad matches).
const MIN_KEYWORD_LEN: usize = 3;
const MAX_MODE_NAME_LEN: usize = 30;

/// Counts Unicode scalar values (code points), matching `Array.from(str).length` in JavaScript.
fn unicode_len(s: &str) -> usize {
    s.chars().count()
}

const VALID_SUPPRESSION_LEVELS: &[&str] = &["all", "except_mentions", "except_dms"];
const VALID_TRIGGER_CATEGORIES: &[&str] = &["game", "coding", "listening", "watching"];

/// Validate the preferences payload: total size limit and focus section structure.
pub fn validate_preferences(prefs: &serde_json::Value) -> Result<(), PreferencesError> {
    // Total size limit
    let serialized_len = serde_json::to_string(prefs).unwrap_or_default().len();
    if serialized_len > MAX_PREFERENCES_SIZE {
        return Err(PreferencesError::Validation(format!(
            "Preferences payload too large ({serialized_len} bytes, max {MAX_PREFERENCES_SIZE})"
        )));
    }

    // Validate focus section if present
    if let Some(focus) = prefs.get("focus") {
        validate_focus_preferences(focus)?;
    }

    Ok(())
}

fn validate_focus_preferences(focus: &serde_json::Value) -> Result<(), PreferencesError> {
    // modes array
    if let Some(modes) = focus.get("modes") {
        let modes = modes
            .as_array()
            .ok_or_else(|| PreferencesError::Validation("focus.modes must be an array".into()))?;

        if modes.len() > MAX_FOCUS_MODES {
            return Err(PreferencesError::Validation(format!(
                "Too many focus modes ({}, max {MAX_FOCUS_MODES})",
                modes.len()
            )));
        }

        for (i, mode) in modes.iter().enumerate() {
            validate_focus_mode(mode, i)?;
        }
    }

    Ok(())
}

fn validate_focus_mode(mode: &serde_json::Value, index: usize) -> Result<(), PreferencesError> {
    let ctx = |field: &str| format!("focus.modes[{index}].{field}");

    // Name length
    if let Some(name) = mode.get("name").and_then(|v| v.as_str()) {
        if name.trim().is_empty() {
            return Err(PreferencesError::Validation(format!(
                "{} must not be empty",
                ctx("name")
            )));
        }
        let name_len = unicode_len(name);
        if name_len > MAX_MODE_NAME_LEN {
            return Err(PreferencesError::Validation(format!(
                "{} too long ({}, max {MAX_MODE_NAME_LEN})",
                ctx("name"),
                name_len
            )));
        }
    }

    // Suppression level must be a known value
    if let Some(level) = mode.get("suppression_level").and_then(|v| v.as_str()) {
        if !VALID_SUPPRESSION_LEVELS.contains(&level) {
            return Err(PreferencesError::Validation(format!(
                "{} invalid value: {level}",
                ctx("suppression_level")
            )));
        }
    }

    // Trigger categories
    if let Some(cats) = mode.get("trigger_categories") {
        if !cats.is_null() {
            let cats = cats.as_array().ok_or_else(|| {
                PreferencesError::Validation(format!(
                    "{} must be an array or null",
                    ctx("trigger_categories")
                ))
            })?;
            for cat in cats {
                let s = cat.as_str().ok_or_else(|| {
                    PreferencesError::Validation(format!(
                        "{} entries must be strings",
                        ctx("trigger_categories")
                    ))
                })?;
                if !VALID_TRIGGER_CATEGORIES.contains(&s) {
                    return Err(PreferencesError::Validation(format!(
                        "{} invalid category: {s}",
                        ctx("trigger_categories")
                    )));
                }
            }
        }
    }

    // VIP user IDs (must be valid UUIDs)
    validate_uuid_array(mode, "vip_user_ids", MAX_VIP_ENTRIES, &ctx("vip_user_ids"))?;

    // VIP channel IDs (must be valid UUIDs)
    validate_uuid_array(
        mode,
        "vip_channel_ids",
        MAX_VIP_ENTRIES,
        &ctx("vip_channel_ids"),
    )?;

    // Emergency keywords (min 3 chars, max 30 chars)
    validate_keyword_array(
        mode,
        "emergency_keywords",
        MAX_KEYWORDS,
        &ctx("emergency_keywords"),
    )?;

    Ok(())
}

/// Validate an array of UUID strings (for VIP user/channel IDs).
fn validate_uuid_array(
    obj: &serde_json::Value,
    field: &str,
    max_len: usize,
    ctx: &str,
) -> Result<(), PreferencesError> {
    if let Some(arr) = obj.get(field) {
        let arr = arr
            .as_array()
            .ok_or_else(|| PreferencesError::Validation(format!("{ctx} must be an array")))?;

        if arr.len() > max_len {
            return Err(PreferencesError::Validation(format!(
                "{ctx} too many entries ({}, max {max_len})",
                arr.len()
            )));
        }

        for entry in arr {
            let s = entry.as_str().ok_or_else(|| {
                PreferencesError::Validation(format!("{ctx} entries must be strings"))
            })?;
            if s.len() > MAX_ID_LEN {
                return Err(PreferencesError::Validation(format!(
                    "{ctx} entry too long ({}, max {MAX_ID_LEN})",
                    s.len()
                )));
            }
            if s.parse::<Uuid>().is_err() {
                return Err(PreferencesError::Validation(format!(
                    "{ctx} entry is not a valid UUID: {s}"
                )));
            }
        }
    }
    Ok(())
}

/// Validate an array of keyword strings (min/max length enforced).
fn validate_keyword_array(
    obj: &serde_json::Value,
    field: &str,
    max_len: usize,
    ctx: &str,
) -> Result<(), PreferencesError> {
    if let Some(arr) = obj.get(field) {
        let arr = arr
            .as_array()
            .ok_or_else(|| PreferencesError::Validation(format!("{ctx} must be an array")))?;

        if arr.len() > max_len {
            return Err(PreferencesError::Validation(format!(
                "{ctx} too many entries ({}, max {max_len})",
                arr.len()
            )));
        }

        for entry in arr {
            let s = entry.as_str().ok_or_else(|| {
                PreferencesError::Validation(format!("{ctx} entries must be strings"))
            })?;
            let keyword_len = unicode_len(s);
            if keyword_len < MIN_KEYWORD_LEN {
                return Err(PreferencesError::Validation(format!(
                    "{ctx} entry too short ({keyword_len}, min {MIN_KEYWORD_LEN})"
                )));
            }
            if keyword_len > MAX_KEYWORD_LEN {
                return Err(PreferencesError::Validation(format!(
                    "{ctx} entry too long ({keyword_len}, max {MAX_KEYWORD_LEN})"
                )));
            }
        }
    }
    Ok(())
}
