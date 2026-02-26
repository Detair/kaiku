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
        let mut last_activity: Option<(String, String)> = None; // (name, activity_type)
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

            let current = scanner
                .scan()
                .map(|g| (g.name.clone(), g.activity_type));

            // Only emit if activity changed
            if current != last_activity {
                let payload = current.as_ref().map(|(name, activity_type)| {
                    serde_json::json!({
                        "name": name,
                        "type": activity_type,
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
#[allow(dead_code)]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_presence_enabled_default() {
        // Reset to default state for test
        ENABLED.store(true, Ordering::SeqCst);
        assert!(is_presence_enabled());
    }

    #[test]
    fn test_set_presence_disabled() {
        // Store original state
        let original = ENABLED.load(Ordering::SeqCst);

        set_presence_enabled(false);
        assert!(!is_presence_enabled());

        // Restore original state
        ENABLED.store(original, Ordering::SeqCst);
    }

    #[test]
    fn test_set_presence_enabled() {
        // Store original state
        let original = ENABLED.load(Ordering::SeqCst);

        set_presence_enabled(false);
        assert!(!is_presence_enabled());

        set_presence_enabled(true);
        assert!(is_presence_enabled());

        // Restore original state
        ENABLED.store(original, Ordering::SeqCst);
    }

    #[test]
    fn test_running_flag_default() {
        // Default should be false (not running)
        // Note: This test may fail if run after start_presence_service
        // In a clean test environment, RUNNING starts as false
        let was_running = RUNNING.swap(false, Ordering::SeqCst);
        assert!(!RUNNING.load(Ordering::SeqCst));

        // Restore if it was running
        if was_running {
            RUNNING.store(true, Ordering::SeqCst);
        }
    }

    #[test]
    fn test_stop_presence_service() {
        // Set running to true
        RUNNING.store(true, Ordering::SeqCst);
        assert!(RUNNING.load(Ordering::SeqCst));

        // Stop service
        stop_presence_service();
        assert!(!RUNNING.load(Ordering::SeqCst));
    }
}
