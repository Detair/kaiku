// TEST-ONLY RSA key for the FCM transport tests. DO NOT USE IN PRODUCTION.
//
// This is not a Google service-account key and grants access to nothing. It
// only lets the tests sign RS256 OAuth2 assertions against the local FCM mock
// (the shared JWT test key in config.rs is Ed25519, which RS256 cannot use).
// Generated with: openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048
//
// Kept as a .rs file under server/tests/ so the gitleaks allowlist for test
// fixtures applies. Included by src/push/fcm.rs (unit tests) and
// tests/integration/helpers/mod.rs.

/// PKCS#8 PEM of the test-only RSA key.
#[allow(dead_code)]
pub const TEST_FCM_RSA_PRIVATE_KEY_PEM: &str = "\
-----BEGIN PRIVATE KEY-----\n\
MIIEvAIBADANBgkqhkiG9w0BAQEFAASCBKYwggSiAgEAAoIBAQC+tC9PRXX1utMD\n\
WnentG/PjPVoQgjiAhDYLmEHr8tLyU7Pgcgvnewf02DdrxJ90E0E7Sde/QvNTbB9\n\
5wsTqPqTXtY3hreX2dPDKx/tVn3ZouLZiOz+eewfJ49RbttfwjDGtnWgl8FzatHN\n\
wLlCD7eIC9W1ESl2HVyK350o67IfwY2A1whh2F3Bt+mljv8qd4Mf3cClBc8y1Zf8\n\
8ZNC7tvQ00GbflHzKm15O2b0zFCI9MHS+5TdviMGhxW52L61ADPUSUvJqJHKVytu\n\
A09FLynGAvmeW+YCUfBiiQzoKzCTFceytuP0z6GZfQQMdB2jX2I9YEvHa+FbQaTW\n\
omD3E72xAgMBAAECggEAI6B+7CTSZj9qV4iJrRuA/fncCQe5KBvxsKlteCWn6bco\n\
kYyx7O9oYwO254NjBX0qPl7dyZmloEvAyqykvZ4GfgysIYB6ARTxQldmpgzQAx4y\n\
mihB+kKl0aYu2KxIsImTWjRRIljni5vrqTA1i/lqtLBdUu/rWoUeulUPQ8tQoizQ\n\
lBuoJDaPDV5pTD5i6B5e4vEPHijqZPj997IgOos0qvtKOVVD13qQfCItNC9nUmGl\n\
72+7BtD5L1MXOso06z99yUID4osgmHTzHN/RwfgJJGm3PTdOPOdnFtrsuJMpaYld\n\
scTlEo/qBInKV7zorGsOHStPuxrW8UJPnu0pJPLNvwKBgQDmpNt17spskrzslDa5\n\
/vBXnCnfI5hREPDgfsnkItGnJmp4qM2p5mptgY6RZFLRgnDwc0b6ZU+MtBsSrdL+\n\
47AW0Td2fqo5Lt7hZO6/mFGXJc4KBTUcqk1pCBlMqzSlCSx5FF90nWXwt8r2NMi9\n\
LNJ2z35SuCqyYW8lJwKAitJ0dwKBgQDTq0UoARCHgA5YyuNNQwDJkeaq8FbOJKqP\n\
On3KBXoxvrPSwHTPotD1N1GkjnG1GcsvLjiCScNSNFlUGnjRYdaUKDKlnnuuut0n\n\
LF2it6YBLvoNh+6VfCfy5MKUnzI9nfJl0HVr/Igst2viwj4rsTdr/osmbxmqFhix\n\
Zx3SI8OxFwKBgHJzcjfGZAqWkktvjOylFozTAHArEKWBm5Y7NGIR6SlhxcnUMiJS\n\
JZPri1yYhQRUD0kyWxFb/qqaoSiWuzIiuDnKXhRd7IhtusnsNRE8O5ieeOy+AsoT\n\
tFAfZbaG6pvOvpPiyS9r/68A9SVRsh81w8aTVmMj4oQcF3Q/NF2nBaInAoGADrLb\n\
B48jXbV3yZS1Oh1IBbdbBE3SSVhke7O9HnVI6gFrLVxuACk3R4AC7GFz5faXlHwq\n\
Mmn65U97UIrSDwAzTXCJK9UYk6aoStsa2XzVS0m+1Ye1+q13fLeEB5wSehLozUcv\n\
kEMmFazbEhEZDiGtTpOLiCy45IXKSy90863GHhsCgYBOnrwIQRGggYOEb1SIcf3X\n\
Q7CBIl/5U/IlEtp6mriyd4bWsGyb5Hq0Jq+CpwoEcdje5GSSZHk0z36by0BoIivp\n\
ikIzSl/Iw4QcAupZh/7zxtNvaF2aYHr5HZHjBdVM9sNR/F4LU59HvPDuMnRQN3kU\n\
iVWmQGSqgHRKcG7voJKQ+Q==\n\
-----END PRIVATE KEY-----\n\
";

/// SPKI PEM public half of the test-only RSA key (lets the mock verify the
/// RS256 assertion).
#[allow(dead_code)]
pub const TEST_FCM_RSA_PUBLIC_KEY_PEM: &str = "\
-----BEGIN PUBLIC KEY-----\n\
MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAvrQvT0V19brTA1p3p7Rv\n\
z4z1aEII4gIQ2C5hB6/LS8lOz4HIL53sH9Ng3a8SfdBNBO0nXv0LzU2wfecLE6j6\n\
k17WN4a3l9nTwysf7VZ92aLi2Yjs/nnsHyePUW7bX8IwxrZ1oJfBc2rRzcC5Qg+3\n\
iAvVtREpdh1cit+dKOuyH8GNgNcIYdhdwbfppY7/KneDH93ApQXPMtWX/PGTQu7b\n\
0NNBm35R8ypteTtm9MxQiPTB0vuU3b4jBocVudi+tQAz1ElLyaiRylcrbgNPRS8p\n\
xgL5nlvmAlHwYokM6CswkxXHsrbj9M+hmX0EDHQdo19iPWBLx2vhW0Gk1qJg9xO9\n\
sQIDAQAB\n\
-----END PUBLIC KEY-----\n\
";
