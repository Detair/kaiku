//! Incoming webhook request/response DTOs.
//!
//! The wire format mirrors Discord's webhook object and Execute Webhook params
//! so existing integrations work by pasting a Kaiku URL where a Discord
//! webhook URL is expected. Request DTOs deliberately tolerate unknown fields
//! (no `deny_unknown_fields`) — tolerance is the compatibility contract.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::chat::embeds::{Embed, EmbedAuthor, EmbedField, EmbedFooter};
use crate::chat::types::AuthorProfile;

/// `incoming_webhooks` table row.
#[derive(Debug, Clone, FromRow)]
pub struct IncomingWebhook {
    pub id: Uuid,
    pub guild_id: Uuid,
    pub channel_id: Uuid,
    pub name: String,
    pub avatar_url: Option<String>,
    pub token: String,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Discord-compatible webhook object.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct WebhookResponse {
    pub id: Uuid,
    /// Always 1 (Incoming) — Discord webhook type discriminator.
    #[serde(rename = "type")]
    #[schema(rename = "type")]
    pub kind: u8,
    pub guild_id: Uuid,
    pub channel_id: Uuid,
    pub name: String,
    /// Discord field name for the default avatar; Kaiku returns a URL here.
    pub avatar: Option<String>,
    pub token: String,
    /// Always null (Kaiku incoming webhooks are not application-owned).
    pub application_id: Option<Uuid>,
    /// Fully-qualified execute URL for copy-paste into integrations.
    pub url: String,
    /// Creator profile. Omitted on token-authenticated reads (Discord parity).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<AuthorProfile>,
}

impl WebhookResponse {
    pub fn new(webhook: IncomingWebhook, base_url: &str, user: Option<AuthorProfile>) -> Self {
        let url = format!(
            "{}/api/webhooks/{}/{}",
            base_url.trim_end_matches('/'),
            webhook.id,
            webhook.token
        );
        Self {
            id: webhook.id,
            kind: 1,
            guild_id: webhook.guild_id,
            channel_id: webhook.channel_id,
            name: webhook.name,
            avatar: webhook.avatar_url,
            token: webhook.token,
            application_id: None,
            url,
            user,
        }
    }
}

/// `POST /api/channels/{id}/webhooks` body.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreateWebhookRequest {
    pub name: String,
    /// Avatar URL (Discord sends base64 image data here; Kaiku accepts an
    /// https URL — non-https values are ignored).
    #[serde(default)]
    pub avatar: Option<String>,
    /// Kaiku-native alias for `avatar`.
    #[serde(default)]
    pub avatar_url: Option<String>,
}

/// `PATCH /api/webhooks/{id}` / `PATCH /api/webhooks/{id}/{token}` body.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ModifyWebhookRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub avatar: Option<String>,
    #[serde(default)]
    pub avatar_url: Option<String>,
    /// Move the webhook to another channel (management route only; ignored on
    /// the token route like Discord).
    #[serde(default)]
    pub channel_id: Option<Uuid>,
}

/// Query params for Execute Webhook.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ExecuteQuery {
    /// When true, wait for the message and return it (200); otherwise 204.
    #[serde(default)]
    pub wait: bool,
    /// Post into an existing thread (thread root message id or forum post id).
    #[serde(default)]
    pub thread_id: Option<Uuid>,
}

/// Execute Webhook JSON body (Discord-compatible).
///
/// Fields Kaiku accepts but ignores for v1 parity: `tts`, `allowed_mentions`,
/// `components`, `flags`, `poll`, `applied_tags` — they simply aren't
/// declared, so serde skips them.
#[derive(Debug, Default, Deserialize, utoipa::ToSchema)]
pub struct ExecuteWebhookBody {
    #[serde(default)]
    pub content: Option<String>,
    /// Per-message display-name override.
    #[serde(default)]
    pub username: Option<String>,
    /// Per-message avatar override (https only; ignored otherwise).
    #[serde(default)]
    pub avatar_url: Option<String>,
    #[serde(default)]
    pub embeds: Option<Vec<DiscordEmbedIn>>,
    /// Create a new forum post with this title (forum channels only).
    #[serde(default)]
    pub thread_name: Option<String>,
}

/// `PATCH /api/webhooks/{id}/{token}/messages/{message_id}` body.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct EditWebhookMessageBody {
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub embeds: Option<Vec<DiscordEmbedIn>>,
}

