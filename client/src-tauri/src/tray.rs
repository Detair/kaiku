//! System tray integration: tray icon with menu, unread badge, voice controls and
//! close-to-tray behavior.
//!
//! - Left-clicking the tray icon (or the "Show Kaiku" menu item) restores and focuses the main
//!   window.
//! - Closing the main window goes through [`handle_main_close`]: with a usable tray it hides the
//!   window (and tells the user Kaiku is still in a call, if it is). Without one, closing during a
//!   call minimizes (the call stays visible in the taskbar) and closing otherwise quits. "Quit
//!   Kaiku" in the tray menu exits.
//! - Whether a tray host exists is probed on a background thread (startup, each close, window
//!   focus) and cached in [`TrayState`]; the close handler only reads the cache.
//! - While in voice the tray menu also offers "Leave voice" and "Mute"/"Unmute". The webview
//!   reports the voice state via `set_tray_voice_state`; the menu items are sent back to the
//!   webview as [`VOICE_ACTION_EVENT`] so the voice store stays the single owner of the call.
//! - The unread badge is driven by the frontend via the `tray_set_unread` command: tooltip
//!   everywhere, plus a text badge next to the icon on platforms that support tray titles (macOS,
//!   most Linux trays).
//!
//! User-facing text lives in [`strings`]; Rust has no i18n layer yet, so a later
//! localisation step only has to replace that module.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use tauri::menu::{IsMenuItem, Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Runtime, Window};
use tauri_plugin_notification::NotificationExt;

/// User-facing tray and notification text (English only until Rust gets i18n).
pub mod strings {
    pub const APP_NAME: &str = "Kaiku";
    pub const MENU_SHOW: &str = "Show Kaiku";
    pub const MENU_QUIT: &str = "Quit Kaiku";
    pub const MENU_LEAVE_VOICE: &str = "Leave voice";
    pub const MENU_MUTE: &str = "Mute";
    pub const MENU_UNMUTE: &str = "Unmute";
    pub const NOTICE_IN_VOICE: &str =
        "Kaiku is still in the voice channel. Use the tray icon to leave.";
    pub const NOTICE_FIRST_CLOSE: &str =
        "Kaiku is still running. Use the tray icon to open it or quit.";
    /// Tooltip with unread messages.
    pub fn tooltip_unread(count: u64) -> String {
        format!("Kaiku — {count} unread")
    }
}

/// Tray icon id, used to look the handle back up for badge and menu updates.
pub const TRAY_ID: &str = "kaiku-tray";

/// Event emitted to the webview when a voice item of the tray menu is clicked.
/// Payload: [`VOICE_ACTION_LEAVE`] or [`VOICE_ACTION_TOGGLE_MUTE`].
pub const VOICE_ACTION_EVENT: &str = "tray-voice-action";
pub const VOICE_ACTION_LEAVE: &str = "leave";
pub const VOICE_ACTION_TOGGLE_MUTE: &str = "toggle_mute";

/// Label of the primary application window (tauri.conf.json default).
const MAIN_WINDOW_LABEL: &str = "main";

const ID_SHOW: &str = "show";
const ID_QUIT: &str = "quit";
const ID_LEAVE_VOICE: &str = "leave_voice";
const ID_TOGGLE_MUTE: &str = "toggle_mute";

/// A host probe that has not answered within this time counts as "no tray".
const PROBE_DEADLINE: Duration = Duration::from_secs(2);
/// Minimum time between two host probes (focus events can come in bursts).
const PROBE_MIN_INTERVAL: Duration = Duration::from_secs(2);

/// What closing the main window does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseAction {
    /// Hide the window; the tray icon brings it back.
    HideToTray {
        /// The user is in a call: say so, so the mic is never live unnoticed.
        notify_voice: bool,
        /// First close this session: explain that Kaiku keeps running in the tray.
        first_time: bool,
    },
    /// No usable tray but in a call: minimize, so the call stays visible in the taskbar.
    Minimize,
    /// No usable tray and not in a call: quit, as a normal window close would.
    Quit,
}

/// Decide what closing the main window does. Pure; see [`handle_main_close`].
pub const fn close_action(tray_available: bool, in_voice: bool, notice_shown: bool) -> CloseAction {
    if !tray_available {
        return if in_voice {
            CloseAction::Minimize
        } else {
            CloseAction::Quit
        };
    }
    CloseAction::HideToTray {
        notify_voice: in_voice,
        first_time: !notice_shown,
    }
}

