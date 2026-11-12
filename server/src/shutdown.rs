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

/// Upper bound for the HTTP drain after the signal.
pub const HTTP_DRAIN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Upper bound for closing the open web sockets after the signal (voice
/// leave, close frames 1012, per-socket cleanup).
pub const SOCKET_DRAIN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// What the shutdown needs besides the router (which owns the app state).
pub struct SocketDrain {
    pub ws_connections: std::sync::Arc<crate::ws::connection_limit::WsConnectionTracker>,
    pub sfu: std::sync::Arc<crate::voice::SfuServer>,
    pub db: sqlx::PgPool,
    pub screen_share_limiter: Option<crate::voice::ScreenShareLimiter>,
}

impl SocketDrain {
    /// From the app state, before it moves into the router.
    pub fn from_state(state: &crate::api::AppState) -> Self {
        Self {
            ws_connections: state.ws_connections.clone(),
            sfu: state.sfu.clone(),
            db: state.db.clone(),
            screen_share_limiter: state.screen_share_limiter.clone(),
        }
    }

    /// Remove every voice peer through the normal leave path (the others hear
    /// `voice_user_left` while their sockets are still open), then tell every
    /// socket to close with 1012 and run its own cleanup, and wait for them —
    /// all within [`SOCKET_DRAIN_TIMEOUT`].
    async fn run(&self) {
        let drain = async {
            let left = crate::voice::leave::leave_all(
                &self.sfu,
                &self.db,
                self.screen_share_limiter.as_ref(),
            )
            .await;
            self.ws_connections.begin_shutdown();
            let open = self.ws_connections.wait_closed(SOCKET_DRAIN_TIMEOUT).await;
            (left, open)
        };
        match tokio::time::timeout(SOCKET_DRAIN_TIMEOUT, drain).await {
            Ok((left, 0)) => info!(voice_peers = left, "WebSockets closed"),
            Ok((left, open)) => tracing::warn!(
                voice_peers = left,
                open,
                "WebSockets still open after the shutdown bound"
            ),
            Err(_) => {
                self.ws_connections.begin_shutdown();
                tracing::warn!("WebSocket shutdown did not finish in time");
            }
        }
    }
}

/// Serve `app` on `listener` until `signal` resolves, then shut down.
///
/// The bound counts from the signal: the HTTP drain (at most
/// [`HTTP_DRAIN_TIMEOUT`]; requests still running are dropped) and the web
/// socket drain ([`SocketDrain`]) run **at the same time**, so a slow HTTP
/// request never delays the sockets' close frames.
pub async fn serve(
    listener: tokio::net::TcpListener,
    app: axum::Router,
    drain: SocketDrain,
    signal: impl std::future::Future<Output = ()> + Send,
) -> std::io::Result<()> {
    use std::future::IntoFuture;

    let stop = tokio_util::sync::CancellationToken::new();
    let server = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(stop.clone().cancelled_owned())
    .into_future();
    let mut server = std::pin::pin!(server);
    tokio::select! {
        served = &mut server => return served,
        () = signal => {}
    }
    stop.cancel();
    let http = async {
        tokio::time::timeout(HTTP_DRAIN_TIMEOUT, &mut server)
            .await
            .unwrap_or_else(|_| {
                tracing::warn!("HTTP requests still running after the shutdown bound; dropped");
                Ok(())
            })
    };
    let (served, ()) = tokio::join!(http, drain.run());
    info!("HTTP server stopped");
    served
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