// ============================================================================
// Discord embed wire adapter
// ============================================================================
//
// Kaiku's `chat::embeds::Embed` stores `image`/`thumbnail` as plain URL
// strings, while Discord senders post objects (`{"url": "..."}`). These
// adapter types mirror Discord's shape exactly and convert into the Kaiku
// embed, dropping non-https URLs instead of rejecting (game senders over
// strictness). `type`, `provider`, `video` and media dimensions are simply
// not declared, matching Discord's "server-set, ignored on input" rule.

#[derive(Debug, Default, Deserialize, utoipa::ToSchema)]
pub struct DiscordEmbedIn {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub color: Option<i64>,
    #[serde(default)]
    pub timestamp: Option<String>,
    #[serde(default)]
    pub footer: Option<DiscordEmbedFooterIn>,
    #[serde(default)]
    pub image: Option<DiscordEmbedMediaIn>,
    #[serde(default)]
    pub thumbnail: Option<DiscordEmbedMediaIn>,
    #[serde(default)]
    pub author: Option<DiscordEmbedAuthorIn>,
    #[serde(default)]
    pub fields: Option<Vec<DiscordEmbedFieldIn>>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct DiscordEmbedMediaIn {
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct DiscordEmbedFooterIn {
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub icon_url: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct DiscordEmbedAuthorIn {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub icon_url: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct DiscordEmbedFieldIn {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub inline: Option<bool>,
}

/// Keep only https URLs (webhook input is untrusted).
fn https_only(url: Option<String>) -> Option<String> {
    url.filter(|u| u.starts_with("https://"))
}

impl DiscordEmbedIn {
    /// Convert to a Kaiku embed. Returns `None` when the converted embed
    /// carries no renderable content at all.
    pub fn into_embed(self) -> Option<Embed> {
        let fields: Vec<EmbedField> = self
            .fields
            .unwrap_or_default()
            .into_iter()
            .filter_map(|f| match (f.name, f.value) {
                (Some(name), Some(value)) if !name.is_empty() && !value.is_empty() => {
                    Some(EmbedField {
                        name,
                        value,
                        inline: f.inline.unwrap_or(false),
                    })
                }
                _ => None,
            })
            .collect();

        let author = self.author.and_then(|a| {
            a.name.filter(|n| !n.is_empty()).map(|name| EmbedAuthor {
                name,
                url: https_only(a.url),
                icon_url: https_only(a.icon_url),
            })
        });

        let footer = self.footer.and_then(|f| {
            f.text.filter(|t| !t.is_empty()).map(|text| EmbedFooter {
                text,
                icon_url: https_only(f.icon_url),
            })
        });

        let embed = Embed {
            title: self.title.filter(|t| !t.is_empty()),
            description: self.description.filter(|d| !d.is_empty()),
            url: https_only(self.url),
            color: self
                .color
                .map(|c| u32::try_from(c.clamp(0, 0x00FF_FFFF)).unwrap_or(0)),
            author,
            fields,
            image: https_only(self.image.and_then(|m| m.url)),
            thumbnail: https_only(self.thumbnail.and_then(|m| m.url)),
            footer,
            timestamp: self.timestamp.filter(|t| !t.is_empty()),
        };

        let has_content = embed.title.is_some()
            || embed.description.is_some()
            || embed.author.is_some()
            || embed.footer.is_some()
            || embed.image.is_some()
            || embed.thumbnail.is_some()
            || !embed.fields.is_empty();
        has_content.then_some(embed)
    }
}

/// Convert Discord wire embeds into validated Kaiku embeds.
///
/// Fully-empty embeds are skipped; validation errors (size caps) bubble up.
pub fn adapt_embeds(
    wire: Vec<DiscordEmbedIn>,
) -> Result<Vec<Embed>, crate::chat::embeds::EmbedError> {
    if wire.len() > crate::chat::embeds::MAX_EMBEDS {
        return Err(crate::chat::embeds::EmbedError::TooMany);
    }
    let mut embeds: Vec<Embed> = wire
        .into_iter()
        .filter_map(DiscordEmbedIn::into_embed)
        .collect();
    crate::chat::embeds::validate_embeds(&mut embeds)?;
    Ok(embeds)
}
