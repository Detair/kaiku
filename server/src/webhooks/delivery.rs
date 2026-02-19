//! Webhook Delivery Worker
//!
//! Background worker that processes webhook deliveries from a Redis queue
//! with exponential backoff retries and dead-letter handling.
//!
//! Architecture:
//! - New deliveries go into `DELIVERY_QUEUE_KEY` (list, BRPOP).
//! - Failed deliveries are scheduled into `RETRY_ZSET_KEY` (sorted set, score = Unix timestamp).
//! - The worker loop polls both: immediate queue and due retries.

use std::time::Duration;

use fred::interfaces::{ListInterface, LuaInterface, SortedSetsInterface};
use fred::prelude::*;
use sqlx::PgPool;
use tracing::{error, info, warn};

use super::queries;
use super::signing;
use super::ssrf;
use super::types::WebhookDeliveryItem;

/// Redis key for the immediate webhook delivery queue.
const DELIVERY_QUEUE_KEY: &str = "webhook:delivery:queue";

/// Redis key for the delayed retry sorted set (score = Unix timestamp when due).
const RETRY_ZSET_KEY: &str = "webhook:delivery:retry";

/// Maximum retry attempts before dead-lettering.
const MAX_ATTEMPTS: u32 = 5;

/// Retry delays in seconds (exponential backoff).
const RETRY_DELAYS_SECS: [u64; 5] = [5, 30, 120, 600, 1800];

// H4: Compile-time assertion that RETRY_DELAYS_SECS covers all attempts
const _: () = assert!(MAX_ATTEMPTS as usize <= RETRY_DELAYS_SECS.len());

/// Lua script that atomically removes and returns due items from the retry sorted set.
/// This prevents the race condition where concurrent workers could double-deliver.
const PROMOTE_RETRIES_LUA: &str = r"
local items = redis.call('ZRANGEBYSCORE', KEYS[1], '-inf', ARGV[1], 'LIMIT', 0, 50)
if #items > 0 then
    redis.call('ZREM', KEYS[1], unpack(items))
end
return items
";

/// Enqueue a delivery item for immediate processing.
pub async fn enqueue(redis: &Client, item: &WebhookDeliveryItem) -> Result<(), Error> {
    let payload = serde_json::to_string(item)
        .map_err(|e| Error::new(ErrorKind::Parse, format!("JSON serialize error: {e}")))?;

    redis.lpush::<(), _, _>(DELIVERY_QUEUE_KEY, payload).await?;
    Ok(())
}

/// Schedule a delivery item for retry at a future timestamp.
async fn schedule_retry(
    redis: &Client,
    item: &WebhookDeliveryItem,
    deliver_at: f64,
) -> Result<(), Error> {
    let payload = serde_json::to_string(item)
        .map_err(|e| Error::new(ErrorKind::Parse, format!("JSON serialize error: {e}")))?;

    redis
        .zadd::<(), _, _>(
            RETRY_ZSET_KEY,
            None,
            None,
            false,
            false,
            (deliver_at, payload),
        )
        .await?;
    Ok(())
}

/// Move due retries from the sorted set into the immediate queue (atomic via Lua).
async fn promote_due_retries(redis: &Client) {
    let now = chrono::Utc::now().timestamp() as f64;

    // C1: Atomic fetch-and-remove via Lua script to prevent duplicate deliveries
    let items: Vec<String> = match redis
        .eval(
            PROMOTE_RETRIES_LUA,
            vec![RETRY_ZSET_KEY],
            vec![now.to_string()],
        )
        .await
    {
        Ok(items) => items,
        Err(e) => {
            error!("Failed to promote due retries (Lua): {}", e);
            return;
        }
    };

    if items.is_empty() {
        return;
    }

    // Push atomically-removed items into the immediate queue
    for payload in &items {
        if let Err(e) = redis
            .lpush::<(), _, _>(DELIVERY_QUEUE_KEY, payload.as_str())
            .await
        {
            error!("Failed to re-enqueue promoted retry item: {}", e);
        }
    }
}

