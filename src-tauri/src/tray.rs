//! The system tray icon, its four states, and the text beside it.
//!
//! The tray keeps the app reachable while its window is hidden, so closing it
//! with live SSH sessions can minimize instead of tearing connections down.
//! Because the window may be hidden, the state is derived here in Rust from
//! `SessionRegistry` rather than pushed from the frontend - a tray that froze
//! whenever the UI was closed would be worse than no tray.

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

use crate::remote::registry::SessionRegistry;

const OFFLINE: &[u8] = include_bytes!("../icons/tray/offline.png");
const ONLINE: &[u8] = include_bytes!("../icons/tray/online.png");
const CONNECTED: &[u8] = include_bytes!("../icons/tray/connected.png");

/// How fast the connecting state blinks. Slow enough to read as deliberate
/// rather than as a glitching icon.
const BLINK: Duration = Duration::from_millis(600);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    /// No route off this machine at all.
    Offline,
    /// The machine is on a network, but we hold no session.
    Online,
    /// At least one live SSH session.
    Connected,
    /// A connect is in flight.
    Connecting,
}

impl Status {
    fn icon(self) -> &'static [u8] {
        match self {
            Status::Offline => OFFLINE,
            Status::Online => ONLINE,
            // Connecting blinks between this and off; the "on" half is violet.
            Status::Connected | Status::Connecting => CONNECTED,
        }
    }

    fn label(self, connected: usize) -> String {
        match self {
            Status::Offline => "ParolaSSH - offline".into(),
            Status::Online => "ParolaSSH - online, no sessions".into(),
            Status::Connecting => "ParolaSSH - connecting…".into(),
            Status::Connected if connected == 1 => "ParolaSSH - 1 host connected".into(),
            Status::Connected => format!("ParolaSSH - {connected} hosts connected"),
        }
    }
}

/// Tray bookkeeping that outlives any one command.
#[derive(Default)]
pub struct TrayState {
    /// Connects currently in flight. A count, not a flag: two hosts can be
    /// dialled at once and the first to finish must not clear the second.
    connecting: AtomicU64,
    /// Hosts whose connect dialog is open - the user is being asked for a
    /// password or passphrase. Rust cannot see a dialog, so the frontend
    /// declares this; kept separate from `connecting` so the two cannot
    /// interfere when the same host appears in both during a submit.
    ///
    /// A set of ids rather than a count, so a repeated "opened" for the same
    /// host cannot drift the state upward and leave the icon stuck blinking.
    pending: Mutex<HashSet<String>>,
    /// Bumped every time the blink task should give up, so a stale task from a
    /// previous connect cannot keep toggling the icon.
    blink_generation: AtomicU64,
    /// The disabled first menu entry. On Linux this is the only place the
    /// status text is actually visible - see `set_text` below.
    header: Mutex<Option<MenuItem<Wry>>>,
}

/// Marks a connect as in flight for as long as it is held. A guard rather than
/// paired calls, so an early `?` return cannot leave the tray blinking forever.
pub struct ConnectingGuard(AppHandle);

impl ConnectingGuard {
    pub fn new(app: &AppHandle) -> Self {
        if let Some(state) = app.try_state::<TrayState>() {
            state.connecting.fetch_add(1, Ordering::SeqCst);
        }
        refresh(app);
        Self(app.clone())
    }
}

impl Drop for ConnectingGuard {
    fn drop(&mut self) {
        if let Some(state) = self.0.try_state::<TrayState>() {
            state.connecting.fetch_sub(1, Ordering::SeqCst);
        }
        refresh(&self.0);
    }
}

/// Declare that a host's connect dialog is open, or has closed.
///
/// Called from the dialog's mount and its cleanup, so cancelling, closing, or
/// navigating away all clear it by the same path a successful submit does.
#[tauri::command]
pub fn set_connect_pending(app: AppHandle, host_id: String, active: bool) {
    if let Some(state) = app.try_state::<TrayState>() {
        if let Ok(mut pending) = state.pending.lock() {
            if active {
                pending.insert(host_id);
            } else {
                pending.remove(&host_id);
            }
        }
    }
    refresh(&app);
}

/// Forget every open dialog. The frontend calls this as it mounts: a reload
/// destroys the dialogs but not this process, and a stale entry would leave the
/// tray blinking at a prompt that no longer exists.
#[tauri::command]
pub fn clear_connect_pending(app: AppHandle) {
    if let Some(state) = app.try_state::<TrayState>() {
        if let Ok(mut pending) = state.pending.lock() {
            pending.clear();
        }
    }
    refresh(&app);
}

/// Whether this machine has any route off itself.
///
/// A UDP `connect` only fixes the socket's peer address - it sends nothing -
/// so this asks the routing table without touching the network. It answers
/// "could we reach anything", which is what the offline state means; a host
/// that is up but firewalled must not read as the laptop being off Wi-Fi.
fn has_route() -> bool {
    use std::net::UdpSocket;

    let v4 = UdpSocket::bind("0.0.0.0:0")
        .and_then(|socket| socket.connect("1.1.1.1:80"))
        .is_ok();
    if v4 {
        return true;
    }
    UdpSocket::bind("[::]:0")
        .and_then(|socket| socket.connect("[2606:4700:4700::1111]:80"))
        .is_ok()
}

