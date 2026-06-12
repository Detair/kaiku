//! Server-side performance budgets (Phase 8, Goal 5 item 2).
//!
//! Release-blocking latency budgets for the hottest read paths, designed to
//! catch order-of-magnitude regressions (N+1 queries, lost indexes, full
//! scans) — not micro-benchmarks. Budgets are deliberately generous for
//! noisy shared CI runners and use best-of-3 timing; typical local times
//! are 10–50x under budget.
//!
//! Frontend budgets (initial bundle size as a startup proxy) live in
//! `scripts/check_bundle_budget.py`. Budgets that still require manual or
//! instrumented measurement (true client startup, voice join end-to-end,
//! client memory) are listed in
//! `docs/developer-guide/development/performance-budgets.md`.

use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::{Method, StatusCode};
use sqlx::PgPool;
use uuid::Uuid;

use super::helpers::{
    create_channel, create_guild, create_test_user, generate_access_token, TestApp,
};

/// Seed `count` messages into a channel with one multi-row insert per 500.
async fn seed_messages(pool: &PgPool, channel_id: Uuid, user_id: Uuid, count: usize) {
    let mut remaining = count;
    let mut batch_start = 0usize;
    while remaining > 0 {
        let batch = remaining.min(500);
        let mut qb = sqlx::QueryBuilder::new(
            "INSERT INTO messages (id, channel_id, user_id, content, created_at) ",
        );
        qb.push_values(0..batch, |mut b, i| {
            let n = batch_start + i;
            b.push_bind(Uuid::now_v7())
                .push_bind(channel_id)
                .push_bind(user_id)
                .push_bind(format!("seeded message number {n}"))
                .push("NOW() - make_interval(secs => ")
                .push_bind_unseparated(((count - n) as f64) * 0.1)
                .push_unseparated(")");
        });
        qb.build().execute(pool).await.expect("seed batch failed");
        batch_start += batch;
        remaining -= batch;
    }
}

/// Run `req_fn` three times, returning the fastest wall-clock duration.
/// Best-of-N filters out CI scheduler noise; a genuine regression slows
/// every attempt.
async fn best_of_3<F, Fut>(mut req_fn: F) -> Duration
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Duration>,
{
    let mut best = Duration::MAX;
    for _ in 0..3 {
        let d = req_fn().await;
        if d < best {
            best = d;
        }
    }
    best
}

/// Budget: fetching a 50-message page from a 1,000-message channel must
/// complete well under 500ms (typical: single-digit ms). Catches lost
/// indexes and N+1 author/attachment loading.
#[sqlx::test]
async fn budget_message_history_page_under_500ms(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (owner, _) = create_test_user(&pool).await;
    let guild = create_guild(&pool, owner).await;
    let channel = create_channel(&pool, guild, "perf-history").await;
    seed_messages(&pool, channel, owner, 1_000).await;
    let token = generate_access_token(&app.config, owner);

    let best = best_of_3(|| {
        let app = &app;
        let token = token.clone();
        async move {
            let start = Instant::now();
            let resp = app
                .oneshot(
                    TestApp::request(
                        Method::GET,
                        &format!("/api/messages/channel/{channel}?limit=50"),
                    )
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
                )
                .await;
            let elapsed = start.elapsed();
            assert_eq!(resp.status(), StatusCode::OK);
            elapsed
        }
    })
    .await;

    assert!(
        best < Duration::from_millis(500),
        "message history page took {best:?} (budget 500ms) — check for lost \
         indexes or N+1 loading on the messages read path"
    );
}

/// Budget: guild full-text search over 1,000 messages must complete well
/// under 750ms. Catches regressions in the tsvector index usage.
#[sqlx::test]
async fn budget_guild_search_under_750ms(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (owner, _) = create_test_user(&pool).await;
    let guild = create_guild(&pool, owner).await;
    let channel = create_channel(&pool, guild, "perf-search").await;
    seed_messages(&pool, channel, owner, 1_000).await;
    let token = generate_access_token(&app.config, owner);

    let best = best_of_3(|| {
        let app = &app;
        let token = token.clone();
        async move {
            let start = Instant::now();
            let resp = app
                .oneshot(
                    TestApp::request(Method::GET, &format!("/api/guilds/{guild}/search?q=seeded"))
                        .header("Authorization", format!("Bearer {token}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await;
            let elapsed = start.elapsed();
            assert_eq!(resp.status(), StatusCode::OK);
            elapsed
        }
    })
    .await;

    assert!(
        best < Duration::from_millis(750),
        "guild search took {best:?} (budget 750ms) — check tsvector index \
         usage on the search path"
    );
}