/// Spawn the background delivery worker.
pub async fn spawn_delivery_worker(db: PgPool, redis: Client, http_client: reqwest::Client) {
    info!("Webhook delivery worker started");

    // H7: Track consecutive BRPOP errors for exponential backoff
    let mut consecutive_errors: u32 = 0;

    loop {
        // Promote any due retries into the immediate queue
        promote_due_retries(&redis).await;

        // BRPOP with 2-second timeout (short so we check retries frequently)
        let result: Result<Option<(String, String)>, _> =
            redis.brpop(DELIVERY_QUEUE_KEY, 2.0).await;

        let payload_str = match result {
            Ok(Some((_key, value))) => {
                consecutive_errors = 0;
                value
            }
            Ok(None) => {
                consecutive_errors = 0;
                continue; // Timeout, no items
            }
            Err(e) => {
                consecutive_errors += 1;
                let backoff_secs = 1u64 << consecutive_errors.min(6); // 2, 4, 8, ... 64
                if backoff_secs > 30 {
                    error!(
                        consecutive_errors,
                        backoff_secs,
                        "Persistent Redis failure in delivery worker, backing off: {}",
                        e
                    );
                } else {
                    error!("Failed to BRPOP from delivery queue: {}", e);
                }
                tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
                continue;
            }
        };

        // H2: Log truncated payload on deserialization failure for debugging
        let item: WebhookDeliveryItem = match serde_json::from_str(&payload_str) {
            Ok(item) => item,
            Err(e) => {
                let truncated: String = payload_str.chars().take(500).collect();
                error!(
                    error = %e,
                    payload_preview = %truncated,
                    "Failed to deserialize delivery item"
                );
                continue;
            }
        };

        let db = db.clone();
        let redis = redis.clone();
        let client = http_client.clone();

        // C2: Spawn delivery with panic-catching wrapper
        tokio::spawn(async move {
            let webhook_id = item.webhook_id;
            let event_id = item.event_id;
            let handle = tokio::spawn(async move {
                process_delivery(&db, &redis, &client, item).await;
            });
            if let Err(e) = handle.await {
                error!(
                    webhook_id = %webhook_id,
                    event_id = %event_id,
                    "Delivery task panicked: {}", e
                );
            }
        });
    }
}

