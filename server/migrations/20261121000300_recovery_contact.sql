-- v1.0 login (L1b Task 20): the recovery contact (spec Part C, SM3 rev 5;
-- C28 "Recovery contact").
--
-- Another member, never an admin, named by the account (or by an admin at
-- setup) and confirmed by that member from a full session. A confirmed
-- contact receives the account's high-severity notices and may cancel its
-- account requests; it counts as an independent canceller only for requests
-- that start at least one delay period after the confirmation.

ALTER TABLE users
    ADD COLUMN recovery_contact_id UUID REFERENCES users(id) ON DELETE SET NULL,
    ADD COLUMN recovery_contact_named_at TIMESTAMPTZ,
    ADD COLUMN recovery_contact_confirmed_at TIMESTAMPTZ,
    ADD CONSTRAINT users_recovery_contact_not_self CHECK (recovery_contact_id <> id);
-- A deleted contact clears the link (ON DELETE SET NULL); readers treat the
-- timestamps as meaningless without `recovery_contact_id`.

CREATE INDEX users_recovery_contact ON users (recovery_contact_id)
    WHERE recovery_contact_id IS NOT NULL;
