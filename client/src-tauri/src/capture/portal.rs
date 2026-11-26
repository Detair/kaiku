//! Screen capture on Linux through the xdg-desktop-portal `ScreenCast`
//! interface (Wayland and X11 alike): the system dialog chooses the screen
//! or window, the portal hands back a `PipeWire` remote and node.
//!
//! `scap` cannot be used here: it lists no sources on Linux and its portal
//! code panics on a cancelled dialog or a missing portal (the release build
//! aborts on panic). This module talks to the portal over `zbus` (already a
//! Linux dependency), asynchronously, so a timeout really ends the wait (the
//! D-Bus connection is dropped with it), and returns errors instead.
//!
//! The flow (`run_flow`) is written against [`PortalBus`] so tests can fake
//! the bus: cancel, denial, missing portal, timeout and success are covered
//! without a desktop session.

use std::collections::HashMap;
use std::future::Future;
use std::os::fd::OwnedFd;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use thiserror::Error;
use tracing::info;

/// Why no screen could be opened.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum PortalError {
    /// No portal (or no `ScreenCast` interface, or no session bus).
    #[error("PORTAL_UNAVAILABLE: {0}")]
    Unavailable(String),
    /// The user closed the dialog.
    #[error("PORTAL_CANCELLED")]
    Cancelled,
    /// The portal ended the request itself (response 2: refused by a
    /// policy or closed by the compositor), not the user's cancel.
    #[error("PORTAL_DENIED")]
    Denied,
    #[error("Screen share failed: {0}")]
    Failed(String),
}

impl PortalError {
    /// Error text the frontend maps (`PORTAL_*` codes).
    pub fn code_text(&self) -> String {
        self.to_string()
    }
}

/// A value of a portal response the flow needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortalValue {
    Str(String),
    /// `streams`: (`PipeWire` node id, source type 1 = monitor, 2 = window).
    Streams(Vec<(u32, u32)>),
}

/// A request-style portal call (answered by a `Response` signal).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortalCall {
    CreateSession,
    SelectSources { session: String, cursor_mode: u32 },
    Start { session: String },
}

/// Errors of the bus itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BusError {
    /// The portal service or interface does not exist.
    NoPortal(String),
    Other(String),
}

/// Answer of a request call: response code and results.
pub type PortalAnswer = (u32, HashMap<String, PortalValue>);

/// The portal as the flow sees it.
pub trait PortalBus {
    /// Make a request call and wait for its `Response` (code, results).
    fn request(
        &mut self,
        call: PortalCall,
    ) -> impl Future<Output = Result<PortalAnswer, BusError>> + Send;
    /// `AvailableCursorModes` (bit 1 hidden, 2 embedded, 4 metadata).
    fn cursor_modes(&mut self) -> impl Future<Output = Result<u32, BusError>> + Send;
    /// `OpenPipeWireRemote` for the started session.
    fn open_pipewire_remote(
        &mut self,
        session: &str,
    ) -> impl Future<Output = Result<OwnedFd, BusError>> + Send;
}

/// What a successful flow yields.
#[derive(Debug)]
pub struct FlowResult {
    pub session: String,
    pub fd: OwnedFd,
    pub node_id: u32,
    pub source_type: u32,
}

fn map_bus(e: BusError) -> PortalError {
    match e {
        BusError::NoPortal(m) => PortalError::Unavailable(m),
        BusError::Other(m) => PortalError::Failed(m),
    }
}

/// Response codes: 0 success, 1 cancelled by the user, 2 ended otherwise.
fn check_code(code: u32) -> Result<(), PortalError> {
    match code {
        0 => Ok(()),
        1 => Err(PortalError::Cancelled),
        2 => Err(PortalError::Denied),
        other => Err(PortalError::Failed(format!("portal response {other}"))),
    }
}

