<!-- Parent: ../AGENTS.md -->
# Server Integration Tests

## Purpose

Integration tests for the Kaiku server. Exercises HTTP handlers through
the full axum router, WebSocket flows, and cross-service interactions
against real PostgreSQL and Redis instances.

## For AI Agents

### Test environment setup

Tests require the dev infrastructure to be running:

```bash
# Start PostgreSQL, Valkey, RustFS (S3)
podman compose -f docker-compose.dev.yml --profile storage up -d

# Run the integration suite
SQLX_OFFLINE=true cargo test -p vc-server --tests

# Or a single test file
cargo test -p vc-server --test integration setup_http
```

`DATABASE_URL` must point at the dev Postgres (`postgresql://voicechat:voicechat_dev@localhost:5433/voicechat`).
`cargo sqlx prepare` populates `.sqlx/` so `SQLX_OFFLINE=true` works in CI.

### Integration test pattern

Each integration test gets its own isolated PostgreSQL database via
`#[sqlx::test]`:

```rust
use sqlx::PgPool;

use crate::integration::helpers::{create_test_user, generate_access_token, TestApp};

#[sqlx::test]
async fn test_something(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user_id, _) = create_test_user(&pool).await;
    let token = generate_access_token(&app.config, user_id);

    let response = app
        .oneshot(
            TestApp::request(Method::GET, "/api/users/me")
                .header("Authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;

    assert_eq!(response.status(), 200);
}
```

Under the hood, `#[sqlx::test]` clones a template DB that already has
all migrations applied, hands the test a fresh `PgPool` bound to that
clone, and drops the clone after the test returns. There is **no shared
state between tests**.

### Factory choice

| Helper | When to use |
|---|---|
| `TestApp::with_pool(pool)` | Most tests — plain axum router against the injected pool. |
| `TestApp::with_pool_and_screen_share_limiter(pool)` | Tests that hit `/screenshare/{check,start,stop}` — handlers need a `ScreenShareLimiter` wired against Redis, otherwise they return 500. |
| `TestApp::with_pool_and_config(pool, config)` | Tests that need a non-default `Config` (e.g., custom rate limits, specific JWT expiries). |
| `fresh_test_app_with_s3_and_pool(pool)` | Tests that exercise the file-upload handlers; returns `(TestApp, bucket_name)` with a unique RustFS bucket. Requires `canis-dev-rustfs` on `localhost:9000` and the `AWS_*` env vars. |

### Rules

- Do **not** use `#[tokio::test]` for DB-using integration tests. The
  handful of files that keep `#[tokio::test]` (`voice_rate_limit.rs`,
  `voice_sfu.rs`, `voice_mute_enforcement.rs`, `screenshare.rs` pure-SFU
  tests, `oidc.rs` pure in-memory tests, `ratelimit.rs` pure Redis
  tests) are carve-outs that touch no DB and would waste a per-test DB.
- Do **not** use `#[serial]` — per-test DB isolation is absolute, and
  the `serial_test` dev-dep has been removed.
- Do **not** reintroduce a shared `SHARED_POOL` / `shared_pool()` /
  `shared_config()` helper. The `#[sqlx::test]` model replaces both.
- Do **not** delete rows manually at test end — the test's DB is
  dropped anyway.

### SQL fixtures

`#[sqlx::test(fixtures("users", "channels"))]` loads seed files from
`server/tests/fixtures/`. Most tests build their state inline via
helpers (`create_test_user`, `create_guild`, `insert_message`, …) and
don't need fixture files.

## Dependencies

- PostgreSQL (`canis-dev-postgres`, port 5433)
- Valkey / Redis (`canis-dev-valkey`, port 6379)
- RustFS (`canis-dev-rustfs`, port 9000) — only for S3-enabled tests
- SQLx's `#[sqlx::test]` macro