/// Process a single webhook delivery.
async fn process_delivery(
    db: &PgPool,
    redis: &Client,
    client: &reqwest::Client,
    item: WebhookDeliveryItem,
) {
    // SSRF protection: verify resolved IP is not private/reserved
    if let Err(e) = ssrf::verify_resolved_ip(&item.url).await {
        warn!(
            webhook_id = %item.webhook_id,
            url = %item.url,
            error = %e,
            "Webhook delivery blocked by SSRF protection"
        );
        if let Err(log_err) = queries::log_delivery(
            db,
            item.webhook_id,
            item.event_type,
            item.event_id,
            None,
            false,
            item.attempt as i32,
            Some(&format!("SSRF blocked: {e}")),
            Some(0),
        )
        .await
        {
            error!("Failed to log SSRF-blocked delivery: {}", log_err);
        }
        // Do NOT retry SSRF-blocked deliveries — the URL itself is the problem
        return;
    }

    // Look up signing secret from database (not stored in Redis queue)
    let signing_secret = match queries::get_signing_secret(db, item.webhook_id).await {
        Ok(Some(secret)) => secret,
        Ok(None) => {
            warn!(webhook_id = %item.webhook_id, "Webhook deleted or deactivated before delivery, skipping");
            return;
        }
        // H1: Treat DB errors as transient failures worth retrying
        Err(e) => {
            error!(webhook_id = %item.webhook_id, error = %e, "Failed to look up signing secret");
            handle_retry(db, redis, item, &format!("DB error: {e}")).await;
            return;
        }
    };

    // Build CloudEvents 1.0 envelope
    let envelope = serde_json::json!({
        "specversion": "1.0",
        "type": item.event_type.as_str(),
        "source": "canis",
        "id": item.event_id.to_string(),
        "time": item.event_time.to_rfc3339(),
        "data": item.payload,
    });

    // H3: Add context to serialization failure log
    let payload_bytes = match serde_json::to_vec(&envelope) {
        Ok(bytes) => bytes,
        Err(e) => {
            error!(
                webhook_id = %item.webhook_id,
                event_id = %item.event_id,
                "Failed to serialize webhook envelope: {}", e
            );
            return;
        }
    };

    let signature = signing::sign_payload(&signing_secret, &payload_bytes);
    let timestamp = chrono::Utc::now().timestamp().to_string();

    let start = std::time::Instant::now();
    let result = client
        .post(&item.url)
        .header("Content-Type", "application/json")
        .header("X-Webhook-Signature", format!("sha256={signature}"))
        .header("X-Webhook-Event", item.event_type.as_str())
        .header("X-Webhook-ID", item.event_id.to_string())
        .header("X-Webhook-Timestamp", &timestamp)
        .body(payload_bytes)
        .send()
        .await;
    let latency_ms = start.elapsed().as_millis() as i32;

    match result {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let success = resp.status().is_success();

            // Log delivery result
            let error_msg = if !success {
                Some(format!("HTTP {status}"))
            } else {
                None
            };
            if let Err(e) = queries::log_delivery(
                db,
                item.webhook_id,
                item.event_type,
                item.event_id,
                Some(status as i16),
                success,
                item.attempt as i32,
                error_msg.as_deref(),
                Some(latency_ms),
            )
            .await
            {
                error!("Failed to log delivery: {}", e);
            }

            if !success {
                handle_retry(db, redis, item, &format!("HTTP {status}")).await;
            }
        }
        Err(e) => {
            let error_msg = e.to_string();
            warn!(
                webhook_id = %item.webhook_id,
                attempt = item.attempt,
                error = %error_msg,
                "Webhook delivery failed"
            );

            // Log failure
            if let Err(log_err) = queries::log_delivery(
                db,
                item.webhook_id,
                item.event_type,
                item.event_id,
                None,
                false,
                item.attempt as i32,
                Some(&error_msg),
                Some(latency_ms),
            )
            .await
            {
                error!("Failed to log delivery failure: {}", log_err);
            }

            handle_retry(db, redis, item, &error_msg).await;
        }
    }
}

/// Handle retry or dead-letter for a failed delivery.
async fn handle_retry(db: &PgPool, redis: &Client, mut item: WebhookDeliveryItem, error: &str) {
    if item.attempt < MAX_ATTEMPTS {
        // H4: Safe index with fallback to max delay
        let delay_secs = RETRY_DELAYS_SECS
            .get(item.attempt as usize)
            .copied()
            .unwrap_or(1800);
        item.attempt += 1;

        // Schedule for future delivery via sorted set (no sleeping tasks)
        let deliver_at = chrono::Utc::now().timestamp() as f64 + delay_secs as f64;

        if let Err(e) = schedule_retry(redis, &item, deliver_at).await {
            // H5: Dead-letter fallback when retry scheduling fails
            error!(
                webhook_id = %item.webhook_id,
                attempt = item.attempt,
                "Failed to schedule retry, falling back to dead-letter: {}", e
            );
            if let Err(dl_err) = queries::insert_dead_letter(
                db,
                item.webhook_id,
                item.event_type,
                item.event_id,
                &item.payload,
                item.attempt as i32,
                Some(&format!("{error} (retry scheduling failed: {e})")),
                item.event_time,
            )
            .await
            {
                error!(
                    "Failed to insert dead letter after retry failure: {}",
                    dl_err
                );
            }
        }
    } else {
        // Dead-letter
        warn!(
            webhook_id = %item.webhook_id,
            event_id = %item.event_id,
            "Webhook delivery exhausted all retries, dead-lettering"
        );

        if let Err(e) = queries::insert_dead_letter(
            db,
            item.webhook_id,
            item.event_type,
            item.event_id,
            &item.payload,
            item.attempt as i32,
            Some(error),
            item.event_time,
        )
        .await
        {
            error!("Failed to insert dead letter: {}", e);
        }
    }
}