/// `CreateSession` → `SelectSources` → Start → `OpenPipeWireRemote`.
pub async fn run_flow<B: PortalBus + Send>(bus: &mut B) -> Result<FlowResult, PortalError> {
    let (code, results) = bus
        .request(PortalCall::CreateSession)
        .await
        .map_err(map_bus)?;
    check_code(code)?;
    let session = match results.get("session_handle") {
        Some(PortalValue::Str(s)) => s.clone(),
        _ => return Err(PortalError::Failed("no session handle".into())),
    };
    // Show the pointer in the picture when the portal can draw it in.
    let cursor_mode = match bus.cursor_modes().await {
        Ok(modes) if modes & 2 != 0 => 2,
        Ok(_) | Err(_) => 1,
    };
    let (code, _) = bus
        .request(PortalCall::SelectSources {
            session: session.clone(),
            cursor_mode,
        })
        .await
        .map_err(map_bus)?;
    check_code(code)?;
    let (code, results) = bus
        .request(PortalCall::Start {
            session: session.clone(),
        })
        .await
        .map_err(map_bus)?;
    check_code(code)?;
    let (node_id, source_type) = match results.get("streams") {
        Some(PortalValue::Streams(streams)) => streams
            .first()
            .copied()
            .ok_or_else(|| PortalError::Failed("no stream chosen".into()))?,
        _ => return Err(PortalError::Failed("no streams in the answer".into())),
    };
    let fd = bus.open_pipewire_remote(&session).await.map_err(map_bus)?;
    Ok(FlowResult {
        session,
        fd,
        node_id,
        source_type,
    })
}

/// Keeps the portal session open (its D-Bus connection) and the task that
/// watches for its end; both go when this is dropped.
pub struct SessionGuard {
    watcher: Option<tokio::task::JoinHandle<()>>,
    _connection: Box<dyn std::any::Any + Send + Sync>,
}

impl Drop for SessionGuard {
    fn drop(&mut self) {
        if let Some(watcher) = self.watcher.take() {
            watcher.abort();
        }
    }
}

/// An open `ScreenCast` session: the `PipeWire` remote and node. Dropping
/// it closes the session. `closed` turns true when the portal ends the
/// session (the system's "stop sharing" control).
pub struct PortalStream {
    pub fd: OwnedFd,
    pub node_id: u32,
    pub source_type: u32,
    pub closed: Arc<AtomicBool>,
    pub guard: Option<SessionGuard>,
}

impl PortalStream {
    /// A stream without a portal session (tests).
    pub fn detached(fd: OwnedFd, node_id: u32, source_type: u32) -> Self {
        Self {
            fd,
            node_id,
            source_type,
            closed: Arc::new(AtomicBool::new(false)),
            guard: None,
        }
    }

    /// Whether the user chose a window (portal source type 2).
    pub const fn is_window(&self) -> bool {
        self.source_type == 2
    }

    /// Label sent to the server as the share's source.
    pub fn label(&self) -> String {
        if self.is_window() { "Window" } else { "Screen" }.to_string()
    }
}

/// How the app opens the portal (a seam: tests pass a fake).
pub trait ScreenCastPortal: Clone + Send + Sync + 'static {
    fn open(&self) -> impl Future<Output = Result<PortalStream, PortalError>> + Send;
}

/// The longest the dialog may stay open.
pub const DIALOG_TIMEOUT: Duration = Duration::from_mins(5);

/// Open a screen through the portal. Past `timeout` the wait is dropped,
/// and with it the D-Bus connection, which ends the portal request: no
/// thread or connection outlives the timeout.
pub async fn open_screencast<P: ScreenCastPortal>(
    portal: &P,
    timeout: Duration,
) -> Result<PortalStream, PortalError> {
    tokio::time::timeout(timeout, portal.open())
        .await
        .unwrap_or_else(|_| {
            Err(PortalError::Failed(
                "the screen dialog did not answer".into(),
            ))
        })
}

/// The real portal on the session bus.
#[derive(Clone, Copy)]
pub struct DbusPortal;

impl ScreenCastPortal for DbusPortal {
    async fn open(&self) -> Result<PortalStream, PortalError> {
        let mut bus = dbus::ZbusPortal::connect().await?;
        let flow = run_flow(&mut bus).await?;
        info!(node = flow.node_id, "Screen cast portal session started");
        let closed = Arc::new(AtomicBool::new(false));
        let conn = bus.into_connection();
        let watcher = tokio::spawn(dbus::watch_closed(
            conn.clone(),
            flow.session.clone(),
            closed.clone(),
        ));
        Ok(PortalStream {
            fd: flow.fd,
            node_id: flow.node_id,
            source_type: flow.source_type,
            closed,
            guard: Some(SessionGuard {
                watcher: Some(watcher),
                _connection: Box::new(conn),
            }),
        })
    }
}

