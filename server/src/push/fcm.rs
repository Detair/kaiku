//! FCM HTTP v1 transport.
//!
//! Sends the content-free [`WakeSignal`] as a data-only FCM message (contract
//! C20). Authentication is a Google service account: an RS256-signed JWT is
//! exchanged for a short-lived `OAuth2` access token, which is cached and shared
//! by all sends (single flight).
//!
//! Both outbound hosts are fixed (`oauth2.googleapis.com` for the token and
//! `fcm.googleapis.com` for sends; [`load_service_account`] refuses any other
//! `token_uri`), so the user-endpoint SSRF guard does not apply here.
//!
//! Secrets rule: the private key, `OAuth2` access tokens and FCM registration
//! tokens never appear in logs, errors or `Debug` output. Logs carry only the
//! HTTP status and the FCM `errorCode`.

use std::time::{Duration, Instant};

use anyhow::{bail, Context};
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{Delivery, WakeSignal};

/// The only `OAuth2` token endpoint a service account may name.
pub const GOOGLE_TOKEN_URI: &str = "https://oauth2.googleapis.com/token";

/// Production FCM API base URL.
pub const FCM_API_BASE: &str = "https://fcm.googleapis.com";

const FCM_SCOPE: &str = "https://www.googleapis.com/auth/firebase.messaging";
const JWT_BEARER_GRANT: &str = "urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Ajwt-bearer";
const ASSERTION_LIFETIME_SECS: i64 = 3600;
/// Refresh the cached access token when less than this much validity remains.
const REFRESH_MARGIN: Duration = Duration::from_mins(5);
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);
/// After a failed token fetch, sends fail fast for this long instead of each
/// waiting on the token endpoint again (an outage would otherwise serialize
/// N x `HTTP_TIMEOUT` behind the single-flight lock).
const TOKEN_FAILURE_COOLDOWN: Duration = Duration::from_secs(30);

// ============================================================================
// Service account
// ============================================================================

/// The fields of a Google service-account JSON key that FCM needs.
#[derive(Clone, Deserialize)]
pub struct ServiceAccountKey {
    pub project_id: String,
    pub client_email: String,
    pub private_key: String,
    pub private_key_id: String,
    pub token_uri: String,
}

impl std::fmt::Debug for ServiceAccountKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServiceAccountKey")
            .field("project_id", &self.project_id)
            .field("client_email", &self.client_email)
            .finish_non_exhaustive()
    }
}

/// Load the FCM service account from `FCM_SERVICE_ACCOUNT_FILE` (a path) or
/// `FCM_SERVICE_ACCOUNT_JSON` (inline JSON). Callers pass `None` for unset or
/// empty values. Neither set → `Ok(None)` (FCM off).
///
/// # Errors
/// Both sources set, unreadable file, invalid JSON, a `token_uri` other than
/// Google's, or a `private_key` that is not an RSA private key PEM. Error
/// messages never include the file contents.
pub fn load_service_account(
    file: Option<String>,
    json: Option<String>,
) -> anyhow::Result<Option<ServiceAccountKey>> {
    let raw = match (file, json) {
        (None, None) => return Ok(None),
        (Some(_), Some(_)) => bail!(
            "FCM_SERVICE_ACCOUNT_FILE and FCM_SERVICE_ACCOUNT_JSON are both set; set only one"
        ),
        (Some(path), None) => std::fs::read_to_string(&path).map_err(|e| {
            anyhow::anyhow!(
                "FCM_SERVICE_ACCOUNT_FILE {path} is not readable: {}",
                e.kind()
            )
        })?,
        (None, Some(json)) => json,
    };
    // serde_json's error text describes the position, never the input itself;
    // drop it anyway so no fragment of the key can reach a log.
    let key: ServiceAccountKey = serde_json::from_str(&raw).map_err(|_| {
        anyhow::anyhow!("FCM service account is not a valid service-account JSON key")
    })?;
    if !is_valid_project_id(&key.project_id) {
        bail!("FCM service account project_id is not a valid Google Cloud project id");
    }
    if key.token_uri != GOOGLE_TOKEN_URI {
        bail!("FCM service account token_uri must be {GOOGLE_TOKEN_URI}");
    }
    encoding_key(&key.private_key)?;
    Ok(Some(key))
}

