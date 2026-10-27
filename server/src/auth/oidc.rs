//! OIDC/OAuth2 Provider Management
//!
//! Supports both OIDC discovery (Google) and manual `OAuth2` endpoints (GitHub).
//! Client secrets are encrypted at rest using AES-256-GCM.

use std::collections::HashMap;
use std::sync::LazyLock;

use base64::Engine as _;
use openidconnect::core::{CoreClient, CoreProviderMetadata, CoreResponseType};
use openidconnect::{
    AuthenticationFlow, AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointMaybeSet,
    EndpointNotSet, EndpointSet, IssuerUrl, Nonce, OAuth2TokenResponse, PkceCodeChallenge,
    PkceCodeVerifier, RedirectUrl, Scope,
};
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::Digest as _;
use sqlx::PgPool;
use tokio::sync::RwLock;
use tracing::{info, warn};
use uuid::Uuid;

use super::mfa_crypto::{decrypt_mfa_secret, encrypt_mfa_secret};
use crate::db::{self, OidcProviderRow, PublicOidcProvider};

/// Type alias for the OIDC client returned by `from_provider_metadata`.
///
/// Discovery sets `AuthUrl`, but `TokenUrl` and `UserInfoUrl` are `EndpointMaybeSet`
/// (they might not be in the provider metadata).
type DiscoveredClient = CoreClient<
    EndpointSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointMaybeSet,
    EndpointMaybeSet,
>;

/// User info extracted from an OIDC/OAuth2 provider.
#[derive(Debug, Clone)]
pub struct OidcUserInfo {
    /// Provider-specific subject identifier.
    pub subject: String,
    /// User's email address.
    pub email: Option<String>,
    /// User's display name.
    pub name: Option<String>,
    /// User's preferred username.
    pub preferred_username: Option<String>,
    /// User's avatar URL.
    pub avatar_url: Option<String>,
}

/// OIDC state stored in Redis during the auth flow.
///
/// Deserialized through [`OidcFlowStateWire`] so states written before the
/// `purpose` field existed (a bare `link_user_id`, or neither) still finish
/// as the flow they were started as.
#[derive(Debug, Serialize, Deserialize)]
#[serde(from = "OidcFlowStateWire")]
pub struct OidcFlowState {
    /// Provider slug.
    pub slug: String,
    /// PKCE code verifier (base64).
    pub pkce_verifier: String,
    /// Nonce for ID token verification.
    pub nonce: String,
    /// Redirect URI for callback.
    pub redirect_uri: String,
    /// When the state was created (for debugging).
    pub created_at: i64,
    /// What the flow is for: login, link or re-authentication. Exactly one;
    /// the callback finishes the flow according to it.
    pub purpose: OidcFlowPurpose,
    /// Desktop (Tauri) loopback address the server hands the flow result to
    /// after it has exchanged the code at its own callback. `None` is the
    /// browser popup flow (result via `postMessage`). Never sent to the provider.
    ///
    /// Flow states stored before this field existed deserialize as `None`
    /// (browser). Such in-flight *desktop* flows cannot complete and must be
    /// restarted; the states expire after 10 minutes, so no migration is needed.
    #[serde(default)]
    pub client_redirect_uri: Option<String>,
    /// Desktop login only: S256 challenge (unpadded base64url SHA-256 of the
    /// desktop's verifier) the hand-off code is bound to. `None` for every
    /// other flow; a desktop login state without one cannot complete.
    #[serde(default)]
    pub client_challenge: Option<String>,
}

/// What an OIDC flow is for; persisted in the encrypted flow state so the
/// shared callback knows how to finish it. The variants exclude each other.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OidcFlowPurpose {
    /// Log in (or register) with the provider.
    #[default]
    Login,
    /// Link the provider identity to this signed-in user; the session that
    /// started the flow, if known (an identity linked from an elevated session
    /// may confirm re-authentication right away, C24).
    Link {
        user_id: Uuid,
        #[serde(default)]
        session_id: Option<Uuid>,
    },
    /// Re-authenticate this user and elevate the bound session (C24). Asks
    /// the provider for a fresh login and uses a shorter state TTL.
    Reauth(OidcReauthBinding),
}

