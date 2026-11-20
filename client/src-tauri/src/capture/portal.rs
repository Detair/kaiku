//! Screen capture on Linux through the xdg-desktop-portal ScreenCast
//! interface (Wayland and X11 alike): the system dialog chooses the screen
//! or window, the portal hands back a PipeWire remote and node.
//!
//! `scap` cannot be used here: it lists no sources on Linux and its portal
//! code panics on a cancelled dialog or a missing portal (the release build
//! aborts on panic). This module talks to the portal over `zbus` (already a
//! Linux dependency) and returns errors instead.
//!
//! The flow (`run_flow`) is written against [`PortalBus`] so tests can fake
//! the bus: cancel, missing portal and success are covered without a
//! desktop session.

use std::collections::HashMap;
use std::os::fd::OwnedFd;
use std::time::Duration;

use thiserror::Error;
use tracing::info;

/// Why no screen could be opened.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum PortalError {
    /// No portal (or no ScreenCast interface, or no session bus).
    #[error("PORTAL_UNAVAILABLE: {0}")]
    Unavailable(String),
    /// The user closed the dialog.
    #[error("PORTAL_CANCELLED")]
    Cancelled,
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
    /// `streams`: (PipeWire node id, source type 1 = monitor, 2 = window).
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

/// The portal as the flow sees it (blocking calls).
pub trait PortalBus {
    /// Make a request call and wait for its `Response` (code, results).
    fn request(
        &mut self,
        call: PortalCall,
    ) -> Result<(u32, HashMap<String, PortalValue>), BusError>;
    /// `AvailableCursorModes` (bit 1 hidden, 2 embedded, 4 metadata).
    fn cursor_modes(&mut self) -> Result<u32, BusError>;
    /// `OpenPipeWireRemote` for the started session.
    fn open_pipewire_remote(&mut self, session: &str) -> Result<OwnedFd, BusError>;
}

/// What a successful flow yields.
#[derive(Debug)]
pub struct FlowResult {
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
        // The dialog was closed some other way (e.g. by the compositor).
        2 => Err(PortalError::Cancelled),
        other => Err(PortalError::Failed(format!("portal response {other}"))),
    }
}

/// CreateSession → SelectSources → Start → OpenPipeWireRemote.
pub fn run_flow<B: PortalBus>(bus: &mut B) -> Result<FlowResult, PortalError> {
    let (code, results) = bus.request(PortalCall::CreateSession).map_err(map_bus)?;
    check_code(code)?;
    let session = match results.get("session_handle") {
        Some(PortalValue::Str(s)) => s.clone(),
        _ => return Err(PortalError::Failed("no session handle".into())),
    };
    // Show the pointer in the picture when the portal can draw it in.
    let cursor_mode = match bus.cursor_modes() {
        Ok(modes) if modes & 2 != 0 => 2,
        Ok(_) | Err(_) => 1,
    };
    let (code, _) = bus
        .request(PortalCall::SelectSources {
            session: session.clone(),
            cursor_mode,
        })
        .map_err(map_bus)?;
    check_code(code)?;
    let (code, results) = bus
        .request(PortalCall::Start {
            session: session.clone(),
        })
        .map_err(map_bus)?;
    check_code(code)?;
    let (node_id, source_type) = match results.get("streams") {
        Some(PortalValue::Streams(streams)) => streams
            .first()
            .copied()
            .ok_or_else(|| PortalError::Failed("no stream chosen".into()))?,
        _ => return Err(PortalError::Failed("no streams in the answer".into())),
    };
    let fd = bus.open_pipewire_remote(&session).map_err(map_bus)?;
    Ok(FlowResult {
        fd,
        node_id,
        source_type,
    })
}

/// An open ScreenCast session: the PipeWire remote and node. Dropping it
/// closes the session (the portal ends it with the D-Bus connection).
pub struct PortalStream {
    pub fd: OwnedFd,
    pub node_id: u32,
    pub source_type: u32,
    _session: Option<Box<dyn std::any::Any + Send + Sync>>,
}

impl PortalStream {
    /// Label sent to the server as the share's source.
    pub fn label(&self) -> String {
        if self.source_type == 2 {
            "Window"
        } else {
            "Screen"
        }
        .to_string()
    }
}

/// How the app opens the portal (a seam: tests pass a fake).
pub trait ScreenCastPortal: Clone + Send + 'static {
    fn open(&self) -> Result<PortalStream, PortalError>;
}

