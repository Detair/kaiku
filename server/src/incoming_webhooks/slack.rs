//! Slack-compatible webhook execution (`POST /api/webhooks/{id}/{token}/slack`).
//!
//! Many game/ops tools only speak Slack's legacy incoming-webhook format.
//! Discord ships this compat route, so Kaiku mirrors it: the Slack payload is
//! mapped onto the regular execute pipeline (`text` → content, `attachments`
//! → embeds). Like Slack/Discord, the response body is a plain `ok`.

use std::sync::LazyLock;

use axum::extract::{Path, Query, State};
use axum::http::header::CONTENT_TYPE;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use axum::Extension;
use serde::Deserialize;
use uuid::Uuid;

use super::error::IncomingWebhookError;
use super::execute::{send_webhook_message, verify_or_record_failure};
use super::types::{
    DiscordEmbedAuthorIn, DiscordEmbedFieldIn, DiscordEmbedFooterIn, DiscordEmbedIn,
    DiscordEmbedMediaIn, ExecuteQuery, ExecuteWebhookBody,
};
use crate::api::AppState;
use crate::ratelimit::NormalizedIp;

/// Slack legacy incoming-webhook payload (unknown fields ignored).
#[derive(Debug, Default, Deserialize, utoipa::ToSchema)]
pub struct SlackPayload {
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub icon_url: Option<String>,
    #[serde(default)]
    pub attachments: Option<Vec<SlackAttachment>>,
}

#[derive(Debug, Default, Deserialize, utoipa::ToSchema)]
pub struct SlackAttachment {
    #[serde(default)]
    pub fallback: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub pretext: Option<String>,
    #[serde(default)]
    pub author_name: Option<String>,
    #[serde(default)]
    pub author_link: Option<String>,
    #[serde(default)]
    pub author_icon: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub title_link: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub fields: Option<Vec<SlackField>>,
    #[serde(default)]
    pub image_url: Option<String>,
    #[serde(default)]
    pub thumb_url: Option<String>,
    #[serde(default)]
    pub footer: Option<String>,
    #[serde(default)]
    pub footer_icon: Option<String>,
    /// Unix timestamp (Slack sends numbers or numeric strings).
    #[serde(default)]
    pub ts: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct SlackField {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub short: Option<bool>,
}

/// Slack link markup `<url|label>` / `<url>` → markdown.
static SLACK_LINK_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"<(https?://[^|>]+)\|([^>]+)>|<(https?://[^>]+)>").unwrap()
});

fn convert_slack_text(text: &str) -> String {
    SLACK_LINK_RE
        .replace_all(text, |caps: &regex::Captures<'_>| {
            if let (Some(url), Some(label)) = (caps.get(1), caps.get(2)) {
                format!("[{}]({})", label.as_str(), url.as_str())
            } else if let Some(url) = caps.get(3) {
                url.as_str().to_string()
            } else {
                caps[0].to_string()
            }
        })
        .to_string()
}

/// Slack color → 24-bit RGB. Accepts `#RRGGBB` hex and Slack's named colors.
fn convert_color(color: &str) -> Option<i64> {
    match color {
        "good" => Some(0x2E_B8_86),
        "warning" => Some(0xDA_A0_38),
        "danger" => Some(0xA3_00_02),
        hex => i64::from_str_radix(hex.trim_start_matches('#'), 16).ok(),
    }
}

/// Slack `ts` (unix seconds, number or string) → RFC3339.
fn convert_ts(ts: &serde_json::Value) -> Option<String> {
    let secs = match ts {
        serde_json::Value::Number(n) => n.as_f64(),
        serde_json::Value::String(s) => s.parse::<f64>().ok(),
        _ => None,
    }?;
    #[allow(clippy::cast_possible_truncation)]
    chrono::DateTime::from_timestamp(secs as i64, 0).map(|t| t.to_rfc3339())
}

fn attachment_to_embed(a: SlackAttachment) -> DiscordEmbedIn {
    let description = match (a.pretext, a.text) {
        (Some(p), Some(t)) => Some(format!(
            "{}\n\n{}",
            convert_slack_text(&p),
            convert_slack_text(&t)
        )),
        (Some(p), None) => Some(convert_slack_text(&p)),
        (None, Some(t)) => Some(convert_slack_text(&t)),
        // fallback only carries content when nothing else does
        (None, None) => a.fallback.as_deref().map(convert_slack_text),
    };
    DiscordEmbedIn {
        title: a.title,
        description,
        url: a.title_link,
        color: a.color.as_deref().and_then(convert_color),
        timestamp: a.ts.as_ref().and_then(convert_ts),
        footer: a.footer.map(|text| DiscordEmbedFooterIn {
            text: Some(text),
            icon_url: a.footer_icon,
        }),
        image: a
            .image_url
            .map(|url| DiscordEmbedMediaIn { url: Some(url) }),
        thumbnail: a
            .thumb_url
            .map(|url| DiscordEmbedMediaIn { url: Some(url) }),
        author: a.author_name.map(|name| DiscordEmbedAuthorIn {
            name: Some(name),
            url: a.author_link,
            icon_url: a.author_icon,
        }),
        fields: a.fields.map(|fields| {
            fields
                .into_iter()
                .map(|f| DiscordEmbedFieldIn {
                    name: f.title,
                    value: f.value.as_deref().map(convert_slack_text),
                    inline: f.short,
                })
                .collect()
        }),
    }
}

