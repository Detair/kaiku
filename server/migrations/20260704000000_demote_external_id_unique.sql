-- TD-31: demote users.external_id to non-unique.
--
-- Since the OAuth identity-linking work (user_identities table), the
-- authoritative identity-set uniqueness guarantee is
-- user_identities(provider_slug, subject). users.external_id is retained only
-- as the primary-identity marker (and to satisfy the oidc_user_has_external_id
-- CHECK); its legacy UNIQUE constraint could block a future change that writes
-- external_id on link (e.g. promoting a linked identity to primary).
--
-- Lookups stay indexed via the existing non-unique idx_users_external_id.

ALTER TABLE users DROP CONSTRAINT users_external_id_key;