/// Google Cloud project id format: `^[a-z][a-z0-9-]{4,29}$`.
fn is_valid_project_id(id: &str) -> bool {
    let b = id.as_bytes();
    (5..=30).contains(&b.len())
        && b[0].is_ascii_lowercase()
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

/// Parse the PEM and prove it can sign RS256 (rejects public keys and
/// non-RSA keys, which `from_rsa_pem` alone would accept or defer).
fn encoding_key(pem: &str) -> anyhow::Result<EncodingKey> {
    let invalid =
        || anyhow::anyhow!("FCM service account private_key is not an RSA private key PEM");
    let key = EncodingKey::from_rsa_pem(pem.as_bytes()).map_err(|_| invalid())?;
    jsonwebtoken::encode(
        &Header::new(Algorithm::RS256),
        &json!({"probe": true}),
        &key,
    )
    .map_err(|_| invalid())?;
    Ok(key)
}

// ============================================================================
// Wire format (C20)
// ============================================================================

/// Build the FCM v1 `messages:send` body: data-only, all data values strings.
pub(crate) fn fcm_message(token: &str, signal: &WakeSignal) -> serde_json::Value {
    let channel_id = signal.channel_id.to_string();
    json!({
        "message": {
            "token": token,
            "data": {
                "t": signal.t,
                "channel_id": channel_id,
                "count": signal.count.to_string(),
            },
            "android": {
                "priority": if signal.is_high_priority() { "HIGH" } else { "NORMAL" },
                "collapse_key": channel_id,
                "ttl": "86400s",
            },
        }
    })
}

// ============================================================================
// Response classification
// ============================================================================

/// What an FCM `messages:send` response means for the subscription.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FcmOutcome {
    Sent,
    /// The registration token is dead: prune the subscription.
    Gone,
    /// Our access token was rejected: refresh it and retry once.
    Unauthorized,
    /// Quota / server trouble: keep the row, try again next time.
    Retryable,
    /// Anything else (including 400, which usually means a bug in our payload):
    /// keep the row. Pruning here could silently disable push for everyone.
    Failed,
}

/// `error.details[].errorCode` of the detail whose `@type` ends with `FcmError`.
fn fcm_error_code(body: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    v["error"]["details"]
        .as_array()?
        .iter()
        .find(|d| d["@type"].as_str().is_some_and(|t| t.ends_with("FcmError")))
        .and_then(|d| d["errorCode"].as_str())
        .map(str::to_owned)
}

pub(crate) fn classify_fcm_response(status: u16, body: &str) -> FcmOutcome {
    if (200..300).contains(&status) {
        return FcmOutcome::Sent;
    }
    let code = fcm_error_code(body);
    match (status, code.as_deref()) {
        // Prune only on an explicit FCM verdict; a bare 404 (wrong URL or
        // project) or a 400 must never delete every device's subscription.
        (404, Some("UNREGISTERED")) | (403, Some("SENDER_ID_MISMATCH")) => FcmOutcome::Gone,
        (401, _) => FcmOutcome::Unauthorized,
        (429 | 500 | 503, _) => FcmOutcome::Retryable,
        _ => FcmOutcome::Failed,
    }
}

// ============================================================================
// Client
// ============================================================================

#[derive(Default)]
struct TokenState {
    cached: Option<CachedToken>,
    /// After a failed fetch: no new fetch before this instant (negative cache).
    retry_after: Option<Instant>,
}

struct CachedToken {
    access_token: String,
    expires_at: Instant,
}

#[derive(Serialize)]
struct AssertionClaims<'a> {
    iss: &'a str,
    scope: &'a str,
    aud: &'a str,
    iat: i64,
    exp: i64,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: u64,
}

/// FCM HTTP v1 client for one service account.
pub struct FcmClient {
    key: ServiceAccountKey,
    signing_key: EncodingKey,
    send_url: String,
    http: reqwest::Client,
    /// Held across the token fetch so concurrent sends share one refresh.
    token: tokio::sync::Mutex<TokenState>,
}

impl std::fmt::Debug for FcmClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FcmClient")
            .field("project_id", &self.key.project_id)
            .finish_non_exhaustive()
    }
}

