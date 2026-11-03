-- v1.0 login (L1a, Task 7): refresh reuse detection by an authenticated
-- format (DC2). The refresh JWT carries `sid` and `gen`; the row keeps the
-- current generation and what is needed to re-sign the identical successor
-- for a lost response (Ed25519 is deterministic).

ALTER TABLE sessions
    ADD COLUMN refresh_generation BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN refresh_jti UUID,
    ADD COLUMN refresh_issued_at TIMESTAMPTZ,
    ADD COLUMN refresh_retry_at TIMESTAMPTZ;
