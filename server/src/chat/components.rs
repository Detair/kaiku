//! Interactive message component types (Discord-parity) and validation.
//!
//! Components are bot-authored action rows containing buttons and select menus.
//! Like embeds they are semi-trusted and validated before storage; `custom_id`
//! is an opaque bot-defined routing key (validated for length only). The
//! interaction round-trip that fires when a user clicks a component is a
//! separate increment — this module is the data model + rendering contract.

use serde::{Deserialize, Serialize};

/// Max action rows per message.
pub const MAX_ROWS: usize = 5;
/// Max buttons in one action row.
pub const MAX_BUTTONS_PER_ROW: usize = 5;
/// Max options in a select menu.
pub const MAX_SELECT_OPTIONS: usize = 25;
/// Max length of a `custom_id`.
pub const MAX_CUSTOM_ID: usize = 100;

/// A row of interactive components.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct ActionRow {
    pub components: Vec<Component>,
}

/// A single interactive component: a button or a select menu.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Component {
    Button(Button),
    SelectMenu(SelectMenu),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ButtonStyle {
    Primary,
    Secondary,
    Success,
    Danger,
    Link,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct Button {
    pub style: ButtonStyle,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Bot routing key. Required for all styles except `Link`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_id: Option<String>,
    /// Destination for `Link` buttons (https only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default)]
    pub disabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct SelectMenu {
    pub custom_id: String,
    pub options: Vec<SelectOption>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,
    #[serde(default)]
    pub disabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct SelectOption {
    pub label: String,
    pub value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub default: bool,
}

/// Reason a component set was rejected.
#[derive(Debug, PartialEq, Eq)]
pub enum ComponentError {
    TooManyRows,
    TooManyButtons,
    TooManySelectOptions,
    CustomIdTooLong,
    MissingCustomId,
    LinkButtonNeedsUrl,
    NonHttpsLinkUrl,
    EmptySelect,
    LabelTooLong,
}

impl std::fmt::Display for ComponentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooManyRows => write!(f, "too many action rows (max {MAX_ROWS})"),
            Self::TooManyButtons => {
                write!(f, "too many buttons in a row (max {MAX_BUTTONS_PER_ROW})")
            }
            Self::TooManySelectOptions => {
                write!(f, "select menu has more than {MAX_SELECT_OPTIONS} options")
            }
            Self::CustomIdTooLong => write!(f, "custom_id exceeds {MAX_CUSTOM_ID} chars"),
            Self::MissingCustomId => write!(f, "non-link button requires a custom_id"),
            Self::LinkButtonNeedsUrl => write!(f, "link button requires a url"),
            Self::NonHttpsLinkUrl => write!(f, "link button url must be https://"),
            Self::EmptySelect => write!(f, "select menu must have at least one option"),
            Self::LabelTooLong => write!(f, "component label exceeds 80 chars"),
        }
    }
}

fn check_custom_id(id: &str) -> Result<(), ComponentError> {
    if id.chars().count() > MAX_CUSTOM_ID {
        return Err(ComponentError::CustomIdTooLong);
    }
    Ok(())
}

