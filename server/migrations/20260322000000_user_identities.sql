-- OAuth/OIDC identity linking: allow one Kaiku account to be reached by
-- multiple external identities.
--
-- Until now a user's single external identity lived in `users.external_id`
-- ("{provider_slug}:{subject}", UNIQUE), which structurally permits only one
-- linked provider per account. This table makes the identity set first-class:
-- the OIDC login lookup resolves through it, and (in a follow-up) authenticated
-- users can attach and remove additional identities.
--
-- `users.external_id` is retained as the original/primary identity marker (it
-- still satisfies the oidc_user_has_external_id CHECK and is back-filled below),
-- but it is no longer the authoritative lookup key.

CREATE TABLE user_identities (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- Provider slug, mirroring oidc_providers.slug. Intentionally NOT a foreign
    -- key: legacy/env-seeded providers and provider deletion must not orphan or
    -- cascade-delete a user's identity history.
    provider_slug VARCHAR(64) NOT NULL,
    -- Provider-specific subject ("sub") claim. May itself contain ':'.
    subject VARCHAR(255) NOT NULL,
    -- Email reported by the provider at link time (informational; nullable).
    email VARCHAR(255),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_used_at TIMESTAMPTZ,

    -- An external identity belongs to exactly one Kaiku account.
    CONSTRAINT user_identities_provider_subject_unique UNIQUE (provider_slug, subject),
    -- At most one identity per provider per account (no two Google logins on one account).
    CONSTRAINT user_identities_user_provider_unique UNIQUE (user_id, provider_slug)
);

CREATE INDEX idx_user_identities_user_id ON user_identities(user_id);

-- Back-fill existing single-identity OIDC users. external_id is
-- "{slug}:{subject}"; the slug never contains ':', so the first ':' is the
-- separator and any remaining ':' stay with the subject.
INSERT INTO user_identities (user_id, provider_slug, subject, email, created_at)
SELECT
    id,
    split_part(external_id, ':', 1),
    substring(external_id FROM position(':' IN external_id) + 1),
    email,
    created_at
FROM users
WHERE auth_method = 'oidc'
  AND external_id IS NOT NULL
  AND position(':' IN external_id) > 0;