/// The longest the dialog may stay open.
const DIALOG_TIMEOUT: Duration = Duration::from_secs(300);

/// Run the portal off the async runtime; the dialog may take minutes.
pub async fn open_screencast<P: ScreenCastPortal>(portal: &P) -> Result<PortalStream, PortalError> {
    let portal = portal.clone();
    match tokio::time::timeout(
        DIALOG_TIMEOUT,
        tokio::task::spawn_blocking(move || portal.open()),
    )
    .await
    {
        Ok(Ok(result)) => result,
        Ok(Err(e)) => Err(PortalError::Failed(e.to_string())),
        Err(_) => Err(PortalError::Failed(
            "the screen dialog did not answer".into(),
        )),
    }
}

/// The real portal on the session bus.
#[derive(Clone, Copy)]
pub struct DbusPortal;

impl ScreenCastPortal for DbusPortal {
    fn open(&self) -> Result<PortalStream, PortalError> {
        let mut bus = dbus::ZbusPortal::connect()?;
        let flow = run_flow(&mut bus)?;
        info!(node = flow.node_id, "Screen cast portal session started");
        Ok(PortalStream {
            fd: flow.fd,
            node_id: flow.node_id,
            source_type: flow.source_type,
            _session: Some(Box::new(bus.into_connection())),
        })
    }
}

mod dbus {
    use std::collections::HashMap;
    use std::os::fd::OwnedFd;

    use zbus::blocking::{Connection, Proxy};
    use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

    use super::{BusError, PortalBus, PortalCall, PortalError, PortalValue};

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

    impl ZbusPortal {
        pub fn connect() -> Result<Self, PortalError> {
            let conn = Connection::session()
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

        fn proxy(&self) -> Result<Proxy<'static>, BusError> {
            Proxy::new(&self.conn, DEST, PATH, IFACE).map_err(|e| bus_err(&e))
        }

        fn next_token(&mut self) -> String {
            self.counter += 1;
            format!("kaiku{}_{}", std::process::id(), self.counter)
        }
    }