impl OidcFlowPurpose {
    /// Short name for logs.
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Login => "login",
            Self::Link { .. } => "link",
            Self::Reauth(_) => "reauth",
        }
    }
}

/// Serialized form of [`OidcFlowState`], also accepting the legacy
/// `link_user_id` field of states written before `purpose` existed.
#[derive(Deserialize)]
struct OidcFlowStateWire {
    slug: String,
    pkce_verifier: String,
    nonce: String,
    redirect_uri: String,
    created_at: i64,
    #[serde(default)]
    purpose: Option<OidcFlowPurpose>,
    #[serde(default)]
    link_user_id: Option<Uuid>,
    #[serde(default)]
    client_redirect_uri: Option<String>,
    #[serde(default)]
    client_challenge: Option<String>,
}

impl From<OidcFlowStateWire> for OidcFlowState {
    fn from(wire: OidcFlowStateWire) -> Self {
        let purpose = match (wire.purpose, wire.link_user_id) {
            (Some(purpose), _) => purpose,
            (None, Some(user_id)) => OidcFlowPurpose::Link {
                user_id,
                session_id: None,
            },
            (None, None) => OidcFlowPurpose::Login,
        };
        Self {
            slug: wire.slug,
            pkce_verifier: wire.pkce_verifier,
            nonce: wire.nonce,
            redirect_uri: wire.redirect_uri,
            created_at: wire.created_at,
            purpose,
            client_redirect_uri: wire.client_redirect_uri,
            client_challenge: wire.client_challenge,
        }
    }
}

/// What a re-authentication flow is bound to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OidcReauthBinding {
    /// The signed-in user who started the flow.
    pub user_id: Uuid,
    /// The session (access token `sid`) to elevate.
    pub session_id: Uuid,
    /// Elevation reason for the audit log.
    pub reason: Option<String>,
    /// A TOTP code was verified when the flow started (required when the
    /// account has MFA on).
    #[serde(default)]
    pub mfa_verified: bool,
}

/// Verified ID token claims needed for re-authentication.
#[derive(Debug, Clone)]
pub struct VerifiedIdClaims {
    /// `sub` of the verified ID token.
    pub subject: String,
    /// `auth_time`, when the provider sent it.
    pub auth_time: Option<chrono::DateTime<chrono::Utc>>,
}

/// Where an OIDC flow sends the provider and, afterwards, the client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowRedirects {
    /// `redirect_uri` sent to the provider (and repeated in the code exchange).
    pub provider_redirect_uri: String,
    /// Desktop loopback return address; `None` for the browser flow.
    pub client_redirect_uri: Option<String>,
}

/// Why [`resolve_flow_redirects`] refused a flow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlowRedirectError {
    /// The client-supplied return address is not an `http` loopback URL.
    InvalidClientRedirect,
    /// `PUBLIC_URL` is not configured, so the server callback is unknown.
    MissingPublicUrl,
}

/// Whether `url` matches the desktop return-address contract.
///
/// `http://{localhost|127.0.0.1}[:port]/callback[?flow=<per-flow value>]`:
/// no userinfo, no fragment, no other query, path exactly `/callback`, any port.
pub fn is_loopback_client_redirect(url: &openidconnect::url::Url) -> bool {
    matches!(
        (url.scheme(), url.host_str()),
        ("http", Some("localhost" | "127.0.0.1"))
    ) && url.username().is_empty()
        && url.password().is_none()
        && url.path() == "/callback"
        && url.query().is_none_or(is_client_flow_query)
        && url.fragment().is_none()
}