/// The one notification shown for a hide-to-tray close, if any. The voice notice wins over
/// the first-close notice so the user gets a single message.
const fn close_notice(notify_voice: bool, first_time: bool) -> Option<&'static str> {
    if notify_voice {
        Some(strings::NOTICE_IN_VOICE)
    } else if first_time {
        Some(strings::NOTICE_FIRST_CLOSE)
    } else {
        None
    }
}

/// What happens to the window after a hide-to-tray close tried its notice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WindowOp {
    Hide,
    Minimize,
}

/// A call must never be hidden without the user being told: if the voice notice could not be
/// delivered, minimize instead of hiding. `notice_delivered` is `true` when no notice was due.
const fn after_notice(notify_voice: bool, notice_delivered: bool) -> WindowOp {
    if notify_voice && !notice_delivered {
        WindowOp::Minimize
    } else {
        WindowOp::Hide
    }
}

/// Whether the tray can take over the window for this close: the tray is up (icon built
/// and a host confirmed), and — in a call — its menu really shows the voice items.
const fn tray_usable(tray_up: bool, voice_menu_rendered: bool, in_voice: bool) -> bool {
    tray_up && (!in_voice || voice_menu_rendered)
}

/// A tray host is usable only after two positive answers: the watcher name has an owner, and
/// the watcher says a host is registered. Any error (timeout, watcher gone in between) means
/// not usable.
fn host_usable<E>(owner: Result<bool, E>, registered: impl FnOnce() -> Result<bool, E>) -> bool {
    matches!(owner, Ok(true)) && matches!(registered(), Ok(true))
}

/// Cached host flag, discarded while a probe has been running longer than the deadline.
fn cached_host_available(host: bool, probe_running_for: Option<Duration>) -> bool {
    host && probe_running_for.is_none_or(|age| age < PROBE_DEADLINE)
}

/// Tray menu as `(id, label)` pairs for the given voice state.
fn menu_entries(in_voice: bool, muted: bool) -> Vec<(&'static str, &'static str)> {
    let mut entries = vec![(ID_SHOW, strings::MENU_SHOW)];
    if in_voice {
        entries.push((ID_LEAVE_VOICE, strings::MENU_LEAVE_VOICE));
        let mute_label = if muted {
            strings::MENU_UNMUTE
        } else {
            strings::MENU_MUTE
        };
        entries.push((ID_TOGGLE_MUTE, mute_label));
    }
    entries.push((ID_QUIT, strings::MENU_QUIT));
    entries
}

/// Result of the background tray-host probe. Unknown (no probe finished yet) = not available.
#[derive(Debug, Default)]
struct HostCache {
    /// A tray host was confirmed by the last finished probe.
    host: AtomicBool,
    /// A notification service is present (Linux: owned or D-Bus activatable).
    notifier: AtomicBool,
    probe: Mutex<ProbeTimes>,
}

#[derive(Debug, Default)]
struct ProbeTimes {
    /// Set while a probe runs.
    started: Option<Instant>,
    finished: Option<Instant>,
}

impl HostCache {
    fn available(&self) -> bool {
        let running_for = self
            .probe
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .started
            .map(|t| t.elapsed());
        cached_host_available(self.host.load(Ordering::SeqCst), running_for)
    }

    fn store(&self, host: bool, notifier: bool) {
        self.host.store(host, Ordering::SeqCst);
        self.notifier.store(notifier, Ordering::SeqCst);
    }
}

/// Tray and voice flags shared by the close handler and the tray commands.
#[derive(Debug, Default)]
pub struct TrayState {
    /// The tray icon was built successfully.
    built: AtomicBool,
    cache: Arc<HostCache>,
    /// Last voice state reported by the webview (connecting counts as in voice).
    in_voice: AtomicBool,
    muted: AtomicBool,
    /// `(in_voice, muted)` of the menu the tray actually shows; `None` before the first
    /// successful render. Kept apart from the reported state so a failed update is retried.
    rendered: Mutex<Option<(bool, bool)>>,
    /// A close notice was delivered this session.
    notice_shown: AtomicBool,
}

