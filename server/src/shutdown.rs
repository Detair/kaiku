//! Process shutdown signal.
//!
//! The server stops gracefully on Ctrl-C (SIGINT) and, on unix, on SIGTERM,
//! which container runtimes and Kubernetes send to stop a pod.

use tracing::info;

/// Resolves once the process is asked to stop: Ctrl-C, or SIGTERM on unix.
///
/// Logs "Received shutdown signal, initiating graceful shutdown..." when it
/// resolves. If a signal handler cannot be installed, that signal is ignored
/// and a warning is logged; the other one still works.
pub async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(e) = tokio::signal::ctrl_c().await {
            tracing::warn!(error = %e, "Failed to install Ctrl-C handler");
            std::future::pending::<()>().await;
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut sigterm) => {
                sigterm.recv().await;
            }
            Err(e) => {
                tracing::warn!(error = %e, "Failed to install SIGTERM handler");
                std::future::pending::<()>().await;
            }
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {}
        () = terminate => {}
    }
    info!("Received shutdown signal, initiating graceful shutdown...");
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[tokio::test]
    async fn shutdown_signal_is_pending_without_signal() {
        let waited = tokio::time::timeout(Duration::from_millis(50), shutdown_signal()).await;
        assert!(waited.is_err(), "resolved without a signal");
    }
}
