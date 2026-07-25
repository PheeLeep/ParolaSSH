//! Tauri commands for reaching a remote machine.
//!
//! Passwords arrive as arguments here and are turned into `Zeroizing` values
//! immediately. They are never written to the host store, never logged, and
//! never interpolated into a command string.

use std::time::Duration;

use tauri::{AppHandle, State};
use zeroize::Zeroizing;

use super::client::{Credentials, Session, Target};
use super::power::{self, Elevation, PowerOutcome, PowerRequest, PowerPlan, PrivilegeReport};
use super::probe::{self, ProbeResult};
use super::registry::{LiveSession, SessionRegistry};
use super::secrets::SecretVault;
use super::{shell, OsFamily};
use crate::app_paths::config_dir;
use crate::hosts::model::{AuthMethod, HostRecord};
use crate::hosts::store::{now_iso8601, HostStore};
use crate::ssh::{SshError, SshResult};

/// What the UI knows about a live connection.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionInfo {
    pub host_id: String,
    pub connected: bool,
    pub os: OsFamily,
    pub os_detail: String,
    pub user: String,
    pub elevation: Elevation,
    pub elevation_explanation: String,
    pub supports_force: bool,
    pub supports_cancel: bool,
    pub fingerprint: Option<String>,
    pub connected_at: String,
    pub has_shell: bool,
    /// Whether the session holds the password it logged in with, so `sudo`
    /// can reuse it instead of asking for the same string again.
    pub has_login_password: bool,
}

/// Check the port before spending time on a handshake.
///
/// Exposed on its own because "is 22 even the right port?" is the first
/// question when a connection fails, and answering it does not need
/// credentials.
#[tauri::command]
pub async fn probe_host(hostname: String, port: u16) -> SshResult<ProbeResult> {
    probe::probe(hostname.trim(), port).await
}

/// Connect, authenticate, and work out how this account elevates.
///
/// `password` is required for password auth unless one is already remembered.
/// `remember` keeps it for the rest of the app's run — see `secrets`.
#[tauri::command]
pub async fn connect_host(
    app: AppHandle,
    registry: State<'_, SessionRegistry>,
    vault: State<'_, SecretVault>,
    host_id: String,
    password: Option<String>,
    remember: bool,
    trust_unknown: bool,
) -> SshResult<ConnectionInfo> {
    // Take ownership as a `Zeroizing` immediately: from here on the plaintext
    // is wiped when it goes out of scope instead of lingering in freed heap.
    let password = password.map(Zeroizing::new);
    let config_dir = config_dir(&app)?;
    let host = HostStore::read(&config_dir)
        .get(&host_id)
        .cloned()
        .ok_or_else(|| SshError::invalid("That connection no longer exists."))?;

    let credentials = build_credentials(&host, password.as_deref().map(|p| p.as_str()), &vault)?;

    let target = Target {
        hostname: host.hostname.clone(),
        port: host.port,
        username: host.username.clone(),
    };

    let session = match Session::connect(&target, &credentials, trust_unknown).await {
        Ok(session) => session,
        Err(error) => {
            // A rejected password must not be replayed on the next attempt —
            // repeated failures are how accounts get locked out.
            if host.auth_method == AuthMethod::Password {
                vault.forget(&host_id);
            }
            return Err(error);
        }
    };

    if remember && host.auth_method == AuthMethod::Password {
        if let Some(password) = password.as_deref() {
            vault.remember(&host_id, password.as_str());
        }
    }

    // Learn the OS and elevation route once, at connect time: every later
    // power action needs both, and they cannot change under a live session.
    let report = power::check_privileges(&session).await?;

    let connected_at = now_iso8601();
    let mut store = HostStore::read(&config_dir);
    store.touch(&host_id, connected_at.clone());
    store.write(&config_dir)?;

    let live = registry.insert(LiveSession::new(
        host_id.clone(),
        session,
        report.os,
        report.os_detail.clone(),
        report.elevation.clone(),
        connected_at.clone(),
    ));

    // Hold the login password for the life of the session so `sudo` can reuse
    // it. On a Unix host the two are the same string in almost every case, and
    // asking twice for it teaches people to type passwords into any box that
    // appears. It is dropped when the session is, independently of whether the
    // user asked for it to be remembered across connections.
    if host.auth_method == AuthMethod::Password {
        if let Some(password) = password.clone() {
            live.set_login_password(password);
        }
    }

    Ok(ConnectionInfo {
        host_id,
        connected: true,
        os: report.os,
        os_detail: report.os_detail,
        user: report.user,
        elevation: report.elevation,
        elevation_explanation: report.explanation,
        supports_force: report.supports_force,
        supports_cancel: report.supports_cancel,
        fingerprint: live.fingerprint.clone(),
        connected_at,
        has_shell: false,
        has_login_password: live.has_login_password(),
    })
}