impl TrayState {
    fn voice_menu_rendered(&self) -> bool {
        self.rendered
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_some_and(|(in_voice, _)| in_voice)
    }
}

/// Build the tray icon with its menu. Called once from the app setup hook, after
/// [`TrayState`] is managed.
pub fn setup_tray<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let menu = build_menu(app, false, false)?;

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip(strings::APP_NAME)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            ID_SHOW => show_main_window(app),
            ID_QUIT => app.exit(0),
            ID_LEAVE_VOICE => emit_voice_action(app, VOICE_ACTION_LEAVE),
            ID_TOGGLE_MUTE => emit_voice_action(app, VOICE_ACTION_TOGGLE_MUTE),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        });

    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }

    builder.build(app)?;
    if let Some(state) = app.try_state::<TrayState>() {
        state.built.store(true, Ordering::SeqCst);
        *state
            .rendered
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some((false, false));
    }
    refresh_host_availability(app);
    Ok(())
}

fn build_menu<R: Runtime>(
    app: &AppHandle<R>,
    in_voice: bool,
    muted: bool,
) -> tauri::Result<Menu<R>> {
    let items = menu_entries(in_voice, muted)
        .into_iter()
        .map(|(id, label)| MenuItem::with_id(app, id, label, true, None::<&str>))
        .collect::<tauri::Result<Vec<_>>>()?;
    let refs: Vec<&dyn IsMenuItem<R>> = items.iter().map(|i| i as &dyn IsMenuItem<R>).collect();
    Menu::with_items(app, &refs)
}

/// Show the menu for `(in_voice, muted)` unless the tray already shows it. Records the
/// rendered state only on success, so a failed update is retried on the next call.
/// Callers (window events, the sync `set_tray_voice_state` command) all run on the main
/// thread; the lock is held across the update so renders never interleave.
fn render_menu<R: Runtime>(app: &AppHandle<R>, state: &TrayState, in_voice: bool, muted: bool) {
    let mut rendered = state
        .rendered
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    if *rendered == Some((in_voice, muted)) {
        return;
    }
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    let result = build_menu(app, in_voice, muted).and_then(|menu| tray.set_menu(Some(menu)));
    match result {
        Ok(()) => *rendered = Some((in_voice, muted)),
        Err(err) => tracing::error!(error = %err, in_voice, muted, "Failed to update tray menu"),
    }
}

fn emit_voice_action<R: Runtime>(app: &AppHandle<R>, action: &str) {
    if let Err(err) = app.emit(VOICE_ACTION_EVENT, action) {
        tracing::error!(error = %err, action, "Failed to forward tray voice action");
    }
}

/// Restore, un-minimize, and focus the main window.
pub fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// Store the webview's voice state and bring the tray menu in line with it.
pub fn set_voice_state<R: Runtime>(app: &AppHandle<R>, in_voice: bool, muted: bool) {
    let Some(state) = app.try_state::<TrayState>() else {
        return;
    };
    state.in_voice.store(in_voice, Ordering::SeqCst);
    state.muted.store(muted, Ordering::SeqCst);
    render_menu(app, &state, in_voice, muted);
}

/// Close handler for the main window (its close was already prevented). Never blocks on
/// D-Bus: tray availability comes from the cache filled by the background probe.
pub fn handle_main_close<R: Runtime>(window: &Window<R>) {
    let app = window.app_handle();
    let Some(state) = app.try_state::<TrayState>() else {
        let _ = window.minimize();
        return;
    };
    let reported_in_voice = state.in_voice.load(Ordering::SeqCst);
    let in_voice = reported_in_voice || native_voice_active(app);
    if in_voice && !reported_in_voice {
        // The webview's report is still on its way: show the voice items now so the
        // notice ("use the tray icon to leave") matches the menu.
        render_menu(app, &state, true, state.muted.load(Ordering::SeqCst));
    }
    let tray_available = tray_usable(
        state.built.load(Ordering::SeqCst) && state.cache.available(),
        state.voice_menu_rendered(),
        in_voice,
    );
    let notice_shown = state.notice_shown.load(Ordering::SeqCst);

    match close_action(tray_available, in_voice, notice_shown) {
        CloseAction::HideToTray {
            notify_voice,
            first_time,
        } => {
            let delivered =
                close_notice(notify_voice, first_time).map(|body| show_notice(app, &state, body));
            match after_notice(notify_voice, delivered.unwrap_or(true)) {
                WindowOp::Hide => {
                    let _ = window.hide();
                }
                WindowOp::Minimize => {
                    tracing::warn!("Voice notice could not be shown; minimizing instead of hiding");
                    let _ = window.minimize();
                }
            }
        }
        CloseAction::Minimize => {
            let _ = window.minimize();
        }
        CloseAction::Quit => app.exit(0),
    }
    refresh_host_availability(app);
}

