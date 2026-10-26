-- "Appear offline" (invisible) is no longer available: presence shows
-- `offline` only while none of the user's devices is connected. Make every
-- user visible again. The `users.invisible` column is kept but deprecated
-- (no longer read or written).
UPDATE users SET invisible = false WHERE invisible;

COMMENT ON COLUMN users.invisible IS 'Deprecated: "appear offline" was removed; always false.';
