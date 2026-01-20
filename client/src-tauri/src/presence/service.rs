//! Background presence polling service.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tokio::time::interval;

use super::ProcessScanner;

/// Whether the service is running.
static RUNNING: AtomicBool = AtomicBool::new(false);

/// Whether presence sharing is enabled.
static ENABLED: AtomicBool = AtomicBool::new(true);

/// Start background presence polling.
pub fn start_presence_service(app: AppHandle) {
    if RUNNING.swap(true, Ordering::SeqCst) {
        return; // Already running
    }

    tauri::async_runtime::spawn(async move {
        let mut scanner = ProcessScanner::new();
        let mut last_activity: Option<String> = None;
        let mut ticker = interval(Duration::from_secs(15));

        loop {
            ticker.tick().await;

            if !RUNNING.load(Ordering::SeqCst) {
                break;
            }

            // Skip if disabled
            if !ENABLED.load(Ordering::SeqCst) {
                if last_activity.is_some() {
                    // Clear activity when disabled
                    let _ = app.emit("presence:activity_changed", None::<serde_json::Value>);
                    last_activity = None;
                }
                continue;
            }

            let current = scanner.scan().map(|g| g.name.clone());

            // Only emit if activity changed
            if current != last_activity {
                let payload = current.as_ref().map(|name| {
                    serde_json::json!({
                        "name": name,
                        "type": "game",
                        "started_at": chrono::Utc::now().to_rfc3339()
                    })
                });

                let _ = app.emit("presence:activity_changed", payload);
                last_activity = current;
            }
        }
    });
}

/// Stop background presence polling.
pub fn stop_presence_service() {
    RUNNING.store(false, Ordering::SeqCst);
}

/// Enable or disable presence sharing.
pub fn set_presence_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::SeqCst);
}

/// Check if presence sharing is enabled.
pub fn is_presence_enabled() -> bool {
    ENABLED.load(Ordering::SeqCst)
}