/// Show a close notice. Returns whether it can reach the user: the plugin hands the
/// notification to a background task and drops its result, so "delivered" means the
/// request was accepted and a notification service is present (Linux).
fn show_notice<R: Runtime>(app: &AppHandle<R>, state: &TrayState, body: &str) -> bool {
    if !state.cache.notifier.load(Ordering::SeqCst) {
        tracing::warn!("No notification service found; close notice not shown");
        return false;
    }
    match app
        .notification()
        .builder()
        .title(strings::APP_NAME)
        .body(body)
        .show()
    {
        Ok(()) => {
            state.notice_shown.store(true, Ordering::SeqCst);
            true
        }
        Err(err) => {
            tracing::warn!(error = %err, "Failed to show close-to-tray notification");
            false
        }
    }
}

/// Whether the native voice engine holds a channel. Complements the webview's report so a
/// close right after joining (before the report arrives) still counts as in voice. A
/// write-locked voice state means a join or leave is in progress: treated as in voice.
fn native_voice_active<R: Runtime>(app: &AppHandle<R>) -> bool {
    let Some(state) = app.try_state::<crate::AppState>() else {
        return false;
    };
    // Bound to a local: the read guard borrows `state` and must be dropped before it.
    let active = match state.voice.try_read() {
        Ok(voice) => voice.as_ref().is_some_and(|v| v.channel_id.is_some()),
        Err(_) => true,
    };
    active
}

/// Re-check, on a background thread, whether a tray host exists. At most one probe runs at a
/// time and probes are throttled; a probe that hangs past [`PROBE_DEADLINE`] makes the cache
/// read "not available" until it returns.
///
/// Tauri reports no failure here: on Linux, `TrayIconBuilder::build` succeeds even when no
/// `StatusNotifier` host is running (libappindicator then registers with nobody, so the icon
/// is invisible — GNOME without an `AppIndicator` extension). So on Linux the probe asks the
/// session bus. Windows and macOS always have a notification area / menu bar.
pub fn refresh_host_availability<R: Runtime>(app: &AppHandle<R>) {
    let Some(state) = app.try_state::<TrayState>() else {
        return;
    };
    spawn_probe(&state.cache);
}

#[cfg(not(target_os = "linux"))]
fn spawn_probe(cache: &Arc<HostCache>) {
    cache.store(true, true);
}