fn current(app: &AppHandle) -> (Status, usize) {
    let connected = app
        .try_state::<SessionRegistry>()
        .map(|registry| registry.connected_ids().len())
        .unwrap_or(0);

    let connecting = app
        .try_state::<TrayState>()
        .map(|state| {
            let in_flight = state.connecting.load(Ordering::SeqCst) > 0;
            let asked = state
                .pending
                .lock()
                .map(|pending| !pending.is_empty())
                .unwrap_or(false);
            // Being asked for the credential is part of connecting: from the
            // operator's side the attempt began when the dialog opened.
            in_flight || asked
        })
        .unwrap_or(false);

    // Order matters: a connect in flight is the most specific thing to say,
    // and the count is what the operator wants once one has landed.
    let status = if connecting {
        Status::Connecting
    } else if connected > 0 {
        Status::Connected
    } else if has_route() {
        Status::Online
    } else {
        Status::Offline
    };

    (status, connected)
}

fn set_icon(app: &AppHandle, bytes: &'static [u8]) {
    let Some(tray) = app.tray_by_id("main") else {
        return;
    };
    if let Ok(image) = Image::from_bytes(bytes) {
        let _ = tray.set_icon(Some(image));
    }
}

/// Recompute the state and draw it. Cheap and idempotent, so every call site
/// that might have changed something can just call it.
pub fn refresh(app: &AppHandle) {
    let (status, connected) = current(app);
    let label = status.label(connected);

    set_icon(app, status.icon());

    if let Some(tray) = app.tray_by_id("main") {
        // Windows and macOS show this on hover. libappindicator, which every
        // Linux status-notifier host uses, silently drops both tooltip and
        // title - hence the menu header below, which does render there.
        let _ = tray.set_tooltip(Some(&label));
        let _ = tray.set_title(Some(&label));
    }

    if let Some(state) = app.try_state::<TrayState>() {
        if let Ok(header) = state.header.lock() {
            if let Some(item) = header.as_ref() {
                let _ = item.set_text(&label);
            }
        }
    }

    // Any state change ends the previous blink; a new one starts only for
    // Connecting.
    let generation = match app.try_state::<TrayState>() {
        Some(state) => state.blink_generation.fetch_add(1, Ordering::SeqCst) + 1,
        None => return,
    };

    if status == Status::Connecting {
        blink(app.clone(), generation);
    }
}

/// Toggle the beam on and off until the generation moves on beneath us.
fn blink(app: AppHandle, generation: u64) {
    tauri::async_runtime::spawn(async move {
        let mut lit = true;
        loop {
            tokio::time::sleep(BLINK).await;

            let still_ours = app
                .try_state::<TrayState>()
                .map(|state| state.blink_generation.load(Ordering::SeqCst) == generation)
                .unwrap_or(false);
            if !still_ours {
                return;
            }

            lit = !lit;
            set_icon(&app, if lit { CONNECTED } else { OFFLINE });
        }
    });
}

fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

pub fn init(app: &tauri::App) -> tauri::Result<()> {
    // Managed unwrapped: `manage` keys state by its exact type, so an
    // `Arc<TrayState>` here would leave every `try_state::<TrayState>()` below
    // returning `None` - silently, since they all fall back to a default.
    let state = TrayState::default();

    // Disabled on purpose: it is a readout, not an action. First so it reads
    // as a header rather than as a skipped menu entry.
    let header = MenuItem::with_id(app, "status", "ParolaSSH", false, None::<&str>)?;
    let show = MenuItem::with_id(app, "show", "Show ParolaSSH", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&header, &show, &quit])?;

    if let Ok(mut slot) = state.header.lock() {
        *slot = Some(header);
    }

    TrayIconBuilder::with_id("main")
        .icon(Image::from_bytes(OFFLINE).expect("tray icons are rendered at build time"))
        .tooltip("ParolaSSH")
        .menu(&menu)
        // Left click restores the window, right click opens the menu. Some
        // Linux status-notifier hosts only show the menu, hence "Show".
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        })
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main(app),
            // Fires RunEvent::Exit, which disconnects every session.
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;

    app.manage(state);
    // The lookups all fall back to a default rather than failing, so a
    // mismatch here degrades quietly instead of breaking. Fail loudly in dev.
    debug_assert!(
        app.try_state::<TrayState>().is_some(),
        "TrayState must be managed under its own type"
    );
    refresh(app.handle());

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_names_the_app_in_every_state() {
        // The tooltip is the only place some hosts show the app's name, so no
        // state may drop it.
        for (status, connected) in [
            (Status::Offline, 0),
            (Status::Online, 0),
            (Status::Connecting, 0),
            (Status::Connected, 1),
            (Status::Connected, 4),
        ] {
            assert!(status.label(connected).starts_with("ParolaSSH - "));
        }
    }

    #[test]
    fn connected_label_agrees_with_its_count() {
        assert_eq!(Status::Connected.label(1), "ParolaSSH - 1 host connected");
        assert_eq!(Status::Connected.label(3), "ParolaSSH - 3 hosts connected");
    }

    #[test]
    fn connecting_and_connected_share_the_lit_icon() {
        // Connecting is the connected icon blinking, not a fourth drawing.
        assert_eq!(Status::Connecting.icon(), Status::Connected.icon());
        assert_ne!(Status::Online.icon(), Status::Connected.icon());
        assert_ne!(Status::Offline.icon(), Status::Online.icon());
    }
}