mod dbus {
    use std::collections::HashMap;
    use std::os::fd::OwnedFd;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    use futures::StreamExt;
    use tracing::info;
    use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};
    use zbus::{Connection, Proxy};

    use super::{BusError, PortalAnswer, PortalBus, PortalCall, PortalError, PortalValue};

    const DEST: &str = "org.freedesktop.portal.Desktop";
    const PATH: &str = "/org/freedesktop/portal/desktop";
    const IFACE: &str = "org.freedesktop.portal.ScreenCast";

    pub struct ZbusPortal {
        conn: Connection,
        sender: String,
        counter: u32,
    }

    fn bus_err(e: &zbus::Error) -> BusError {
        let text = e.to_string();
        if text.contains("ServiceUnknown")
            || text.contains("UnknownInterface")
            || text.contains("UnknownMethod")
            || text.contains("NameHasNoOwner")
        {
            BusError::NoPortal(text)
        } else {
            BusError::Other(text)
        }
    }

    /// Set `closed` when the portal ends the session (`Session.Closed`).
    pub async fn watch_closed(conn: Connection, session: String, closed: Arc<AtomicBool>) {
        let Ok(proxy) = Proxy::new(&conn, DEST, session, "org.freedesktop.portal.Session").await
        else {
            return;
        };
        let Ok(mut signals) = proxy.receive_signal("Closed").await else {
            return;
        };
        if signals.next().await.is_some() {
            info!("Screen cast portal session closed");
            closed.store(true, Ordering::Release);
        }
    }

    impl ZbusPortal {
        pub async fn connect() -> Result<Self, PortalError> {
            let conn = Connection::session()
                .await
                .map_err(|e| PortalError::Unavailable(format!("no session bus: {e}")))?;
            let sender = conn
                .unique_name()
                .map(|n| n.as_str().trim_start_matches(':').replace('.', "_"))
                .ok_or_else(|| PortalError::Unavailable("no bus name".into()))?;
            Ok(Self {
                conn,
                sender,
                counter: 0,
            })
        }

        pub fn into_connection(self) -> Connection {
            self.conn
        }

        async fn proxy(&self) -> Result<Proxy<'static>, BusError> {
            Proxy::new(&self.conn, DEST, PATH, IFACE)
                .await
                .map_err(|e| bus_err(&e))
        }

        fn next_token(&mut self) -> String {
            self.counter += 1;
            format!("kaiku{}_{}", std::process::id(), self.counter)
        }
    }

    fn results_of(raw: &HashMap<String, OwnedValue>) -> HashMap<String, PortalValue> {
        let mut out = HashMap::new();
        if let Some(v) = raw.get("session_handle") {
            if let Ok(s) = <&str>::try_from(&**v) {
                out.insert("session_handle".into(), PortalValue::Str(s.to_string()));
            }
        }
        if let Some(v) = raw.get("streams") {
            if let Ok(list) = Vec::<(u32, HashMap<String, OwnedValue>)>::try_from(v.clone()) {
                let streams = list
                    .into_iter()
                    .map(|(node, props)| {
                        let kind = props
                            .get("source_type")
                            .and_then(|t| u32::try_from(&**t).ok())
                            .unwrap_or(1);
                        (node, kind)
                    })
                    .collect();
                out.insert("streams".into(), PortalValue::Streams(streams));
            }
        }
        out
    }

    impl PortalBus for ZbusPortal {
        async fn request(&mut self, call: PortalCall) -> Result<PortalAnswer, BusError> {
            let token = self.next_token();
            // Subscribe to the answer before asking, so it cannot be missed.
            let request_path = format!("{PATH}/request/{}/{token}", self.sender);
            let request = Proxy::new(
                &self.conn,
                DEST,
                request_path,
                "org.freedesktop.portal.Request",
            )
            .await
            .map_err(|e| bus_err(&e))?;
            let mut answers = request
                .receive_signal("Response")
                .await
                .map_err(|e| bus_err(&e))?;

            let proxy = self.proxy().await?;
            let session_token = self.next_token();
            let mut options: HashMap<&str, Value<'_>> = HashMap::new();
            options.insert("handle_token", Value::from(token.as_str()));
            let _: OwnedObjectPath = match &call {
                PortalCall::CreateSession => {
                    options.insert("session_handle_token", Value::from(session_token.as_str()));
                    proxy.call("CreateSession", &(options,)).await
                }
                PortalCall::SelectSources {
                    session,
                    cursor_mode,
                } => {
                    let path = zbus::zvariant::ObjectPath::try_from(session.as_str())
                        .map_err(|e| BusError::Other(e.to_string()))?;
                    options.insert("types", Value::from(1u32 | 2u32));
                    options.insert("multiple", Value::from(false));
                    options.insert("cursor_mode", Value::from(*cursor_mode));
                    proxy.call("SelectSources", &(path, options)).await
                }
                PortalCall::Start { session } => {
                    let path = zbus::zvariant::ObjectPath::try_from(session.as_str())
                        .map_err(|e| BusError::Other(e.to_string()))?;
                    proxy.call("Start", &(path, "", options)).await
                }
            }
            .map_err(|e| bus_err(&e))?;

            let message = answers
                .next()
                .await
                .ok_or_else(|| BusError::Other("no answer from the portal".into()))?;
            let (code, raw): (u32, HashMap<String, OwnedValue>) = message
                .body()
                .deserialize()
                .map_err(|e| BusError::Other(e.to_string()))?;
            Ok((code, results_of(&raw)))
        }

        async fn cursor_modes(&mut self) -> Result<u32, BusError> {
            self.proxy()
                .await?
                .get_property::<u32>("AvailableCursorModes")
                .await
                .map_err(|e| bus_err(&e))
        }

        async fn open_pipewire_remote(&mut self, session: &str) -> Result<OwnedFd, BusError> {
            let path = zbus::zvariant::ObjectPath::try_from(session)
                .map_err(|e| BusError::Other(e.to_string()))?;
            let options: HashMap<&str, Value<'_>> = HashMap::new();
            let fd: zbus::zvariant::OwnedFd = self
                .proxy()
                .await?
                .call("OpenPipeWireRemote", &(path, options))
                .await
                .map_err(|e| bus_err(&e))?;
            Ok(fd.into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Scripted bus: answers each request in order.
    struct FakeBus {
        answers: Vec<Result<PortalAnswer, BusError>>,
        calls: Vec<PortalCall>,
        remote: Option<Result<(), BusError>>,
        cursor: Result<u32, BusError>,
    }

    impl FakeBus {
        fn new(answers: Vec<Result<PortalAnswer, BusError>>) -> Self {
            Self {
                answers,
                calls: vec![],
                remote: None,
                cursor: Ok(1 | 2),
            }
        }
    }

    fn ok(code: u32, results: &[(&str, PortalValue)]) -> Result<PortalAnswer, BusError> {
        Ok((
            code,
            results
                .iter()
                .map(|(k, v)| ((*k).to_string(), v.clone()))
                .collect(),
        ))
    }

    impl PortalBus for FakeBus {
        async fn request(&mut self, call: PortalCall) -> Result<PortalAnswer, BusError> {
            self.calls.push(call);
            self.answers.remove(0)
        }

        async fn cursor_modes(&mut self) -> Result<u32, BusError> {
            self.cursor.clone()
        }

        async fn open_pipewire_remote(&mut self, _session: &str) -> Result<OwnedFd, BusError> {
            match self.remote.take().unwrap_or(Ok(())) {
                Ok(()) => Ok(std::fs::File::open("/dev/null").expect("dev null").into()),
                Err(e) => Err(e),
            }
        }
    }

    fn session() -> (&'static str, PortalValue) {
        ("session_handle", PortalValue::Str("/s/1".into()))
    }

    fn streams(list: Vec<(u32, u32)>) -> (&'static str, PortalValue) {
        ("streams", PortalValue::Streams(list))
    }

    #[tokio::test]
    async fn success_yields_the_node_and_a_remote() {
        let mut bus = FakeBus::new(vec![
            ok(0, &[session()]),
            ok(0, &[]),
            ok(0, &[streams(vec![(42, 2)])]),
        ]);
        let flow = run_flow(&mut bus).await.expect("flow");
        assert_eq!(
            (flow.node_id, flow.source_type, flow.session.as_str()),
            (42, 2, "/s/1")
        );
        assert_eq!(
            bus.calls,
            vec![
                PortalCall::CreateSession,
                PortalCall::SelectSources {
                    session: "/s/1".into(),
                    cursor_mode: 2
                },
                PortalCall::Start {
                    session: "/s/1".into()
                },
            ]
        );
    }

    #[tokio::test]
    async fn without_embedded_cursor_the_hidden_mode_is_asked() {
        for cursor in [Ok(1), Err(BusError::NoPortal("old portal".into()))] {
            let mut bus = FakeBus::new(vec![
                ok(0, &[session()]),
                ok(0, &[]),
                ok(0, &[streams(vec![(1, 1)])]),
            ]);
            bus.cursor = cursor;
            run_flow(&mut bus).await.expect("flow");
            assert!(matches!(
                bus.calls[1],
                PortalCall::SelectSources { cursor_mode: 1, .. }
            ));
        }
    }

    #[tokio::test]
    async fn user_cancels_at_every_step() {
        for cancel_at in [0usize, 1, 2] {
            let mut answers = vec![ok(0, &[session()]), ok(0, &[]), ok(0, &[])];
            answers[cancel_at] = ok(1, &[]);
            let mut bus = FakeBus::new(answers);
            assert_eq!(
                run_flow(&mut bus).await.unwrap_err(),
                PortalError::Cancelled
            );
        }
        assert_eq!(PortalError::Cancelled.code_text(), "PORTAL_CANCELLED");
    }

    #[tokio::test]
    async fn response_2_is_a_denial_not_a_cancel() {
        let mut bus = FakeBus::new(vec![ok(0, &[session()]), ok(2, &[])]);
        let err = run_flow(&mut bus).await.unwrap_err();
        assert_eq!(err, PortalError::Denied);
        assert_eq!(err.code_text(), "PORTAL_DENIED");
    }

    #[tokio::test]
    async fn missing_portal_is_unavailable() {
        let mut bus = FakeBus::new(vec![Err(BusError::NoPortal(
            "org.freedesktop.DBus.Error.ServiceUnknown".into(),
        ))]);
        let err = run_flow(&mut bus).await.unwrap_err();
        assert!(matches!(err, PortalError::Unavailable(_)));
        assert!(err.code_text().starts_with("PORTAL_UNAVAILABLE"));
    }

    #[tokio::test]
    async fn no_stream_chosen_or_no_remote_is_a_failure_not_a_hang() {
        let mut bus = FakeBus::new(vec![
            ok(0, &[session()]),
            ok(0, &[]),
            ok(0, &[streams(vec![])]),
        ]);
        assert!(matches!(
            run_flow(&mut bus).await,
            Err(PortalError::Failed(_))
        ));
        let mut bus = FakeBus::new(vec![
            ok(0, &[session()]),
            ok(0, &[]),
            ok(0, &[streams(vec![(7, 1)])]),
        ]);
        bus.remote = Some(Err(BusError::NoPortal("no OpenPipeWireRemote".into())));
        assert!(matches!(
            run_flow(&mut bus).await,
            Err(PortalError::Unavailable(_))
        ));
    }

    #[derive(Clone)]
    struct NeverAnswers;
    impl ScreenCastPortal for NeverAnswers {
        async fn open(&self) -> Result<PortalStream, PortalError> {
            std::future::pending().await
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_dialog_that_never_answers_times_out_and_leaves_nothing_running() {
        let result = open_screencast(&NeverAnswers, Duration::from_secs(1)).await;
        assert!(matches!(result, Err(PortalError::Failed(_))));
    }

    #[test]
    fn window_or_screen_from_the_portal_source_type() {
        let fd = || std::fs::File::open("/dev/null").expect("dev null").into();
        assert!(PortalStream::detached(fd(), 1, 2).is_window());
        assert_eq!(PortalStream::detached(fd(), 1, 1).label(), "Screen");
    }
}