/// Resolve the provider and client redirect targets for a new flow.
///
/// `client_redirect_uri` is the optional desktop loopback address from the
/// authorize request; `public_url` is the server's `PUBLIC_URL`.
///
/// The provider always redirects to the server's own `/auth/oidc/callback`:
/// only the server holds the PKCE verifier and client secret, so only it can
/// exchange the code. A desktop loopback address is kept separately and used
/// solely to hand the result back once the server has finished the flow.
pub fn resolve_flow_redirects(
    client_redirect_uri: Option<&str>,
    public_url: Option<&str>,
) -> Result<FlowRedirects, FlowRedirectError> {
    let client_redirect_uri = client_redirect_uri
        .map(|uri| {
            openidconnect::url::Url::parse(uri)
                .ok()
                .filter(is_loopback_client_redirect)
                .map(|_| uri.to_string())
                .ok_or(FlowRedirectError::InvalidClientRedirect)
        })
        .transpose()?;
    let public_url = public_url.ok_or(FlowRedirectError::MissingPublicUrl)?;
    Ok(FlowRedirects {
        provider_redirect_uri: format!("{}/auth/oidc/callback", public_url.trim_end_matches('/')),
        client_redirect_uri,
    })
}

/// The desktop loopback URL to hand a finished flow's result to, or `None` for
/// the browser popup flow. Re-validates the stored address (defence in depth).
pub fn client_return_url(flow_state: &OidcFlowState) -> Option<openidconnect::url::Url> {
    flow_state
        .client_redirect_uri
        .as_deref()
        .and_then(|uri| openidconnect::url::Url::parse(uri).ok())
        .filter(is_loopback_client_redirect)
}

