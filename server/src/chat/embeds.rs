//! Rich embed types (Discord-parity shape) and server-side validation.
//!
//! Embeds are semi-trusted (bot/webhook-authored). Every embed is validated
//! against Discord-parity size caps and URL rules before storage; text is
//! stored verbatim and sanitized at render time on the client.

use serde::{Deserialize, Serialize};

/// Max embeds per message.
pub const MAX_EMBEDS: usize = 10;
/// Combined character budget across one embed's text fields.
pub const MAX_EMBED_TOTAL_CHARS: usize = 6000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, utoipa::ToSchema)]
pub struct Embed {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// 24-bit RGB color.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<EmbedAuthor>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<EmbedField>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumbnail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub footer: Option<EmbedFooter>,
    /// RFC3339 timestamp string (rendered as-is).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, utoipa::ToSchema)]
pub struct EmbedField {
    pub name: String,
    pub value: String,
    #[serde(default)]
    pub inline: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, utoipa::ToSchema)]
pub struct EmbedAuthor {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, utoipa::ToSchema)]
pub struct EmbedFooter {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon_url: Option<String>,
}

/// Reason an embed set was rejected.
#[derive(Debug, PartialEq, Eq)]
pub enum EmbedError {
    TooMany,
    TitleTooLong,
    DescriptionTooLong,
    TooManyFields,
    FieldNameTooLong,
    FieldValueTooLong,
    FooterTooLong,
    AuthorNameTooLong,
    TotalTooLong,
    NonHttpsUrl(&'static str),
}

impl std::fmt::Display for EmbedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooMany => write!(f, "too many embeds (max {MAX_EMBEDS})"),
            Self::TitleTooLong => write!(f, "embed title exceeds 256 chars"),
            Self::DescriptionTooLong => write!(f, "embed description exceeds 4096 chars"),
            Self::TooManyFields => write!(f, "embed has more than 25 fields"),
            Self::FieldNameTooLong => write!(f, "embed field name exceeds 256 chars"),
            Self::FieldValueTooLong => write!(f, "embed field value exceeds 1024 chars"),
            Self::FooterTooLong => write!(f, "embed footer exceeds 2048 chars"),
            Self::AuthorNameTooLong => write!(f, "embed author name exceeds 256 chars"),
            Self::TotalTooLong => {
                write!(f, "embed total text exceeds {MAX_EMBED_TOTAL_CHARS} chars")
            }
            Self::NonHttpsUrl(field) => write!(f, "embed {field} must be an https:// URL"),
        }
    }
}

fn check_https(u: &Option<String>, field: &'static str) -> Result<(), EmbedError> {
    if let Some(u) = u {
        if !u.is_empty() && !u.starts_with("https://") {
            return Err(EmbedError::NonHttpsUrl(field));
        }
    }
    Ok(())
}

/// Validate + normalize a set of embeds. Clamps colors to 24-bit; rejects on any
/// size cap or non-https URL.
pub fn validate_embeds(embeds: &mut [Embed]) -> Result<(), EmbedError> {
    if embeds.len() > MAX_EMBEDS {
        return Err(EmbedError::TooMany);
    }
    for e in embeds.iter_mut() {
        let mut total = 0usize;
        if let Some(t) = &e.title {
            if t.chars().count() > 256 {
                return Err(EmbedError::TitleTooLong);
            }
            total += t.chars().count();
        }
        if let Some(d) = &e.description {
            if d.chars().count() > 4096 {
                return Err(EmbedError::DescriptionTooLong);
            }
            total += d.chars().count();
        }
        if e.fields.len() > 25 {
            return Err(EmbedError::TooManyFields);
        }
        for f in &e.fields {
            if f.name.chars().count() > 256 {
                return Err(EmbedError::FieldNameTooLong);
            }
            if f.value.chars().count() > 1024 {
                return Err(EmbedError::FieldValueTooLong);
            }
            total += f.name.chars().count() + f.value.chars().count();
        }
        if let Some(fo) = &e.footer {
            if fo.text.chars().count() > 2048 {
                return Err(EmbedError::FooterTooLong);
            }
            total += fo.text.chars().count();
        }
        if let Some(a) = &e.author {
            if a.name.chars().count() > 256 {
                return Err(EmbedError::AuthorNameTooLong);
            }
            total += a.name.chars().count();
        }
        if total > MAX_EMBED_TOTAL_CHARS {
            return Err(EmbedError::TotalTooLong);
        }
        check_https(&e.url, "url")?;
        check_https(&e.image, "image")?;
        check_https(&e.thumbnail, "thumbnail")?;
        if let Some(a) = &e.author {
            check_https(&a.url, "author.url")?;
            check_https(&a.icon_url, "author.icon_url")?;
        }
        if let Some(fo) = &e.footer {
            check_https(&fo.icon_url, "footer.icon_url")?;
        }
        if let Some(c) = e.color {
            e.color = Some(c & 0x00FF_FFFF);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Embed {
        Embed {
            title: None,
            description: None,
            url: None,
            color: None,
            author: None,
            fields: vec![],
            image: None,
            thumbnail: None,
            footer: None,
            timestamp: None,
        }
    }

    #[test]
    fn accepts_a_simple_embed() {
        let mut e = vec![Embed {
            title: Some("Hi".into()),
            description: Some("desc".into()),
            ..base()
        }];
        assert!(validate_embeds(&mut e).is_ok());
    }

    #[test]
    fn rejects_too_many_embeds() {
        let mut e = vec![base(); MAX_EMBEDS + 1];
        assert_eq!(validate_embeds(&mut e), Err(EmbedError::TooMany));
    }

    #[test]
    fn rejects_overlong_title() {
        let mut e = vec![Embed {
            title: Some("x".repeat(257)),
            ..base()
        }];
        assert_eq!(validate_embeds(&mut e), Err(EmbedError::TitleTooLong));
    }

    #[test]
    fn rejects_non_https_url() {
        let mut e = vec![Embed {
            url: Some("http://evil".into()),
            ..base()
        }];
        assert_eq!(validate_embeds(&mut e), Err(EmbedError::NonHttpsUrl("url")));
        let mut e2 = vec![Embed {
            image: Some("javascript:alert(1)".into()),
            ..base()
        }];
        assert_eq!(
            validate_embeds(&mut e2),
            Err(EmbedError::NonHttpsUrl("image"))
        );
    }

    #[test]
    fn clamps_color_to_24bit() {
        let mut e = vec![Embed {
            color: Some(0xFF12_3456),
            ..base()
        }];
        validate_embeds(&mut e).unwrap();
        assert_eq!(e[0].color, Some(0x0012_3456));
    }

    #[test]
    fn rejects_total_over_budget() {
        let mut e = vec![Embed {
            description: Some("a".repeat(4096)),
            fields: vec![
                EmbedField {
                    name: "n".repeat(256),
                    value: "v".repeat(1024),
                    inline: false,
                },
                EmbedField {
                    name: "n".repeat(256),
                    value: "v".repeat(1024),
                    inline: false,
                },
            ],
            ..base()
        }];
        assert_eq!(validate_embeds(&mut e), Err(EmbedError::TotalTooLong));
    }
}