    fn results_of(raw: HashMap<String, OwnedValue>) -> HashMap<String, PortalValue> {
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
        fn request(
            &mut self,
            call: PortalCall,
        ) -> Result<(u32, HashMap<String, PortalValue>), BusError> {
            let token = self.next_token();
            // Subscribe to the answer before asking, so it cannot be missed.
            let request_path = format!("{PATH}/request/{}/{token}", self.sender);
            let request = Proxy::new(
                &self.conn,
                DEST,
                request_path,
                "org.freedesktop.portal.Request",
            )
            .map_err(|e| bus_err(&e))?;
            let mut answers = request
                .receive_signal("Response")
                .map_err(|e| bus_err(&e))?;

            let proxy = self.proxy()?;
            let mut options: HashMap<&str, Value<'_>> = HashMap::new();
            options.insert("handle_token", Value::from(token.as_str()));
            let session_token = self.next_token();
            let _: OwnedObjectPath = match &call {
                PortalCall::CreateSession => {
                    options.insert("session_handle_token", Value::from(session_token.as_str()));
                    proxy.call("CreateSession", &(options,))
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
                    proxy.call("SelectSources", &(path, options))
                }
                PortalCall::Start { session } => {
                    let path = zbus::zvariant::ObjectPath::try_from(session.as_str())
                        .map_err(|e| BusError::Other(e.to_string()))?;
                    proxy.call("Start", &(path, "", options))
                }
            }
            .map_err(|e| bus_err(&e))?;

            let message = answers
                .next()
                .ok_or_else(|| BusError::Other("no answer from the portal".into()))?;
            let (code, raw): (u32, HashMap<String, OwnedValue>) = message
                .body()
                .deserialize()
                .map_err(|e| BusError::Other(e.to_string()))?;
            Ok((code, results_of(raw)))
        }

        fn cursor_modes(&mut self) -> Result<u32, BusError> {
            self.proxy()?
                .get_property::<u32>("AvailableCursorModes")
                .map_err(|e| bus_err(&e))
        }

        fn open_pipewire_remote(&mut self, session: &str) -> Result<OwnedFd, BusError> {
            let path = zbus::zvariant::ObjectPath::try_from(session)
                .map_err(|e| BusError::Other(e.to_string()))?;
            let options: HashMap<&str, Value<'_>> = HashMap::new();
            let fd: zbus::zvariant::OwnedFd = self
                .proxy()?
                .call("OpenPipeWireRemote", &(path, options))
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
        answers: Vec<Result<(u32, HashMap<String, PortalValue>), BusError>>,
        calls: Vec<PortalCall>,
        remote: Option<Result<(), BusError>>,
    }

    fn ok(
        code: u32,
        results: &[(&str, PortalValue)],
    ) -> Result<(u32, HashMap<String, PortalValue>), BusError> {
        Ok((
            code,
            results
                .iter()
                .map(|(k, v)| ((*k).to_string(), v.clone()))
                .collect(),
        ))
    }

    impl PortalBus for FakeBus {
        fn request(
            &mut self,
            call: PortalCall,
        ) -> Result<(u32, HashMap<String, PortalValue>), BusError> {
            self.calls.push(call);
            self.answers.remove(0)
        }

        fn cursor_modes(&mut self) -> Result<u32, BusError> {
            Ok(1 | 2)
        }

        fn open_pipewire_remote(&mut self, _session: &str) -> Result<OwnedFd, BusError> {
            match self.remote.take().unwrap_or(Ok(())) {
                Ok(()) => Ok(std::fs::File::open("/dev/null").expect("dev null").into()),
                Err(e) => Err(e),
            }
        }
    }

    fn session() -> (&'static str, PortalValue) {
        ("session_handle", PortalValue::Str("/s/1".into()))
    }

    #[test]
    fn success_yields_the_node_and_a_remote() {
        let mut bus = FakeBus {
            answers: vec![
                ok(0, &[session()]),
                ok(0, &[]),
                ok(0, &[("streams", PortalValue::Streams(vec![(42, 2)]))]),
            ],
            calls: vec![],
            remote: None,
        };
        let flow = run_flow(&mut bus).expect("flow");
        assert_eq!((flow.node_id, flow.source_type), (42, 2));
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

    #[test]
    fn user_cancels_the_dialog() {
        for cancel_at in [1usize, 2] {
            let mut answers = vec![ok(0, &[session()]), ok(0, &[]), ok(0, &[])];
            answers[cancel_at] = ok(1, &[]);
            let mut bus = FakeBus {
                answers,
                calls: vec![],
                remote: None,
            };
            assert_eq!(run_flow(&mut bus).unwrap_err(), PortalError::Cancelled);
            assert_eq!(PortalError::Cancelled.code_text(), "PORTAL_CANCELLED");
        }
    }

    #[test]
    fn missing_portal_is_unavailable() {
        let mut bus = FakeBus {
            answers: vec![Err(BusError::NoPortal(
                "org.freedesktop.DBus.Error.ServiceUnknown".into(),
            ))],
            calls: vec![],
            remote: None,
        };
        let err = run_flow(&mut bus).unwrap_err();
        assert!(matches!(err, PortalError::Unavailable(_)));
        assert!(err.code_text().starts_with("PORTAL_UNAVAILABLE"));
    }

    #[test]
    fn no_stream_chosen_or_no_remote_is_a_failure_not_a_hang() {
        let mut bus = FakeBus {
            answers: vec![
                ok(0, &[session()]),
                ok(0, &[]),
                ok(0, &[("streams", PortalValue::Streams(vec![]))]),
            ],
            calls: vec![],
            remote: None,
        };
        assert!(matches!(run_flow(&mut bus), Err(PortalError::Failed(_))));
        let mut bus = FakeBus {
            answers: vec![
                ok(0, &[session()]),
                ok(0, &[]),
                ok(0, &[("streams", PortalValue::Streams(vec![(7, 1)]))]),
            ],
            calls: vec![],
            remote: Some(Err(BusError::Other("denied".into()))),
        };
        assert!(matches!(run_flow(&mut bus), Err(PortalError::Failed(_))));
    }

    #[derive(Clone)]
    struct CancellingPortal;
    impl ScreenCastPortal for CancellingPortal {
        fn open(&self) -> Result<PortalStream, PortalError> {
            Err(PortalError::Cancelled)
        }
    }

    #[tokio::test]
    async fn open_screencast_passes_the_cancel_through() {
        assert!(matches!(
            open_screencast(&CancellingPortal).await,
            Err(PortalError::Cancelled)
        ));
    }
}
