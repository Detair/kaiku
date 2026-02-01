-- OIDC/OAuth2 identity providers for SSO
-- Supports both OIDC discovery (issuer_url) and manual OAuth2 endpoints (authorization_url + token_url)

CREATE TABLE oidc_providers (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    slug VARCHAR(64) UNIQUE NOT NULL,
    display_name VARCHAR(128) NOT NULL,
    icon_hint VARCHAR(64),
    provider_type VARCHAR(16) NOT NULL DEFAULT 'custom',
    -- OIDC/OAuth2 config
    issuer_url TEXT,
    authorization_url TEXT,
    token_url TEXT,
    userinfo_url TEXT,
    client_id VARCHAR(512) NOT NULL,
    client_secret_encrypted TEXT NOT NULL,
    scopes VARCHAR(512) NOT NULL DEFAULT 'openid profile email',
    -- State
    enabled BOOLEAN NOT NULL DEFAULT true,
    position INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_by UUID REFERENCES users(id) ON DELETE SET NULL,
    CONSTRAINT slug_format CHECK (slug ~ '^[a-z0-9][a-z0-9-]{0,62}[a-z0-9]$'),
    CONSTRAINT valid_provider_type CHECK (provider_type IN ('preset', 'custom')),
    CONSTRAINT has_discovery_or_manual CHECK (
        issuer_url IS NOT NULL OR (authorization_url IS NOT NULL AND token_url IS NOT NULL)
    )
);

-- Auth methods configuration
INSERT INTO server_config (key, value)
VALUES ('auth_methods_allowed', '{"local": true, "oidc": false}'::jsonb)
ON CONFLICT (key) DO NOTHING;