impl FcmClient {
    /// Build a client. `api_base` is [`FCM_API_BASE`] in production.
    ///
    /// # Errors
    /// The private key is not a usable RSA key, or the HTTP client cannot be
    /// built.
    pub fn new(key: ServiceAccountKey, api_base: &str) -> anyhow::Result<Self> {
        let signing_key = encoding_key(&key.private_key)?;
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(HTTP_TIMEOUT)
            .build()
            .context("failed to build FCM HTTP client")?;
        let send_url = format!(
            "{}/v1/projects/{}/messages:send",
            api_base.trim_end_matches('/'),
            key.project_id
        );
        Ok(Self {
            key,
            signing_key,
            send_url,
            http,
            token: tokio::sync::Mutex::new(TokenState::default()),
        })
    }

    #[must_use]
    pub fn project_id(&self) -> &str {
        &self.key.project_id
    }

    /// Send one wake signal to one registration token. On a 401 the access
    /// token is refreshed and the send retried once.
    pub async fn send(&self, token: &str, signal: &WakeSignal) -> Delivery {
        let body = fcm_message(token, signal);
        for attempt in 0..2 {
            let access = match self.access_token().await {
                Ok(t) => t,
                Err(e) => {
                    tracing::warn!(error = %e, "push: FCM access token unavailable");
                    return Delivery::Failed;
                }
            };
            let Ok(resp) = self
                .http
                .post(&self.send_url)
                .bearer_auth(&access)
                .json(&body)
                .send()
                .await
            else {
                // Keep the log shape uniform: status/errorCode only.
                tracing::warn!("push: FCM send failed (transport error)");
                return Delivery::Failed;
            };
            let status = resp.status().as_u16();
            let text = resp.text().await.unwrap_or_default();
            match classify_fcm_response(status, &text) {
                FcmOutcome::Sent => return Delivery::Sent,
                FcmOutcome::Gone => {
                    tracing::info!(
                        status,
                        error_code = fcm_error_code(&text).as_deref().unwrap_or("-"),
                        "push: FCM token is no longer valid; dropping subscription"
                    );
                    return Delivery::Gone;
                }
                FcmOutcome::Unauthorized if attempt == 0 => {
                    self.invalidate(&access).await;
                }
                outcome => {
                    tracing::warn!(
                        status,
                        error_code = fcm_error_code(&text).as_deref().unwrap_or("-"),
                        ?outcome,
                        "push: FCM send failed"
                    );
                    return Delivery::Failed;
                }
            }
        }
        Delivery::Failed
    }

    /// A valid access token, fetching a new one when the cache is empty or
    /// close to expiry. The lock is held across the fetch (single flight).
    async fn access_token(&self) -> anyhow::Result<String> {
        let mut guard = self.token.lock().await;
        let now = Instant::now();
        if let Some(t) = guard.cached.as_ref() {
            if t.expires_at.saturating_duration_since(now) > REFRESH_MARGIN {
                return Ok(t.access_token.clone());
            }
        }
        // A token inside the refresh margin is still valid: if the refresh
        // fails (or is cooling down), keep using it rather than blacking out
        // sends over a transient OAuth2 hiccup.
        let still_valid = guard
            .cached
            .as_ref()
            .filter(|t| t.expires_at > now)
            .map(|t| t.access_token.clone());
        if guard.retry_after.is_some_and(|at| now < at) {
            return still_valid.ok_or_else(|| {
                anyhow::anyhow!("OAuth2 token fetch failed recently; cooling down")
            });
        }
        match self.fetch_token().await {
            Ok(fresh) => {
                let access = fresh.access_token.clone();
                guard.cached = Some(fresh);
                guard.retry_after = None;
                Ok(access)
            }
            Err(e) => {
                guard.retry_after = Some(Instant::now() + TOKEN_FAILURE_COOLDOWN);
                still_valid.ok_or(e)
            }
        }
    }

    /// Drop the cached token if it is still the one that was rejected.
    async fn invalidate(&self, rejected: &str) {
        let mut guard = self.token.lock().await;
        if guard
            .cached
            .as_ref()
            .is_some_and(|t| t.access_token == rejected)
        {
            guard.cached = None;
        }
    }