/// Whether every byte of `s` is base64url (`A-Z a-z 0-9 - _`) and its length
/// is within `min..=max`.
fn is_base64url(s: &str, min: usize, max: usize) -> bool {
    (min..=max).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Query the desktop may carry in its return address: exactly one per-flow
/// value `flow=<base64url, 22–128 chars>`, handed back unchanged so the
/// desktop listener can tell its own flow from stray requests.
fn is_client_flow_query(query: &str) -> bool {
    query
        .strip_prefix("flow=")
        .is_some_and(|v| is_base64url(v, 22, 128))
}

/// Whether `challenge` has the form of an S256 client challenge: an unpadded
/// base64url SHA-256 digest (43 characters).
pub fn is_valid_client_challenge(challenge: &str) -> bool {
    is_base64url(challenge, 43, 43)
}

/// Whether `verifier` (RFC 7636 charset and length, 43–128) hashes to
/// `challenge` with S256. Compared in constant time.
pub fn client_challenge_matches(verifier: &str, challenge: &str) -> bool {
    let verifier_ok = (43..=128).contains(&verifier.len())
        && verifier
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~'));
    if !verifier_ok || !is_valid_client_challenge(challenge) {
        return false;
    }
    let computed = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(sha2::Sha256::digest(verifier.as_bytes()));
    // Length-fixed XOR fold (both sides are 43 bytes), no early exit. Not a
    // vetted primitive, but the code is consumed before this check, so a
    // timing difference could not be retried against the same grant.
    computed
        .bytes()
        .zip(challenge.bytes())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

/// Lifetime of a desktop hand-off code.
pub const OIDC_HANDOFF_TTL_SECS: i64 = 60;

/// What a desktop hand-off code grants once redeemed with the verifier
/// (`POST /auth/oidc/desktop/exchange`). Stored in Valkey under
/// [`handoff_key`] for at most [`OIDC_HANDOFF_TTL_SECS`]; holds no token.
#[derive(Debug, Serialize, Deserialize)]
pub struct OidcHandoffGrant {
    /// The account the provider login resolved to.
    pub user_id: Uuid,
    /// Provider slug (logs).
    pub slug: String,
    /// S256 challenge from the authorize request.
    pub client_challenge: String,
    /// Unix seconds when the code was issued.
    pub created_at: i64,
}

/// Valkey key of a hand-off code (only its SHA-256 is stored).
pub fn handoff_key(code: &str) -> String {
    format!(
        "oidc:handoff:{}",
        hex::encode(sha2::Sha256::digest(code.as_bytes()))
    )
}

/// A fresh hand-off code: 32 random bytes, unpadded base64url.
pub fn new_handoff_code() -> String {
    let bytes: [u8; 32] = rand::thread_rng().gen();
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// Cached provider configuration with pre-built client.
struct CachedProvider {
    row: OidcProviderRow,
    /// Pre-built openidconnect client (only for OIDC discovery providers).
    oidc_client: Option<DiscoveredClient>,
}

/// Manages OIDC/OAuth2 providers loaded from the database.
pub struct OidcProviderManager {
    providers: RwLock<HashMap<String, CachedProvider>>,
    encryption_key: Vec<u8>,
    http_client: reqwest::Client,
}

/// Build a shared reqwest client for OIDC operations.
///
/// Disables redirects to prevent SSRF via authorization server responses.
fn build_oidc_http_client() -> reqwest::Client {
    reqwest::ClientBuilder::new()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("Failed to build OIDC HTTP client")
}

impl OidcProviderManager {
    /// Create a new manager with the given encryption key.
    pub fn new(encryption_key: Vec<u8>) -> Self {
        Self {
            providers: RwLock::new(HashMap::new()),
            encryption_key,
            http_client: build_oidc_http_client(),
        }
    }

    /// Load (or reload) all enabled providers from the database.
    pub async fn load_providers(&self, pool: &PgPool) -> anyhow::Result<()> {
        let rows = db::list_oidc_providers(pool).await?;
        let mut map = HashMap::new();

        for row in rows {
            match self.build_cached_provider(row).await {
                Ok(cached) => {
                    info!(slug = %cached.row.slug, "Loaded OIDC provider");
                    map.insert(cached.row.slug.clone(), cached);
                }
                Err(e) => {
                    warn!(error = %e, "Failed to load OIDC provider, skipping");
                }
            }
        }

        *self.providers.write().await = map;
        Ok(())
    }

    /// Build a cached provider from a database row.
    async fn build_cached_provider(&self, row: OidcProviderRow) -> anyhow::Result<CachedProvider> {
        let oidc_client = if let Some(ref issuer_url) = row.issuer_url {
            match self.build_oidc_client(&row, issuer_url).await {
                Ok(client) => Some(client),
                Err(e) => {
                    warn!(
                        slug = %row.slug,
                        error = %e,
                        "Failed to discover OIDC metadata, provider will use manual endpoints"
                    );
                    None
                }
            }
        } else {
            None
        };

        Ok(CachedProvider { row, oidc_client })
    }

    /// Build an openidconnect `CoreClient` via OIDC discovery.
    async fn build_oidc_client(
        &self,
        row: &OidcProviderRow,
        issuer_url: &str,
    ) -> anyhow::Result<DiscoveredClient> {
        let issuer = IssuerUrl::new(issuer_url.to_string())?;
        let metadata = CoreProviderMetadata::discover_async(issuer, &self.http_client).await?;

        let client_secret = self.decrypt_secret(&row.client_secret_encrypted)?;

        let client = CoreClient::from_provider_metadata(
            metadata,
            ClientId::new(row.client_id.clone()),
            Some(ClientSecret::new(client_secret)),
        );

        Ok(client)
    }

    /// Get a provider's row by slug.
    pub async fn get_provider_row(&self, slug: &str) -> Option<OidcProviderRow> {
        self.providers.read().await.get(slug).map(|p| p.row.clone())
    }

    /// List public info for all enabled providers.
    pub async fn list_public(&self) -> Vec<PublicOidcProvider> {
        let providers = self.providers.read().await;
        let mut list: Vec<_> = providers
            .values()
            .filter(|p| p.row.enabled)
            .map(|p| PublicOidcProvider {
                slug: p.row.slug.clone(),
                display_name: p.row.display_name.clone(),
                icon_hint: p.row.icon_hint.clone(),
            })
            .collect();
        list.sort_by_key(|p| p.slug.clone());
        list
    }

    /// Generate authorization URL for a provider.
    ///
    /// Returns (`auth_url`, state, nonce, `pkce_verifier`).
    pub async fn generate_auth_url(
        &self,
        slug: &str,
        redirect_uri: &str,
    ) -> anyhow::Result<(String, String, String, String)> {
        self.generate_auth_url_with(slug, redirect_uri, false).await
    }

    /// Whether the provider uses OIDC discovery, so its code exchange returns
    /// a verified ID token (needed for re-authentication).
    pub async fn issues_id_tokens(&self, slug: &str) -> bool {
        self.providers
            .read()
            .await
            .get(slug)
            .is_some_and(|p| p.oidc_client.is_some())
    }

    /// Like [`Self::generate_auth_url`]; `force_login` asks the provider for
    /// a fresh login (`prompt=login`, `max_age=0`), as re-authentication
    /// needs. It is only honoured by OIDC discovery providers.
    pub async fn generate_auth_url_with(
        &self,
        slug: &str,
        redirect_uri: &str,
        force_login: bool,
    ) -> anyhow::Result<(String, String, String, String)> {
        let providers = self.providers.read().await;
        let cached = providers
            .get(slug)
            .ok_or_else(|| anyhow::anyhow!("Provider not found: {slug}"))?;

        let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();
        let state = CsrfToken::new_random();
        let nonce = Nonce::new_random();

        let auth_url = if let Some(ref client) = cached.oidc_client {
            // OIDC discovery flow
            let state_clone = state.clone();
            let nonce_clone = nonce.clone();
            let mut request = client
                .authorize_url(
                    AuthenticationFlow::<CoreResponseType>::AuthorizationCode,
                    move || state_clone,
                    move || nonce_clone,
                )
                .set_redirect_uri(std::borrow::Cow::Owned(RedirectUrl::new(
                    redirect_uri.to_string(),
                )?))
                .set_pkce_challenge(pkce_challenge)
                .add_scopes(
                    cached
                        .row
                        .scopes
                        .split_whitespace()
                        .map(|s| Scope::new(s.to_string())),
                );
            if force_login {
                request = request
                    .add_prompt(openidconnect::core::CoreAuthPrompt::Login)
                    .set_max_age(std::time::Duration::ZERO);
            }
            let (url, _, _) = request.url();
            url.to_string()
        } else {
            // Manual OAuth2 endpoints (GitHub etc.)
            let auth_endpoint = cached
                .row
                .authorization_url
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("No authorization URL for provider {slug}"))?;

            let mut url = openidconnect::url::Url::parse(auth_endpoint)?;
            url.query_pairs_mut()
                .append_pair("client_id", &cached.row.client_id)
                .append_pair("redirect_uri", redirect_uri)
                .append_pair("state", state.secret())
                .append_pair("scope", &cached.row.scopes)
                .append_pair("response_type", "code")
                .append_pair("code_challenge", pkce_challenge.as_str())
                .append_pair("code_challenge_method", "S256");
            url.to_string()
        };

        Ok((
            auth_url,
            state.secret().clone(),
            nonce.secret().clone(),
            pkce_verifier.secret().clone(),
        ))
    }

    /// Exchange an authorization code for tokens.
    ///
    /// Returns the access token and optionally an ID token (for OIDC).
    /// For OIDC discovery providers, the ID token's nonce is verified.
    pub async fn exchange_code(
        &self,
        slug: &str,
        code: &str,
        pkce_verifier: &str,
        redirect_uri: &str,
        nonce: &str,
    ) -> anyhow::Result<(String, Option<String>)> {
        let (access_token, id_token) = self
            .exchange_code_verified(slug, code, pkce_verifier, redirect_uri, nonce)
            .await?;
        Ok((access_token, id_token.map(|(raw, _)| raw)))
    }

    /// Like [`Self::exchange_code`], but also returns the verified ID token
    /// claims (signature, issuer, audience, expiry and nonce checked).
    pub async fn exchange_code_verified(
        &self,
        slug: &str,
        code: &str,
        pkce_verifier: &str,
        redirect_uri: &str,
        nonce: &str,
    ) -> anyhow::Result<(String, Option<(String, VerifiedIdClaims)>)> {
        let providers = self.providers.read().await;
        let cached = providers
            .get(slug)
            .ok_or_else(|| anyhow::anyhow!("Provider not found: {slug}"))?;

        let verifier = PkceCodeVerifier::new(pkce_verifier.to_string());

        if let Some(ref client) = cached.oidc_client {
            // OIDC flow — exchange_code returns Result because TokenUrl is MaybeSet
            let token_response = client
                .exchange_code(AuthorizationCode::new(code.to_string()))
                .map_err(|e| anyhow::anyhow!("Token endpoint not configured: {e}"))?
                .set_redirect_uri(std::borrow::Cow::Owned(RedirectUrl::new(
                    redirect_uri.to_string(),
                )?))
                .set_pkce_verifier(verifier)
                .request_async(&self.http_client)
                .await
                .map_err(|e| anyhow::anyhow!("Token exchange failed: {e}"))?;

            // Verify the ID token (incl. nonce) if present
            let id_token = match token_response.extra_fields().id_token() {
                Some(id_token) => {
                    let verifier = client.id_token_verifier();
                    let expected_nonce = Nonce::new(nonce.to_string());
                    let claims = id_token
                        .claims(&verifier, &expected_nonce)
                        .map_err(|e| anyhow::anyhow!("ID token verification failed: {e}"))?;
                    let verified = VerifiedIdClaims {
                        subject: claims.subject().to_string(),
                        auth_time: claims.auth_time(),
                    };
                    Some((id_token.to_string(), verified))
                }
                None => None,
            };

            let access_token = token_response.access_token().secret().clone();

            Ok((access_token, id_token))
        } else {
            // Manual OAuth2 (GitHub etc.)
            let token_url = cached
                .row
                .token_url
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("No token URL for provider {slug}"))?;

            let client_secret = self.decrypt_secret(&cached.row.client_secret_encrypted)?;

            let http_client = reqwest::Client::new();
            let resp = http_client
                .post(token_url)
                .header("Accept", "application/json")
                .form(&[
                    ("client_id", cached.row.client_id.as_str()),
                    ("client_secret", &client_secret),
                    ("code", code),
                    ("redirect_uri", redirect_uri),
                    ("code_verifier", pkce_verifier),
                    ("grant_type", "authorization_code"),
                ])
                .send()
                .await?;

            let body: serde_json::Value = resp.json().await?;

            let access_token = body["access_token"]
                .as_str()
                .ok_or_else(|| {
                    let err = body["error"].as_str().unwrap_or("unknown");
                    let desc = body["error_description"].as_str().unwrap_or("");
                    anyhow::anyhow!("Token exchange failed: {err} {desc}")
                })?
                .to_string();

            Ok((access_token, None))
        }
    }

    /// Extract user info from the provider using access token.
    pub async fn extract_user_info(
        &self,
        slug: &str,
        access_token: &str,
    ) -> anyhow::Result<OidcUserInfo> {
        let providers = self.providers.read().await;
        let cached = providers
            .get(slug)
            .ok_or_else(|| anyhow::anyhow!("Provider not found: {slug}"))?;

        // Determine userinfo URL
        let userinfo_url = if let Some(ref url) = cached.row.userinfo_url {
            url.clone()
        } else if cached.oidc_client.is_some() {
            // Use OIDC userinfo endpoint via the client
            return self.extract_from_oidc_client(cached, access_token).await;
        } else {
            anyhow::bail!("No userinfo URL for provider {slug}");
        };

        let http_client = reqwest::Client::new();
        let resp = http_client
            .get(&userinfo_url)
            .header("Authorization", format!("Bearer {access_token}"))
            .header("Accept", "application/json")
            .send()
            .await?;

        let body: serde_json::Value = resp.json().await?;

        // GitHub uses "id" (integer), standard OIDC uses "sub"
        let subject = body["sub"]
            .as_str()
            .map(String::from)
            .or_else(|| body["id"].as_u64().map(|id| id.to_string()))
            .ok_or_else(|| anyhow::anyhow!("No subject identifier in userinfo response"))?;

        Ok(OidcUserInfo {
            subject,
            email: body["email"].as_str().map(String::from),
            name: body["name"].as_str().map(String::from),
            preferred_username: body["preferred_username"]
                .as_str()
                .or_else(|| body["login"].as_str())
                .map(String::from),
            avatar_url: body["picture"]
                .as_str()
                .or_else(|| body["avatar_url"].as_str())
                .map(String::from),
        })
    }

    /// Extract user info from an OIDC provider using the openidconnect client.
    async fn extract_from_oidc_client(
        &self,
        cached: &CachedProvider,
        access_token: &str,
    ) -> anyhow::Result<OidcUserInfo> {
        let client = cached
            .oidc_client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No OIDC client"))?;

        // user_info returns Result because UserInfoUrl is MaybeSet
        let userinfo_endpoint = client
            .user_info(
                openidconnect::AccessToken::new(access_token.to_string()),
                None,
            )
            .map_err(|e| anyhow::anyhow!("Userinfo not supported: {e}"))?;

        let claims: openidconnect::UserInfoClaims<
            openidconnect::EmptyAdditionalClaims,
            openidconnect::core::CoreGenderClaim,
        > = userinfo_endpoint
            .request_async(&self.http_client)
            .await
            .map_err(|e| anyhow::anyhow!("Userinfo request failed: {e}"))?;

        Ok(OidcUserInfo {
            subject: claims.subject().to_string(),
            email: claims.email().map(|e| e.to_string()),
            name: claims
                .name()
                .and_then(|n| n.get(None))
                .map(|n| n.to_string()),
            preferred_username: claims.preferred_username().map(|u| u.to_string()),
            avatar_url: claims
                .picture()
                .and_then(|p| p.get(None))
                .map(|p| p.to_string()),
        })
    }

    /// Encrypt a client secret for storage.
    pub fn encrypt_secret(&self, secret: &str) -> anyhow::Result<String> {
        encrypt_mfa_secret(secret, &self.encryption_key)
            .map_err(|e| anyhow::anyhow!("Failed to encrypt secret: {e}"))
    }

    /// Decrypt a stored client secret.
    pub fn decrypt_secret(&self, encrypted: &str) -> anyhow::Result<String> {
        decrypt_mfa_secret(encrypted, &self.encryption_key)
            .map_err(|e| anyhow::anyhow!("Failed to decrypt secret: {e}"))
    }

    /// Seed a provider from legacy environment variables (backward compatibility).
    pub async fn seed_from_env(
        &self,
        config: &crate::config::Config,
        pool: &PgPool,
    ) -> anyhow::Result<()> {
        let (Some(issuer_url), Some(client_id), Some(client_secret)) = (
            config.oidc_issuer_url.as_ref(),
            config.oidc_client_id.as_ref(),
            config.oidc_client_secret.as_ref(),
        ) else {
            return Ok(());
        };

        // Check if already seeded
        if db::get_oidc_provider_by_slug(pool, "legacy-oidc")
            .await
            .is_ok()
        {
            return Ok(());
        }

        let encrypted_secret = self.encrypt_secret(client_secret)?;

        db::create_oidc_provider(
            pool,
            db::CreateOidcProviderParams {
                slug: "legacy-oidc",
                display_name: "SSO Login",
                icon_hint: Some("key"),
                provider_type: "custom",
                issuer_url: Some(issuer_url),
                authorization_url: None,
                token_url: None,
                userinfo_url: None,
                client_id,
                client_secret_encrypted: &encrypted_secret,
                scopes: "openid profile email",
                created_by: None,
            },
        )
        .await?;

        // Enable OIDC in auth methods
        let mut methods = db::get_auth_methods_allowed(pool).await?;
        methods.oidc = true;
        db::set_auth_methods_allowed(pool, &methods, None).await?;

        info!("Seeded legacy OIDC provider from environment variables");
        Ok(())
    }
}

/// GitHub preset configuration.
pub struct GitHubPreset;

impl GitHubPreset {
    pub const SLUG: &'static str = "github";
    pub const DISPLAY_NAME: &'static str = "GitHub";
    pub const ICON_HINT: &'static str = "github";
    pub const AUTHORIZATION_URL: &'static str = "https://github.com/login/oauth/authorize";
    pub const TOKEN_URL: &'static str = "https://github.com/login/oauth/access_token";
    pub const USERINFO_URL: &'static str = "https://api.github.com/user";
    pub const SCOPES: &'static str = "read:user user:email";
}

/// Google preset configuration.
pub struct GooglePreset;

impl GooglePreset {
    pub const SLUG: &'static str = "google";
    pub const DISPLAY_NAME: &'static str = "Google";
    pub const ICON_HINT: &'static str = "chrome";
    pub const ISSUER_URL: &'static str = "https://accounts.google.com";
    pub const SCOPES: &'static str = "openid profile email";
}

/// Generate a username from OIDC claims.
///
/// Priority:
/// 1. `preferred_username` if valid
/// 2. `name` normalized
/// 3. Email local part
/// 4. `user_{random_6}`
// Valid username pattern (3-32 chars, lowercase alphanumeric + underscore).
static VALID_USERNAME: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"^[a-z0-9_]{3,32}$").expect("valid regex"));