#[tauri::command]
pub async fn disconnect_host(
    registry: State<'_, SessionRegistry>,
    host_id: String,
) -> SshResult<bool> {
    Ok(registry.disconnect(&host_id).await)
}

/// Which hosts currently have a live session, so the list can show it.
#[tauri::command]
pub fn connected_hosts(registry: State<'_, SessionRegistry>) -> Vec<String> {
    registry.connected_ids()
}

/// Whether a password is already held for this host this run.
#[tauri::command]
pub fn has_remembered_password(vault: State<'_, SecretVault>, host_id: String) -> bool {
    vault.has(&host_id)
}

#[tauri::command]
pub fn forget_password(vault: State<'_, SecretVault>, host_id: String) {
    vault.forget(&host_id);
}

/// How this account will elevate, re-read from the live session.
#[tauri::command]
pub async fn privilege_report(
    registry: State<'_, SessionRegistry>,
    host_id: String,
) -> SshResult<PrivilegeReport> {
    let live = registry.require(&host_id)?;
    power::check_privileges(&live.session).await
}

/// The exact command a power request would run, without running it.
///
/// The confirm dialog shows this. A power button that will not say what it
/// does is a power button nobody should press.
#[tauri::command]
pub fn preview_power(
    registry: State<'_, SessionRegistry>,
    host_id: String,
    request: PowerRequest,
) -> SshResult<PowerPlan> {
    let live = registry.require(&host_id)?;
    power::plan(live.os, &live.elevation, &request)
}

/// Shut down, reboot, or cancel a pending shutdown.
#[tauri::command]
pub async fn power_host(
    registry: State<'_, SessionRegistry>,
    vault: State<'_, SecretVault>,
    host_id: String,
    request: PowerRequest,
    password: Option<String>,
) -> SshResult<PowerOutcome> {
    let live = registry.require(&host_id)?;

    // sudo needs the account password. Prefer an explicit one from the dialog
    // — someone may sudo as a different account than they logged in as — then
    // the password this session authenticated with, then the remembered one.
    let sudo_password: Option<Zeroizing<String>> = password
        .map(Zeroizing::new)
        .or_else(|| live.login_password())
        .or_else(|| vault.recall(&host_id));

    let outcome = power::execute(
        &live.session,
        live.os,
        &live.elevation,
        &request,
        sudo_password.as_deref().map(|password| password.as_str()),
    )
    .await?;

    // An immediate shutdown or reboot takes the session with it; leaving a
    // dead entry in the registry would show the host as connected forever.
    let terminal = request.delay_minutes == 0
        && !matches!(request.action, power::PowerAction::Cancel)
        && outcome.succeeded;
    if terminal {
        registry.disconnect(&host_id).await;
    }

    Ok(outcome)
}

/// One host's liveness, as of the last heartbeat.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostHealth {
    pub host_id: String,
    /// Whether there is a live authenticated session.
    pub connected: bool,
    /// Whether the port answered — true for a reachable host we are not
    /// logged in to.
    pub reachable: bool,
    pub latency_ms: Option<u64>,
}

/// How long a single host gets to answer before the heartbeat gives up on it.
///
/// Short, because every saved host is checked on the same cycle and a
/// black-holed address must not hold up the rest of the list.
const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(3);

/// Check every saved host: are they up, and are our sessions still good?
///
/// Called on a timer by the UI. Connected hosts get a channel-open round trip
/// on the existing session — which also notices a machine that rebooted out
/// from under us — and everything else gets a plain TCP probe. All of it runs
/// concurrently, so the cycle costs about one timeout, not one per host.
#[tauri::command]
pub async fn heartbeat(
    app: AppHandle,
    registry: State<'_, SessionRegistry>,
) -> SshResult<Vec<HostHealth>> {
    let hosts = HostStore::read(&config_dir(&app)?).hosts;

    let checks = hosts.into_iter().map(|host| {
        let live = registry.get(&host.id);
        async move {
            if let Some(live) = live {
                let started = std::time::Instant::now();
                if live.session.is_alive().await {
                    return HostHealth {
                        host_id: host.id,
                        connected: true,
                        reachable: true,
                        latency_ms: Some(started.elapsed().as_millis() as u64),
                    };
                }

                // The session is gone. Fall through to a plain probe so the
                // UI can still distinguish "host up, session dropped" from
                // "host down"; the dead entry is reaped below.
            }

            let (reachable, latency_ms) =
                probe::reachable(&host.hostname, host.port, HEARTBEAT_TIMEOUT).await;

            HostHealth {
                host_id: host.id,
                connected: false,
                reachable,
                latency_ms,
            }
        }
    });

    let health = futures_util::future::join_all(checks).await;

    // Drop sessions that failed their liveness check, so the registry does not
    // keep handing out a handle to a connection that is no longer there.
    for entry in &health {
        if !entry.connected && registry.is_connected(&entry.host_id) {
            registry.disconnect(&entry.host_id).await;
        }
    }

    Ok(health)
}