/// Validate a set of action rows against Discord-parity structural caps.
pub fn validate_components(rows: &[ActionRow]) -> Result<(), ComponentError> {
    if rows.len() > MAX_ROWS {
        return Err(ComponentError::TooManyRows);
    }
    for row in rows {
        let button_count = row
            .components
            .iter()
            .filter(|c| matches!(c, Component::Button(_)))
            .count();
        if button_count > MAX_BUTTONS_PER_ROW {
            return Err(ComponentError::TooManyButtons);
        }
        for comp in &row.components {
            match comp {
                Component::Button(b) => {
                    if let Some(l) = &b.label {
                        if l.chars().count() > 80 {
                            return Err(ComponentError::LabelTooLong);
                        }
                    }
                    if b.style == ButtonStyle::Link {
                        let url = b.url.as_deref().ok_or(ComponentError::LinkButtonNeedsUrl)?;
                        if !url.starts_with("https://") {
                            return Err(ComponentError::NonHttpsLinkUrl);
                        }
                    } else {
                        let id = b
                            .custom_id
                            .as_deref()
                            .ok_or(ComponentError::MissingCustomId)?;
                        check_custom_id(id)?;
                    }
                }
                Component::SelectMenu(s) => {
                    check_custom_id(&s.custom_id)?;
                    if s.options.is_empty() {
                        return Err(ComponentError::EmptySelect);
                    }
                    if s.options.len() > MAX_SELECT_OPTIONS {
                        return Err(ComponentError::TooManySelectOptions);
                    }
                    for o in &s.options {
                        if o.label.chars().count() > 80 {
                            return Err(ComponentError::LabelTooLong);
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

/// Whether a `custom_id` exists anywhere in the component set (used by the
/// interaction round-trip to validate a click against the stored components).
#[must_use]
pub fn custom_id_present(rows: &[ActionRow], custom_id: &str) -> bool {
    rows.iter().flat_map(|r| &r.components).any(|c| match c {
        Component::Button(b) => b.custom_id.as_deref() == Some(custom_id),
        Component::SelectMenu(s) => s.custom_id == custom_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn btn(style: ButtonStyle, custom_id: Option<&str>, url: Option<&str>) -> Component {
        Component::Button(Button {
            style,
            label: Some("Click".into()),
            custom_id: custom_id.map(str::to_string),
            url: url.map(str::to_string),
            disabled: false,
        })
    }

    #[test]
    fn accepts_a_button_row() {
        let rows = vec![ActionRow {
            components: vec![btn(ButtonStyle::Primary, Some("cid_a"), None)],
        }];
        assert!(validate_components(&rows).is_ok());
    }

    #[test]
    fn rejects_too_many_rows() {
        let rows = vec![ActionRow { components: vec![] }; MAX_ROWS + 1];
        assert_eq!(validate_components(&rows), Err(ComponentError::TooManyRows));
    }

    #[test]
    fn rejects_too_many_buttons_per_row() {
        let rows = vec![ActionRow {
            components: (0..6)
                .map(|i| btn(ButtonStyle::Primary, Some(&format!("c{i}")), None))
                .collect(),
        }];
        assert_eq!(
            validate_components(&rows),
            Err(ComponentError::TooManyButtons)
        );
    }

    #[test]
    fn non_link_button_requires_custom_id() {
        let rows = vec![ActionRow {
            components: vec![btn(ButtonStyle::Primary, None, None)],
        }];
        assert_eq!(
            validate_components(&rows),
            Err(ComponentError::MissingCustomId)
        );
    }

    #[test]
    fn link_button_requires_https_url() {
        let http = vec![ActionRow {
            components: vec![btn(ButtonStyle::Link, None, Some("http://x"))],
        }];
        assert_eq!(
            validate_components(&http),
            Err(ComponentError::NonHttpsLinkUrl)
        );
        let missing = vec![ActionRow {
            components: vec![btn(ButtonStyle::Link, None, None)],
        }];
        assert_eq!(
            validate_components(&missing),
            Err(ComponentError::LinkButtonNeedsUrl)
        );
        let ok = vec![ActionRow {
            components: vec![btn(ButtonStyle::Link, None, Some("https://x"))],
        }];
        assert!(validate_components(&ok).is_ok());
    }

    #[test]
    fn rejects_overlong_custom_id() {
        let long = "x".repeat(101);
        let rows = vec![ActionRow {
            components: vec![btn(ButtonStyle::Primary, Some(&long), None)],
        }];
        assert_eq!(
            validate_components(&rows),
            Err(ComponentError::CustomIdTooLong)
        );
    }

    #[test]
    fn select_menu_validation() {
        let empty = vec![ActionRow {
            components: vec![Component::SelectMenu(SelectMenu {
                custom_id: "sel".into(),
                options: vec![],
                placeholder: None,
                disabled: false,
            })],
        }];
        assert_eq!(
            validate_components(&empty),
            Err(ComponentError::EmptySelect)
        );

        let ok = vec![ActionRow {
            components: vec![Component::SelectMenu(SelectMenu {
                custom_id: "sel".into(),
                options: vec![SelectOption {
                    label: "One".into(),
                    value: "1".into(),
                    description: None,
                    default: false,
                }],
                placeholder: Some("pick".into()),
                disabled: false,
            })],
        }];
        assert!(validate_components(&ok).is_ok());
    }

    #[test]
    fn custom_id_present_finds_button_and_select() {
        let rows = vec![
            ActionRow {
                components: vec![btn(ButtonStyle::Primary, Some("btn_1"), None)],
            },
            ActionRow {
                components: vec![Component::SelectMenu(SelectMenu {
                    custom_id: "sel_1".into(),
                    options: vec![SelectOption {
                        label: "a".into(),
                        value: "a".into(),
                        description: None,
                        default: false,
                    }],
                    placeholder: None,
                    disabled: false,
                })],
            },
        ];
        assert!(custom_id_present(&rows, "btn_1"));
        assert!(custom_id_present(&rows, "sel_1"));
        assert!(!custom_id_present(&rows, "nope"));
    }
}
