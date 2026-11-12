//! Process shutdown: the signal, and what happens after it.
//!
//! The server stops gracefully on Ctrl-C (SIGINT) and, on unix, on SIGTERM,
//! which container runtimes and Kubernetes send to stop a pod.
//!
//! # Bounds
//!
//! [`serve`] returns at most [`SHUTDOWN_DEADLINE`] (5 s) after the signal;
//! telling the web sockets is its first action. Request handlers still
//! running shortly before the deadline are cancelled, connections that have
//! not ended by then are not waited for: they end with the runtime when
//! `main` returns. What `main` does afterwards is bounded as well (see
//! there): background tasks 1 s, telemetry exporters 5 s each (only with
//! OTLP export enabled and a collector that does not answer), telemetry
//! ingestion 2 s, database pool 1 s.

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

/// The whole graceful shutdown, measured from the signal: after it,
/// [`serve`] returns whatever is still running.
pub const SHUTDOWN_DEADLINE: std::time::Duration = std::time::Duration::from_secs(5);

/// How long before the deadline the request handlers that are still running
/// are cancelled, so their connections can answer and close within it.
pub const REQUEST_CANCEL_LEAD: std::time::Duration = std::time::Duration::from_millis(250);

/// What the shutdown needs besides the router (which owns the app state).
pub struct SocketDrain {
    pub ws_connections: std::sync::Arc<crate::ws::connection_limit::WsConnectionTracker>,
    pub sfu: std::sync::Arc<crate::voice::SfuServer>,
    pub db: sqlx::PgPool,
    pub screen_share_limiter: Option<crate::voice::ScreenShareLimiter>,
    /// The object clean-up worker, if one runs: stopped with the rest.
    pub storage_cleanup: Option<crate::chat::storage_cleanup::CleanupWorker>,
}

impl SocketDrain {
    /// From the app state, before it moves into the router.
    pub fn from_state(state: &crate::api::AppState) -> Self {
        Self {
            ws_connections: state.ws_connections.clone(),
            sfu: state.sfu.clone(),
            db: state.db.clone(),
            screen_share_limiter: state.screen_share_limiter.clone(),
            storage_cleanup: None,
        }
    }

    /// Remove every voice peer through the normal leave path (sessions
    /// finalized, DM calls ended, share slots released). Each closing socket
    /// removes its own peer too; this covers peers without a live socket.
    async fn leave_voice(&self) -> usize {
        crate::voice::leave::leave_all(&self.sfu, &self.db, self.screen_share_limiter.as_ref())
            .await
    }

    /// Wait until every socket has finished its cleanup, at most until
    /// `deadline`. Returns how many are still open.
    async fn wait_sockets(&self, deadline: tokio::time::Instant) -> usize {
        self.ws_connections
            .wait_closed(deadline.saturating_duration_since(tokio::time::Instant::now()))
            .await
    }
}

/// Answers 503 for a request whose handler is still running when `cancel`
/// fires; the handler's future is dropped (cancelled) at that moment.
async fn cancel_with_shutdown(
    axum::extract::State(cancel): axum::extract::State<tokio_util::sync::CancellationToken>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    tokio::select! {
        biased;
        response = next.run(request) => response,
        () = cancel.cancelled() => {
            axum::response::IntoResponse::into_response(crate::auth::AuthError::ServiceUnavailable)
        }
    }
}

/// Serve `app` on `listener` until `signal` resolves, then shut down within
/// [`SHUTDOWN_DEADLINE`], measured from the signal.
///
/// 1. **First**, with nothing awaited before it: every web socket task is told to close
///    (`WsConnectionTracker::begin_shutdown`; each sends Close 1012 and runs its normal cleanup,
///    which also removes its voice peer), and the listener stops accepting.
/// 2. **Concurrently**, under the one deadline: the voice rooms are emptied
///    ([`SocketDrain::leave_voice`]), the sockets finish, HTTP requests in flight complete, and the
///    object clean-up worker stops after the store call it is in (keys it did not reach stay
///    queued). None of them waits for another: a stalled voice departure, a slow request or a slow
///    object store delays no close frame.
/// 3. [`REQUEST_CANCEL_LEAD`] before the deadline, request handlers that are still running are
///    **cancelled**: their futures are dropped and the client gets 503. (A response body that is
///    still streaming is not a handler; its connection ends with the process.)
/// 4. At the deadline this returns. Nothing after it waits for a connection or a handler: tasks
///    that did not finish end with the runtime when `main` returns (see the bounded exit path
///    there).
pub async fn serve(
    listener: tokio::net::TcpListener,
    app: axum::Router,
    mut drain: SocketDrain,
    signal: impl std::future::Future<Output = ()> + Send,
) -> std::io::Result<()> {
    use std::future::IntoFuture;

    let stop = tokio_util::sync::CancellationToken::new();
    let cancel_requests = tokio_util::sync::CancellationToken::new();
    let app = app.layer(axum::middleware::from_fn_with_state(
        cancel_requests.clone(),
        cancel_with_shutdown,
    ));
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

    // Nothing is awaited between the signal and these two (both synchronous).
    let deadline = tokio::time::Instant::now() + SHUTDOWN_DEADLINE;
    drain.ws_connections.begin_shutdown();
    stop.cancel();

    let http = async {
        let drained = tokio::time::timeout_at(deadline - REQUEST_CANCEL_LEAD, &mut server).await;
        if let Ok(served) = drained {
            served
        } else {
            tracing::warn!("HTTP requests still running near the shutdown bound; cancelled");
            cancel_requests.cancel();
            (&mut server).await
        }
    };
    // Not finished at the deadline: the handle is dropped, the worker ends
    // after its current store call on its own, and nothing is lost (the keys
    // it claimed come back when their lease runs out).
    let storage_cleanup = drain.storage_cleanup.take();
    let stop_cleanup = async {
        if let Some(worker) = storage_cleanup {
            worker.shutdown().await;
        }
    };
    let all = async {
        tokio::join!(
            http,
            drain.leave_voice(),
            drain.wait_sockets(deadline),
            stop_cleanup
        )
    };
    let served = match tokio::time::timeout_at(deadline, all).await {
        Ok((served, left, 0, ())) => {
            info!(voice_peers = left, "WebSockets closed");
            served
        }
        Ok((served, left, open, ())) => {
            tracing::warn!(
                voice_peers = left,
                open,
                "WebSockets still open at the shutdown bound"
            );
            served
        }
        Err(_) => {
            // Also when the lead was not enough for the connections to end.
            cancel_requests.cancel();
            tracing::warn!(
                open = drain.ws_connections.open_sockets(),
                "Shutdown did not finish within its bound; proceeding"
            );
            Ok(())
        }
    };
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