/// Open an interactive shell. Output arrives as `terminal://output` events.
///
/// There is deliberately no general "run this string" command alongside this
/// one. A terminal is an arbitrary-execution primitive the user is driving
/// and watching; a silent `run_command(String)` would be the same power with
/// none of the visibility, and every caller we actually have is either the
/// terminal or a purpose-built verb like `power_host`.
#[tauri::command]
pub async fn open_shell(
    app: AppHandle,
    webview: tauri::Webview,
    registry: State<'_, SessionRegistry>,
    host_id: String,
    cols: u32,
    rows: u32,
) -> SshResult<u64> {
    let live = registry.require(&host_id)?;

    // Serialise the whole replace-then-open sequence. Two callers racing here
    // would otherwise both close the old shell and both install a new one,
    // leaving one orphaned — still authenticated, still streaming its banner
    // and prompt into whichever pane is listening.
    let _guard = live.lock_shell_open().await;

    // Reopening replaces the old shell rather than stacking a second one on
    // the same session, which would interleave output from both.
    if let Some(existing) = live.take_shell() {
        existing.close().await;
    }

    // Output goes back to the window that asked for it, not to every window.
    let label = webview.label().to_string();

    let handle = shell::open(&live.session, app, label, host_id, cols, rows).await?;
    let shell_id = handle.id;
    live.set_shell(handle);

    // Returned so the pane can filter events to its own shell, and later close
    // exactly the one it opened.
    Ok(shell_id)
}

#[tauri::command]
pub async fn write_shell(
    registry: State<'_, SessionRegistry>,
    host_id: String,
    data: String,
) -> SshResult<()> {
    let live = registry.require(&host_id)?;
    let shell = live
        .shell()
        .ok_or_else(|| SshError::invalid("There is no open terminal for that host."))?;

    shell.write(&data).await
}

#[tauri::command]
pub async fn resize_shell(
    registry: State<'_, SessionRegistry>,
    host_id: String,
    cols: u32,
    rows: u32,
) -> SshResult<()> {
    let live = registry.require(&host_id)?;
    // A resize arriving after the shell closed is normal — the pane is still
    // laying out as it unmounts — so it is ignored rather than an error.
    if let Some(shell) = live.shell() {
        shell.resize(cols, rows).await?;
    }
    Ok(())
}

/// Close a shell.
///
/// `shell_id` names which one. Passing it means a pane that is unmounting can
/// only ever close its own session — without it, a teardown arriving after a
/// reopen would silently kill the shell the user is now typing into. Omitting
/// it closes whatever is current, which is what an explicit "close" button
/// wants.
#[tauri::command]
pub async fn close_shell(
    registry: State<'_, SessionRegistry>,
    host_id: String,
    shell_id: Option<u64>,
) -> SshResult<()> {
    let Some(live) = registry.get(&host_id) else {
        return Ok(());
    };

    let shell = match shell_id {
        Some(id) => live.take_shell_if(id),
        None => live.take_shell(),
    };

    if let Some(shell) = shell {
        shell.close().await;
    }

    Ok(())
}

/// Decide what to authenticate with, given the record and what the UI sent.
fn build_credentials(
    host: &HostRecord,
    password: Option<&str>,
    vault: &SecretVault,
) -> SshResult<Credentials> {
    match host.auth_method {
        AuthMethod::Password => {
            let password = password
                .map(|password| Zeroizing::new(password.to_string()))
                .or_else(|| vault.recall(&host.id))
                .ok_or_else(|| {
                    SshError::invalid(format!(
                        "A password is needed for {}@{}.",
                        host.username, host.hostname
                    ))
                })?;

            if password.is_empty() {
                return Err(SshError::invalid("The password cannot be empty."));
            }
            Ok(Credentials::Password(password))
        }

        AuthMethod::Publickey => {
            let path = host.key_path.clone().ok_or_else(|| {
                SshError::invalid("This connection has no private key set. Edit it and choose one.")
            })?;

            Ok(Credentials::Key {
                path,
                // For a key, any password supplied is the key's passphrase.
                passphrase: password
                    .filter(|passphrase| !passphrase.is_empty())
                    .map(|passphrase| Zeroizing::new(passphrase.to_string())),
            })
        }

        AuthMethod::Agent => Ok(Credentials::Agent),
    }
}
