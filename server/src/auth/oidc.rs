//! OIDC/OAuth2 Provider Management
//!
//! Supports both OIDC discovery (Google) and manual `OAuth2` endpoints (GitHub).
//! Client secrets are encrypted at rest using AES-256-GCM.

use std::collections::HashMap;
use std::sync::LazyLock;

use rand::Rng;

use openidconnect::{
    core::{CoreClient, CoreProviderMetadata, CoreResponseType},
    reqwest::async_http_client,
    AuthenticationFlow, AuthorizationCode, ClientId, ClientSecret, CsrfToken, IssuerUrl, Nonce,
    OAuth2TokenResponse, PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, Scope,
};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tokio::sync::RwLock;
use tracing::{info, warn};
use uuid::Uuid;

use super::mfa_crypto::{decrypt_mfa_secret, encrypt_mfa_secret};
use crate::db::{self, OidcProviderRow, PublicOidcProvider};

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
#[derive(Debug, Serialize, Deserialize)]
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
}

/// Cached provider configuration with pre-built client.
struct CachedProvider {
    row: OidcProviderRow,
    /// Pre-built openidconnect client (only for OIDC discovery providers).
    oidc_client: Option<CoreClient>,
}

/// Manages OIDC/OAuth2 providers loaded from the database.
pub struct OidcProviderManager {
    providers: RwLock<HashMap<String, CachedProvider>>,
    encryption_key: Vec<u8>,
}

impl OidcProviderManager {
    /// Create a new manager with the given encryption key.
    pub fn new(encryption_key: Vec<u8>) -> Self {
        Self {
            providers: RwLock::new(HashMap::new()),
            encryption_key,
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
    ) -> anyhow::Result<CoreClient> {
        let issuer = IssuerUrl::new(issuer_url.to_string())?;
        let metadata =
            CoreProviderMetadata::discover_async(issuer, async_http_client).await?;

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
        self.providers
            .read()
            .await
            .get(slug)
            .map(|p| p.row.clone())
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
            let (url, _, _) = client
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
                )
                .url();
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
        let providers = self.providers.read().await;
        let cached = providers
            .get(slug)
            .ok_or_else(|| anyhow::anyhow!("Provider not found: {slug}"))?;

        let verifier = PkceCodeVerifier::new(pkce_verifier.to_string());

        if let Some(ref client) = cached.oidc_client {
            // OIDC flow
            let token_response = client
                .exchange_code(AuthorizationCode::new(code.to_string()))
                .set_redirect_uri(std::borrow::Cow::Owned(RedirectUrl::new(
                    redirect_uri.to_string(),
                )?))
                .set_pkce_verifier(verifier)
                .request_async(async_http_client)
                .await
                .map_err(|e| anyhow::anyhow!("Token exchange failed: {e}"))?;

            // Verify ID token nonce if present
            if let Some(id_token) = token_response.extra_fields().id_token() {
                let verifier = client.id_token_verifier();
                let expected_nonce = Nonce::new(nonce.to_string());
                id_token
                    .claims(&verifier, &expected_nonce)
                    .map_err(|e| anyhow::anyhow!("ID token verification failed: {e}"))?;
            }

            let access_token = token_response.access_token().secret().clone();
            let id_token = token_response
                .extra_fields()
                .id_token()
                .map(|t| t.to_string());

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
            .request_async(async_http_client)
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
            "legacy-oidc",
            "SSO Login",
            Some("key"),
            "custom",
            Some(issuer_url),
            None,
            None,
            None,
            client_id,
            &encrypted_secret,
            "openid profile email",
            Uuid::nil(),
        )
        .await?;

        // Enable OIDC in auth methods
        let mut methods = db::get_auth_methods_allowed(pool).await?;
        methods.oidc = true;
        db::set_auth_methods_allowed(pool, &methods, Uuid::nil()).await?;

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