#[cfg(target_os = "linux")]
fn spawn_probe(cache: &Arc<HostCache>) {
    {
        let mut times = cache.probe.lock().unwrap_or_else(PoisonError::into_inner);
        let throttled = times
            .finished
            .is_some_and(|t| t.elapsed() < PROBE_MIN_INTERVAL);
        if times.started.is_some() || throttled {
            return;
        }
        times.started = Some(Instant::now());
    }
    let worker_cache = Arc::clone(cache);
    let spawned = std::thread::Builder::new()
        .name("tray-host-probe".into())
        .spawn(move || {
            let (host, notifier) = linux::probe();
            worker_cache.store(host, notifier);
            let mut times = worker_cache
                .probe
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            times.started = None;
            times.finished = Some(Instant::now());
            drop(times);
            if !host {
                tracing::info!(
                    "No system tray host found (e.g. GNOME without an AppIndicator extension); \
                     closing the window quits, or minimizes during a call"
                );
            }
        });
    if let Err(err) = spawned {
        tracing::error!(error = %err, "Failed to start tray host probe");
        cache.store(false, false);
        cache
            .probe
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .started = None;
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use std::sync::{Mutex, PoisonError};
    use std::time::Duration;

    use zbus::blocking::fdo::{DBusProxy, PropertiesProxy};
    use zbus::blocking::{connection, Connection};
    use zbus::names::{BusName, InterfaceName};

    const WATCHER: &str = "org.kde.StatusNotifierWatcher";
    const WATCHER_PATH: &str = "/StatusNotifierWatcher";
    const NOTIFICATIONS: &str = "org.freedesktop.Notifications";

    /// One session-bus connection for all probes (no executor set up per probe).
    static CONNECTION: Mutex<Option<Connection>> = Mutex::new(None);

    /// `(tray host usable, notification service present)`. Runs on the probe thread only.
    pub(super) fn probe() -> (bool, bool) {
        let mut slot = CONNECTION.lock().unwrap_or_else(PoisonError::into_inner);
        if slot.is_none() {
            match connection::Builder::session()
                .map(|b| b.method_timeout(Duration::from_millis(500)))
                .and_then(|b| b.build())
            {
                Ok(conn) => *slot = Some(conn),
                Err(err) => {
                    tracing::debug!(error = %err, "No session bus for tray probe");
                    return (false, false);
                }
            }
        }
        let Some(conn) = slot.as_ref() else {
            return (false, false);
        };
        let (host, notifier, healthy) = query(conn);
        if !healthy {
            // Reconnect next time (bus restarted, connection broken).
            *slot = None;
        }
        (host, notifier)
    }

    fn query(conn: &Connection) -> (bool, bool, bool) {
        let Ok(dbus) = DBusProxy::new(conn) else {
            return (false, false, false);
        };
        let has_owner = |name: &'static str| -> Result<bool, String> {
            let name = BusName::try_from(name).map_err(|e| e.to_string())?;
            dbus.name_has_owner(name).map_err(|e| e.to_string())
        };
        let owner = has_owner(WATCHER);
        let healthy = owner.is_ok();
        let host = super::host_usable(owner, || {
            let props = PropertiesProxy::builder(conn)
                .destination(WATCHER)
                .and_then(|b| b.path(WATCHER_PATH))
                .and_then(|b| b.build())
                .map_err(|e| e.to_string())?;
            let iface = InterfaceName::try_from(WATCHER).map_err(|e| e.to_string())?;
            let value = props
                .get(iface, "IsStatusNotifierHostRegistered")
                .map_err(|e| e.to_string())?;
            bool::try_from(value).map_err(|e| e.to_string())
        });
        let notifier = matches!(has_owner(NOTIFICATIONS), Ok(true))
            || dbus
                .list_activatable_names()
                .is_ok_and(|names| names.iter().any(|n| n.as_str() == NOTIFICATIONS));
        (host, notifier, healthy)
    }
}