pub fn generate_username_from_claims(info: &OidcUserInfo) -> String {
    let valid_pattern = &*VALID_USERNAME;

    // Try preferred_username
    if let Some(ref username) = info.preferred_username {
        let normalized = username.to_lowercase().replace('-', "_");
        if valid_pattern.is_match(&normalized) {
            return normalized;
        }
    }

    // Try name
    if let Some(ref name) = info.name {
        let normalized = name
            .to_lowercase()
            .replace(' ', "_")
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect::<String>();
        if valid_pattern.is_match(&normalized) {
            return normalized;
        }
    }

    // Try email local part
    if let Some(ref email) = info.email {
        if let Some(local) = email.split('@').next() {
            let normalized: String = local
                .to_lowercase()
                .chars()
                .map(|c| if c == '.' || c == '-' { '_' } else { c })
                .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            if valid_pattern.is_match(&normalized) {
                return normalized;
            }
        }
    }

    // Fallback: user_{random_6}
    let suffix: u32 = rand::thread_rng().gen_range(100_000..999_999);
    format!("user_{suffix}")
}

/// Append a random suffix to make a username unique.
///
/// Truncates the base to ensure the full `_XXXX` suffix always fits within 32 chars.
pub fn append_collision_suffix(base: &str) -> String {
    let suffix: u16 = rand::thread_rng().gen_range(1000..9999);
    // Reserve 5 chars for "_XXXX" so the suffix is never truncated
    let max_base = 32 - 5; // 27
    let truncated_base = if base.len() > max_base {
        &base[..max_base]
    } else {
        base
    };
    format!("{truncated_base}_{suffix}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_username_preferred() {
        let info = OidcUserInfo {
            subject: "123".into(),
            email: Some("test@example.com".into()),
            name: Some("Test User".into()),
            preferred_username: Some("testuser".into()),
            avatar_url: None,
        };
        assert_eq!(generate_username_from_claims(&info), "testuser");
    }

    #[test]
    fn test_generate_username_name_fallback() {
        let info = OidcUserInfo {
            subject: "123".into(),
            email: Some("test@example.com".into()),
            name: Some("John Doe".into()),
            preferred_username: None,
            avatar_url: None,
        };
        assert_eq!(generate_username_from_claims(&info), "john_doe");
    }

    #[test]
    fn test_generate_username_email_fallback() {
        let info = OidcUserInfo {
            subject: "123".into(),
            email: Some("jane.doe@example.com".into()),
            name: None,
            preferred_username: None,
            avatar_url: None,
        };
        assert_eq!(generate_username_from_claims(&info), "jane_doe");
    }

    #[test]
    fn test_generate_username_random_fallback() {
        let info = OidcUserInfo {
            subject: "123".into(),
            email: None,
            name: None,
            preferred_username: None,
            avatar_url: None,
        };
        let username = generate_username_from_claims(&info);
        assert!(username.starts_with("user_"));
        assert!(username.len() >= 10);
    }

    #[test]
    fn test_append_collision_suffix() {
        let result = append_collision_suffix("testuser");
        assert!(result.starts_with("testuser_"));
        assert!(result.len() <= 32);
    }
}