    async fn fetch_token(&self) -> anyhow::Result<CachedToken> {
        let iat = chrono::Utc::now().timestamp();
        let claims = AssertionClaims {
            iss: &self.key.client_email,
            scope: FCM_SCOPE,
            aud: &self.key.token_uri,
            iat,
            exp: iat + ASSERTION_LIFETIME_SECS,
        };
        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some(self.key.private_key_id.clone());
        let assertion = jsonwebtoken::encode(&header, &claims, &self.signing_key)
            .map_err(|_| anyhow::anyhow!("failed to sign the OAuth2 assertion"))?;
        // The assertion is base64url segments joined by '.', so it is already
        // form-safe.
        let form = format!("grant_type={JWT_BEARER_GRANT}&assertion={assertion}");
        let requested_at = Instant::now();
        let resp = self
            .http
            .post(&self.key.token_uri)
            .header("content-type", "application/x-www-form-urlencoded")
            .body(form)
            .send()
            .await
            .map_err(|_| anyhow::anyhow!("OAuth2 token request failed (transport error)"))?;
        let status = resp.status();
        if !status.is_success() {
            bail!("OAuth2 token endpoint answered HTTP {}", status.as_u16());
        }
        let token: TokenResponse = resp
            .json()
            .await
            .map_err(|_| anyhow::anyhow!("OAuth2 token response is not valid JSON"))?;
        Ok(CachedToken {
            access_token: token.access_token,
            expires_at: requested_at + Duration::from_secs(token.expires_in),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use axum::extract::State;
    use axum::http::{HeaderMap, StatusCode};
    use axum::routing::post;
    use axum::Router;
    use serde_json::json;
    use uuid::Uuid;

    use super::*;
    use crate::push::{Delivery, WakeSignal};

    include!("../../tests/fixtures/fcm_test_key.rs");

    fn key(token_uri: String) -> ServiceAccountKey {
        ServiceAccountKey {
            project_id: "kaiku-test".into(),
            client_email: "push@kaiku-test.iam.gserviceaccount.com".into(),
            private_key: TEST_FCM_RSA_PRIVATE_KEY_PEM.into(),
            private_key_id: "test-key-id".into(),
            token_uri,
        }
    }

    fn sa_json(token_uri: &str) -> String {
        json!({
            "type": "service_account",
            "project_id": "kaiku-test",
            "private_key_id": "test-key-id",
            "private_key": TEST_FCM_RSA_PRIVATE_KEY_PEM,
            "client_email": "push@kaiku-test.iam.gserviceaccount.com",
            "token_uri": token_uri,
        })
        .to_string()
    }

    fn fcm_error(status: &str, code: &str) -> String {
        json!({"error": {"code": 0, "message": "x", "status": status, "details": [
            {"@type": "type.googleapis.com/google.firebase.fcm.v1.FcmError", "errorCode": code}
        ]}})
        .to_string()
    }

    // ---- local FCM mock (127.0.0.1 only) ----------------------------------

    #[derive(Clone, Default)]
    struct Mock {
        token_hits: Arc<AtomicUsize>,
        sends: Arc<AtomicUsize>,
        /// Queued send responses; empty queue answers 200.
        queue: Arc<Mutex<VecDeque<(u16, String)>>>,
        /// Bearer tokens seen on sends.
        bearers: Arc<Mutex<Vec<String>>>,
        /// Delay applied to `/token` (to make concurrent callers overlap).
        token_delay_ms: u64,
        /// When set, `/token` answers this status instead of a token.
        token_status: Option<u16>,
    }

    async fn token_handler(State(m): State<Mock>, body: String) -> (StatusCode, String) {
        let n = m.token_hits.fetch_add(1, Ordering::SeqCst) + 1;
        tokio::time::sleep(Duration::from_millis(m.token_delay_ms)).await;
        if let Some(status) = m.token_status {
            return (StatusCode::from_u16(status).unwrap(), String::new());
        }
        // Verify the RS256 assertion the client posted.
        let form: std::collections::HashMap<String, String> = body
            .split('&')
            .filter_map(|kv| kv.split_once('='))
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        assert_eq!(
            form.get("grant_type").map(String::as_str),
            Some("urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Ajwt-bearer")
        );
        let assertion = form.get("assertion").expect("assertion");
        let header = jsonwebtoken::decode_header(assertion).unwrap();
        assert_eq!(header.alg, jsonwebtoken::Algorithm::RS256);
        assert_eq!(header.kid.as_deref(), Some("test-key-id"));
        let dk = jsonwebtoken::DecodingKey::from_rsa_pem(TEST_FCM_RSA_PUBLIC_KEY_PEM.as_bytes())
            .unwrap();
        let mut v = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::RS256);
        v.validate_aud = false; // checked below; the mock's port is not known here
        let claims = jsonwebtoken::decode::<serde_json::Value>(assertion, &dk, &v)
            .unwrap()
            .claims;
        assert_eq!(claims["iss"], "push@kaiku-test.iam.gserviceaccount.com");
        assert_eq!(
            claims["scope"],
            "https://www.googleapis.com/auth/firebase.messaging"
        );
        assert!(claims["aud"].as_str().unwrap().ends_with("/token"));
        assert_eq!(
            claims["exp"].as_i64().unwrap() - claims["iat"].as_i64().unwrap(),
            3600
        );
        (
            StatusCode::OK,
            json!({"access_token": format!("at-{n}"), "expires_in": 3599, "token_type": "Bearer"})
                .to_string(),
        )
    }

    async fn send_handler(
        State(m): State<Mock>,
        headers: HeaderMap,
        body: String,
    ) -> (StatusCode, String) {
        m.sends.fetch_add(1, Ordering::SeqCst);
        let bearer = headers
            .get("authorization")
            .and_then(|h| h.to_str().ok())
            .unwrap_or_default()
            .to_string();
        m.bearers.lock().unwrap().push(bearer);
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert!(v["message"]["token"].is_string());
        let next = m.queue.lock().unwrap().pop_front();
        match next {
            Some((s, b)) => (StatusCode::from_u16(s).unwrap(), b),
            None => (
                StatusCode::OK,
                json!({"name": "projects/kaiku-test/messages/1"}).to_string(),
            ),
        }
    }

    async fn spawn_mock(m: Mock) -> String {
        let app = Router::new()
            .route("/token", post(token_handler))
            .route("/v1/projects/kaiku-test/messages:send", post(send_handler))
            .with_state(m);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        format!("http://{addr}")
    }

    // ---- wire format (C20) ------------------------------------------------

    #[test]
    fn message_is_data_only_with_string_values() {
        let id = Uuid::now_v7();
        let v = fcm_message("tok", &WakeSignal::dm(id, 3));
        assert!(v["message"].get("notification").is_none());
        assert_eq!(v["message"]["token"], "tok");
        assert_eq!(
            v["message"]["data"],
            json!({"t":"dm","channel_id":id.to_string(),"count":"3"})
        );
        assert_eq!(v["message"]["android"]["priority"], "HIGH");
        assert_eq!(v["message"]["android"]["collapse_key"], id.to_string());
        assert_eq!(v["message"]["android"]["ttl"], "86400s");
        assert!(v["message"]["android"].get("notification").is_none());
    }

    #[test]
    fn mention_signal_is_high_priority() {
        let id = Uuid::now_v7();
        let v = fcm_message("tok", &WakeSignal::mention(id, 2));
        assert_eq!(v["message"]["data"]["t"], "mention");
        assert_eq!(v["message"]["android"]["priority"], "HIGH");
    }

    #[test]
    fn read_signal_is_normal_priority() {
        let id = Uuid::now_v7();
        let sig = WakeSignal::read(id);
        assert!(!sig.is_high_priority());
        let v = fcm_message("tok", &sig);
        assert_eq!(v["message"]["data"]["t"], "read");
        assert_eq!(v["message"]["data"]["count"], "0");
        assert_eq!(v["message"]["android"]["priority"], "NORMAL");
    }

    // ---- response classification ------------------------------------------

    #[test]
    fn classify_unregistered_is_gone() {
        assert_eq!(
            classify_fcm_response(404, &fcm_error("NOT_FOUND", "UNREGISTERED")),
            FcmOutcome::Gone
        );
        assert_eq!(classify_fcm_response(200, "{}"), FcmOutcome::Sent);
    }

    #[test]
    fn classify_bare_404_and_400_unregistered_are_failed() {
        // A bare 404 (e.g. wrong URL or project) must not prune.
        assert_eq!(classify_fcm_response(404, ""), FcmOutcome::Failed);
        assert_eq!(
            classify_fcm_response(404, &fcm_error("NOT_FOUND", "INVALID_ARGUMENT")),
            FcmOutcome::Failed
        );
        // UNREGISTERED only counts together with 404.
        assert_eq!(
            classify_fcm_response(400, &fcm_error("INVALID_ARGUMENT", "UNREGISTERED")),
            FcmOutcome::Failed
        );
        assert_eq!(classify_fcm_response(200, "{}"), FcmOutcome::Sent);
    }

    #[test]
    fn classify_sender_mismatch_is_gone() {
        assert_eq!(
            classify_fcm_response(403, &fcm_error("PERMISSION_DENIED", "SENDER_ID_MISMATCH")),
            FcmOutcome::Gone
        );
        // A 403 for any other reason (e.g. our credentials) must not prune.
        assert_eq!(
            classify_fcm_response(
                403,
                &fcm_error("PERMISSION_DENIED", "THIRD_PARTY_AUTH_ERROR")
            ),
            FcmOutcome::Failed
        );
    }

    #[test]
    fn classify_400_invalid_argument_is_failed_not_gone() {
        assert_eq!(
            classify_fcm_response(400, &fcm_error("INVALID_ARGUMENT", "INVALID_ARGUMENT")),
            FcmOutcome::Failed
        );
        assert_eq!(classify_fcm_response(400, "not json"), FcmOutcome::Failed);
    }

    #[test]
    fn classify_quota_and_unavailable_are_retryable() {
        assert_eq!(
            classify_fcm_response(429, &fcm_error("RESOURCE_EXHAUSTED", "QUOTA_EXCEEDED")),
            FcmOutcome::Retryable
        );
        assert_eq!(
            classify_fcm_response(503, &fcm_error("UNAVAILABLE", "UNAVAILABLE")),
            FcmOutcome::Retryable
        );
        assert_eq!(classify_fcm_response(500, ""), FcmOutcome::Retryable);
        assert_eq!(classify_fcm_response(401, ""), FcmOutcome::Unauthorized);
    }

    #[test]
    fn error_code_ignores_details_of_other_types() {
        let body = json!({"error": {"details": [
            {"@type": "type.googleapis.com/google.rpc.BadRequest", "errorCode": "UNREGISTERED"}
        ]}})
        .to_string();
        assert_eq!(classify_fcm_response(400, &body), FcmOutcome::Failed);
    }

    // ---- service account loading ------------------------------------------

    #[test]
    fn service_account_debug_redacts_private_key() {
        let k = key(GOOGLE_TOKEN_URI.into());
        let dbg = format!("{k:?}");
        assert!(!dbg.contains("BEGIN"), "{dbg}");
        assert!(!dbg.contains("test-key-id"), "{dbg}");
        assert!(dbg.contains("kaiku-test"));
        assert!(dbg.contains("push@kaiku-test.iam.gserviceaccount.com"));
    }

    #[test]
    fn load_rejects_both_sources_and_foreign_token_uri() {
        // Neither → FCM off.
        assert!(load_service_account(None, None).unwrap().is_none());

        // Both → error.
        let good = sa_json(GOOGLE_TOKEN_URI);
        assert!(load_service_account(Some("/x".into()), Some(good.clone())).is_err());

        // Valid inline JSON loads.
        let k = load_service_account(None, Some(good.clone()))
            .unwrap()
            .unwrap();
        assert_eq!(k.project_id, "kaiku-test");

        // Foreign token_uri → error.
        let foreign = sa_json("https://evil.example/token");
        assert!(load_service_account(None, Some(foreign)).is_err());

        // Invalid JSON → error, and the message never echoes the input.
        let err = load_service_account(None, Some("{not json BEGIN PRIVATE".into()))
            .unwrap_err()
            .to_string();
        assert!(!err.contains("BEGIN"), "{err}");

        // Not an RSA private key (Ed25519 test key / public key / garbage).
        for pk in [
            "-----BEGIN PRIVATE KEY-----\nMC4CAQAwBQYDK2VwBCIEIFnP1ht3Mr9d8bran3Wb2LaqI+jwcgcExazvWJP5e+Ho\n-----END PRIVATE KEY-----\n",
            TEST_FCM_RSA_PUBLIC_KEY_PEM,
            "nope",
        ] {
            let mut v: serde_json::Value = serde_json::from_str(&good).unwrap();
            v["private_key"] = json!(pk);
            let err = load_service_account(None, Some(v.to_string()));
            assert!(err.is_err(), "accepted non-RSA key");
            assert!(!err.unwrap_err().to_string().contains("BEGIN"));
        }

        // Unreadable file → error naming no contents.
        assert!(load_service_account(Some("/nonexistent/kaiku-fcm.json".into()), None).is_err());

        // File source works.
        let dir = std::env::temp_dir().join(format!("kaiku-fcm-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sa.json");
        std::fs::write(&path, &good).unwrap();
        let k = load_service_account(Some(path.to_string_lossy().into_owned()), None)
            .unwrap()
            .unwrap();
        assert_eq!(k.client_email, "push@kaiku-test.iam.gserviceaccount.com");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    // ---- OAuth2 token handling --------------------------------------------

    #[tokio::test]
    async fn access_token_is_cached_and_single_flight() {
        let m = Mock {
            token_delay_ms: 200,
            ..Mock::default()
        };
        let base = spawn_mock(m.clone()).await;
        let client = Arc::new(FcmClient::new(key(format!("{base}/token")), &base).unwrap());
        assert_eq!(client.project_id(), "kaiku-test");
        let sig = WakeSignal::dm(Uuid::now_v7(), 1);
        let (a, b) = tokio::join!(client.send("tok-a", &sig), client.send("tok-b", &sig));
        assert_eq!(a, Delivery::Sent);
        assert_eq!(b, Delivery::Sent);
        assert_eq!(m.token_hits.load(Ordering::SeqCst), 1);
        assert_eq!(m.sends.load(Ordering::SeqCst), 2);
        // A third send reuses the cached token.
        assert_eq!(client.send("tok-c", &sig).await, Delivery::Sent);
        assert_eq!(m.token_hits.load(Ordering::SeqCst), 1);
        assert!(m.bearers.lock().unwrap().iter().all(|b| b == "Bearer at-1"));
    }

    #[tokio::test]
    async fn unauthorized_refreshes_token_once() {
        let m = Mock::default();
        m.queue.lock().unwrap().push_back((
            401,
            json!({"error": {"status": "UNAUTHENTICATED"}}).to_string(),
        ));
        let base = spawn_mock(m.clone()).await;
        let client = FcmClient::new(key(format!("{base}/token")), &base).unwrap();
        let sig = WakeSignal::dm(Uuid::now_v7(), 1);
        assert_eq!(client.send("tok", &sig).await, Delivery::Sent);
        assert_eq!(m.token_hits.load(Ordering::SeqCst), 2);
        assert_eq!(m.sends.load(Ordering::SeqCst), 2);
        assert_eq!(
            *m.bearers.lock().unwrap(),
            vec!["Bearer at-1".to_string(), "Bearer at-2".to_string()]
        );
    }

    #[tokio::test]
    async fn repeated_unauthorized_fails_without_looping() {
        let m = Mock::default();
        for _ in 0..3 {
            m.queue.lock().unwrap().push_back((401, String::new()));
        }
        let base = spawn_mock(m.clone()).await;
        let client = FcmClient::new(key(format!("{base}/token")), &base).unwrap();
        let sig = WakeSignal::dm(Uuid::now_v7(), 1);
        assert_eq!(client.send("tok", &sig).await, Delivery::Failed);
        assert_eq!(m.sends.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn send_outcomes_map_to_delivery() {
        let m = Mock::default();
        {
            let mut q = m.queue.lock().unwrap();
            q.push_back((404, fcm_error("NOT_FOUND", "UNREGISTERED")));
            q.push_back((400, fcm_error("INVALID_ARGUMENT", "INVALID_ARGUMENT")));
            q.push_back((503, String::new()));
        }
        let base = spawn_mock(m.clone()).await;
        let client = FcmClient::new(key(format!("{base}/token")), &base).unwrap();
        let sig = WakeSignal::dm(Uuid::now_v7(), 1);
        assert_eq!(client.send("t", &sig).await, Delivery::Gone);
        assert_eq!(client.send("t", &sig).await, Delivery::Failed);
        assert_eq!(client.send("t", &sig).await, Delivery::Failed);
    }

    #[tokio::test]
    async fn failed_token_fetch_cools_down() {
        let m = Mock {
            token_status: Some(503),
            ..Mock::default()
        };
        let base = spawn_mock(m.clone()).await;
        let client = FcmClient::new(key(format!("{base}/token")), &base).unwrap();
        let sig = WakeSignal::dm(Uuid::now_v7(), 1);
        assert_eq!(client.send("t", &sig).await, Delivery::Failed);
        assert_eq!(m.token_hits.load(Ordering::SeqCst), 1);
        // Within the cooldown: fail fast, the token endpoint is not contacted.
        for _ in 0..3 {
            assert_eq!(client.send("t", &sig).await, Delivery::Failed);
        }
        assert_eq!(m.token_hits.load(Ordering::SeqCst), 1);
        assert_eq!(m.sends.load(Ordering::SeqCst), 0);
        // After the cooldown the next send fetches again.
        // (No Instant subtraction: it can underflow on low-uptime hosts.)
        client.token.lock().await.retry_after = Some(Instant::now());
        assert_eq!(client.send("t", &sig).await, Delivery::Failed);
        assert_eq!(m.token_hits.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn failed_refresh_keeps_using_unexpired_token() {
        let m = Mock {
            token_status: Some(503),
            ..Mock::default()
        };
        let base = spawn_mock(m.clone()).await;
        let client = FcmClient::new(key(format!("{base}/token")), &base).unwrap();
        // Cached token inside the refresh margin, but not expired.
        client.token.lock().await.cached = Some(CachedToken {
            access_token: "at-old".into(),
            expires_at: Instant::now() + Duration::from_mins(1),
        });
        let sig = WakeSignal::dm(Uuid::now_v7(), 1);
        // Refresh attempt fails (503) → send still goes out with the old token.
        assert_eq!(client.send("t", &sig).await, Delivery::Sent);
        assert_eq!(m.token_hits.load(Ordering::SeqCst), 1);
        // During the cooldown: old token again, no new fetch.
        assert_eq!(client.send("t", &sig).await, Delivery::Sent);
        assert_eq!(m.token_hits.load(Ordering::SeqCst), 1);
        assert!(m
            .bearers
            .lock()
            .unwrap()
            .iter()
            .all(|b| b == "Bearer at-old"));
        // Once the cached token has expired, the cooldown means Failed.
        client
            .token
            .lock()
            .await
            .cached
            .as_mut()
            .unwrap()
            .expires_at = Instant::now();
        assert_eq!(client.send("t", &sig).await, Delivery::Failed);
        assert_eq!(m.token_hits.load(Ordering::SeqCst), 1);
        assert_eq!(m.sends.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn load_rejects_malformed_project_id() {
        for bad in [
            "",
            "abcd",
            "Kaiku-test",
            "1kaiku",
            "kaiku_test",
            "kaiku/../x",
            &"a".repeat(31),
        ] {
            let mut v: serde_json::Value =
                serde_json::from_str(&sa_json(GOOGLE_TOKEN_URI)).unwrap();
            v["project_id"] = json!(bad);
            let err = load_service_account(None, Some(v.to_string()));
            assert!(err.is_err(), "accepted project_id {bad:?}");
            let msg = err.unwrap_err().to_string();
            assert!(bad.is_empty() || !msg.contains(bad), "{msg}");
        }
        for good in ["kaiku-test", "abcde", "my-project-123456"] {
            let mut v: serde_json::Value =
                serde_json::from_str(&sa_json(GOOGLE_TOKEN_URI)).unwrap();
            v["project_id"] = json!(good);
            assert!(
                load_service_account(None, Some(v.to_string())).is_ok(),
                "{good}"
            );
        }
    }

    #[tokio::test]
    async fn unreachable_token_endpoint_is_failed() {
        // Port 9 on loopback: nothing listens, nothing leaves the host.
        let client =
            FcmClient::new(key("http://127.0.0.1:9/token".into()), "http://127.0.0.1:9").unwrap();
        let sig = WakeSignal::dm(Uuid::now_v7(), 1);
        assert_eq!(client.send("t", &sig).await, Delivery::Failed);
    }
}