/// Map a Slack payload onto the Discord-shaped execute body.
pub fn slack_to_execute(payload: SlackPayload) -> ExecuteWebhookBody {
    ExecuteWebhookBody {
        content: payload.text.as_deref().map(convert_slack_text),
        username: payload.username,
        avatar_url: payload.icon_url,
        embeds: payload
            .attachments
            .map(|list| list.into_iter().map(attachment_to_embed).collect()),
        thread_name: None,
    }
}

/// `POST /api/webhooks/{webhook_id}/{token}/slack` — Slack-compatible execute.
///
/// Accepts `application/json` bodies and Slack's classic
/// `application/x-www-form-urlencoded` `payload=<json>` form.
#[utoipa::path(
    post,
    path = "/api/webhooks/{webhook_id}/{token}/slack",
    tag = "incoming-webhooks",
    params(
        ("webhook_id" = Uuid, Path, description = "Webhook ID"),
        ("token" = String, Path, description = "Webhook token"),
        ("thread_id" = Option<Uuid>, Query, description = "Post into an existing thread"),
    ),
    request_body = SlackPayload,
    responses((status = 200, description = "ok")),
)]
#[tracing::instrument(skip(state, token, ip, headers, raw_body))]
pub async fn execute_slack_webhook(
    State(state): State<AppState>,
    Path((webhook_id, token)): Path<(Uuid, String)>,
    Query(query): Query<ExecuteQuery>,
    ip: Option<Extension<NormalizedIp>>,
    headers: HeaderMap,
    raw_body: String,
) -> Result<Response, IncomingWebhookError> {
    let webhook = verify_or_record_failure(&state, webhook_id, &token, ip.as_deref()).await?;

    let is_form = headers
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|ct| ct.starts_with("application/x-www-form-urlencoded"));
    let json = if is_form {
        let form: Vec<(String, String)> = serde_urlencoded::from_str(&raw_body)
            .map_err(|_| IncomingWebhookError::Validation("Invalid form body".to_string()))?;
        form.into_iter()
            .find(|(k, _)| k == "payload")
            .map(|(_, v)| v)
            .ok_or_else(|| {
                IncomingWebhookError::Validation("Missing payload form field".to_string())
            })?
    } else {
        raw_body
    };
    let payload: SlackPayload = serde_json::from_str(&json)
        .map_err(|_| IncomingWebhookError::Validation("Invalid Slack payload".to_string()))?;

    send_webhook_message(&state, &webhook, slack_to_execute(payload), query.thread_id).await?;
    // Slack-compat responses are a literal "ok" (Discord does the same).
    Ok("ok".into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_slack_links() {
        assert_eq!(
            convert_slack_text("see <https://example.com|the docs> or <https://a.b>"),
            "see [the docs](https://example.com) or https://a.b"
        );
    }

    #[test]
    fn converts_colors() {
        assert_eq!(convert_color("#36a64f"), Some(0x0036_A64F));
        assert_eq!(convert_color("good"), Some(0x002E_B886));
        assert_eq!(convert_color("not-a-color"), None);
    }

    #[test]
    fn converts_ts_number_and_string() {
        assert_eq!(
            convert_ts(&serde_json::json!(0)),
            Some("1970-01-01T00:00:00+00:00".to_string())
        );
        assert!(convert_ts(&serde_json::json!("1700000000")).is_some());
        assert_eq!(convert_ts(&serde_json::json!(["x"])), None);
    }

    #[test]
    fn maps_attachment_to_embed() {
        let payload = SlackPayload {
            text: Some("deploy <https://ci.example.com|done>".to_string()),
            username: Some("CI".to_string()),
            icon_url: Some("https://example.com/icon.png".to_string()),
            attachments: Some(vec![SlackAttachment {
                color: Some("#36a64f".to_string()),
                title: Some("Build 42".to_string()),
                title_link: Some("https://ci.example.com/42".to_string()),
                text: Some("All tests green".to_string()),
                fields: Some(vec![SlackField {
                    title: Some("Branch".to_string()),
                    value: Some("main".to_string()),
                    short: Some(true),
                }]),
                ..Default::default()
            }]),
        };
        let body = slack_to_execute(payload);
        assert_eq!(
            body.content.as_deref(),
            Some("deploy [done](https://ci.example.com)")
        );
        assert_eq!(body.username.as_deref(), Some("CI"));
        let embeds = body.embeds.unwrap();
        assert_eq!(embeds.len(), 1);
        let embed = embeds[0].title.as_deref();
        assert_eq!(embed, Some("Build 42"));
        assert_eq!(embeds[0].color, Some(0x0036_A64F));
    }

    #[test]
    fn fallback_used_only_when_empty() {
        let a = SlackAttachment {
            fallback: Some("fallback text".to_string()),
            ..Default::default()
        };
        let e = attachment_to_embed(a);
        assert_eq!(e.description.as_deref(), Some("fallback text"));

        let a2 = SlackAttachment {
            fallback: Some("fallback text".to_string()),
            text: Some("real text".to_string()),
            ..Default::default()
        };
        let e2 = attachment_to_embed(a2);
        assert_eq!(e2.description.as_deref(), Some("real text"));
    }
}
