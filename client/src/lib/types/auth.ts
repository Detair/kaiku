/**
 * Authentication DTOs: login/register requests and token responses.
 */

export interface LoginRequest {
  server_url: string;
  username: string;
  password: string;
}

export interface RegisterRequest {
  server_url: string;
  username: string;
  email?: string;
  password: string;
  display_name?: string;
}

export interface TokenResponse {
  access_token: string;
  refresh_token: string;
  expires_in: number;
  token_type: string;
}

// OIDC / SSO Types

/** Public OIDC provider info returned from server settings. */
export interface OidcProvider {
  slug: string;
  display_name: string;
  icon_hint: string | null;
}

/** Result from Tauri OIDC login flow (tokens returned directly). */
export interface OidcLoginResult {
  access_token: string;
  refresh_token: string;
  expires_in: number;
  setup_required: boolean;
}

/** Which auth methods are enabled on the server. */
export interface AuthMethodsConfig {
  local: boolean;
  oidc: boolean;
}

/** Server settings response (public, unauthenticated). */
export interface ServerSettings {
  require_e2ee_setup: boolean;
  oidc_enabled: boolean;
  oidc_providers: OidcProvider[];
  auth_methods: AuthMethodsConfig;
  registration_policy: string;
}

/** Admin auth settings response. */
export interface AuthSettingsResponse {
  auth_methods: AuthMethodsConfig;
  registration_policy: string;
}

/** Admin OIDC provider (full detail, secrets masked). */
export interface AdminOidcProvider {
  id: string;
  slug: string;
  display_name: string;
  icon_hint: string | null;
  provider_type: string;
  issuer_url: string | null;
  authorization_url: string | null;
  token_url: string | null;
  userinfo_url: string | null;
  client_id: string;
  scopes: string;
  enabled: boolean;
  position: number;
  created_at: string;
}