/// Update the tray's unread indicator.
///
/// Tooltip is supported on all platforms; `set_title` renders a text badge
/// next to the icon on macOS and most Linux status trays, and is a no-op
/// elsewhere.
pub fn update_unread_badge<R: Runtime>(app: &AppHandle<R>, count: u64) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    if count > 0 {
        let _ = tray.set_tooltip(Some(strings::tooltip_unread(count)));
        let _ = tray.set_title(Some(count.to_string()));
    } else {
        let _ = tray.set_tooltip(Some(strings::APP_NAME));
        let _ = tray.set_title(None::<String>);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_without_tray_minimizes() {
        // No usable tray during a call: minimize, the call stays visible in the taskbar.
        for notice_shown in [false, true] {
            assert_eq!(
                close_action(false, true, notice_shown),
                CloseAction::Minimize
            );
        }
    }

    #[test]
    fn close_without_tray_and_not_in_voice_quits() {
        // Without a tray there is no other way to quit (GNOME without AppIndicator).
        for notice_shown in [false, true] {
            assert_eq!(close_action(false, false, notice_shown), CloseAction::Quit);
        }
    }

    #[test]
    fn close_in_voice_hides_with_voice_notice() {
        // Every close during a call tells the user the mic is still live,
        // even after the first-close notice was already shown.
        for notice_shown in [false, true] {
            match close_action(true, true, notice_shown) {
                CloseAction::HideToTray { notify_voice, .. } => assert!(notify_voice),
                other => panic!("tray available: must hide, got {other:?}"),
            }
        }
        assert_eq!(close_notice(true, true), Some(strings::NOTICE_IN_VOICE));
        assert_eq!(close_notice(true, false), Some(strings::NOTICE_IN_VOICE));
        assert_eq!(after_notice(true, true), WindowOp::Hide);
    }

    #[test]
    fn undelivered_voice_notice_minimizes_instead_of_hiding() {
        assert_eq!(after_notice(true, false), WindowOp::Minimize);
        // Not in a call: a missing first-close notice does not keep the window up.
        assert_eq!(after_notice(false, false), WindowOp::Hide);
        assert_eq!(after_notice(false, true), WindowOp::Hide);
    }

    #[test]
    fn first_close_shows_notice_once() {
        assert_eq!(
            close_action(true, false, false),
            CloseAction::HideToTray {
                notify_voice: false,
                first_time: true
            }
        );
        assert_eq!(
            close_action(true, false, true),
            CloseAction::HideToTray {
                notify_voice: false,
                first_time: false
            }
        );
        assert_eq!(close_notice(false, true), Some(strings::NOTICE_FIRST_CLOSE));
        assert_eq!(close_notice(false, false), None);
    }

    #[test]
    fn tray_unusable_in_voice_without_rendered_voice_menu() {
        assert!(!tray_usable(true, false, true));
        assert!(tray_usable(true, true, true));
        // Outside a call the voice items are not needed.
        assert!(tray_usable(true, false, false));
        // No tray at all.
        assert!(!tray_usable(false, true, true));
        assert!(!tray_usable(false, true, false));
    }

    #[test]
    fn host_check_requires_two_positive_answers() {
        assert!(host_usable::<()>(Ok(true), || Ok(true)));
        // No watcher: the property is never asked.
        assert!(!host_usable::<()>(Ok(false), || panic!("must not query")));
        // Watcher present but no host registered.
        assert!(!host_usable::<()>(Ok(true), || Ok(false)));
        // Owner query failed (timeout, broken bus).
        assert!(!host_usable(Err("timeout"), || Ok(true)));
    }

    #[test]
    fn host_property_error_means_no_tray() {
        assert!(!host_usable(Ok(true), || Err(
            "org.freedesktop.DBus.Error.Timeout"
        )));
    }

    #[test]
    fn watcher_vanishing_after_owner_check_means_no_tray() {
        assert!(!host_usable(Ok(true), || Err(
            "org.freedesktop.DBus.Error.ServiceUnknown"
        )));
    }

    #[test]
    fn host_cache_unknown_or_stale_probe_is_unavailable() {
        // Not yet determined (default false) → not available.
        assert!(!HostCache::default().available());
        assert!(cached_host_available(true, None));
        assert!(cached_host_available(
            true,
            Some(Duration::from_millis(100))
        ));
        // A probe hanging past the deadline invalidates the cached "yes".
        assert!(!cached_host_available(true, Some(PROBE_DEADLINE)));
        assert!(!cached_host_available(false, None));
    }

    /// Live check against this machine's session bus: `cargo test -- --ignored tray`.
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "needs a desktop session bus"]
    fn live_probe_reports_session_tray() {
        let (host, notifier) = linux::probe();
        eprintln!("tray host: {host}, notification service: {notifier}");
        // Second probe reuses the cached connection.
        assert_eq!(linux::probe(), (host, notifier));
    }

    #[test]
    fn tray_menu_offers_leave_and_mute_only_in_voice() {
        let ids = |in_voice, muted| -> Vec<&'static str> {
            menu_entries(in_voice, muted).iter().map(|e| e.0).collect()
        };
        assert_eq!(ids(false, false), vec![ID_SHOW, ID_QUIT]);
        assert_eq!(
            ids(true, false),
            vec![ID_SHOW, ID_LEAVE_VOICE, ID_TOGGLE_MUTE, ID_QUIT]
        );

        let label = |in_voice, muted, id| {
            menu_entries(in_voice, muted)
                .into_iter()
                .find(|e| e.0 == id)
                .map(|e| e.1)
        };
        assert_eq!(label(true, false, ID_TOGGLE_MUTE), Some(strings::MENU_MUTE));
        assert_eq!(
            label(true, true, ID_TOGGLE_MUTE),
            Some(strings::MENU_UNMUTE)
        );
        assert_eq!(
            label(true, true, ID_LEAVE_VOICE),
            Some(strings::MENU_LEAVE_VOICE)
        );
    }
}
