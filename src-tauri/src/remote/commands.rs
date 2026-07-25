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
use super::services::{
    self, ServiceActionRequest, ServiceEntry, ServiceLog, ServiceOutcome, ServicePlan,
};
use super::{shell, stream, OsFamily};
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
    /// What the key exchange negotiated, for the audit tab's free tier.
    pub negotiated: Option<super::client::NegotiatedCrypto>,
    pub connected_at: String,
    /// Shells already open on this host — empty for a fresh connection.
    pub shell_ids: Vec<u64>,
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
    let mut result = probe::probe(hostname.trim(), port).await?;

    // Silence from a VPN address is usually the VPN's doing, not the host's.
    // The check lives here rather than in `probe` so the probe itself stays a
    // pure network primitive; this layer is the one that knows about the
    // machine the app is running on.
    if !result.reachable {
        if let Some(advice) = crate::vpn::explain_unreachable(&result.hostname).await {
            result.message = format!("{} {advice}", result.message);
        }
    }

    Ok(result)
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
        negotiated: live.negotiated.clone(),
        connected_at,
        shell_ids: Vec::new(),
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

/// Most shells one host may hold at once.
///
/// Each live terminal keeps an xterm instance with its own scrollback — a
/// megabyte or two of buffer plus a renderer context, and browsers cap live
/// WebGL contexts near sixteen. The ceiling is the renderer, not memory, so
/// the cap is deliberately well under it.
const MAX_SHELLS_PER_HOST: usize = 8;

/// Open an interactive shell. Output arrives as `terminal://output` events.
///
/// Opening does not replace anything: a host can hold several terminals, all
/// riding the one authenticated connection as separate channels. The returned
/// id addresses every later call — write, resize, close — and tags every event
/// so a pane renders only its own bytes.
///
/// There is deliberately no general "run this string" command alongside this
/// one. A terminal is an arbitrary-execution primitive the user is driving and
/// watching; a silent `run_command(String)` would be the same power with none
/// of the visibility.
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

    // Held across the check and the insert, so concurrent opens cannot both
    // see room under the cap and both take it.
    let _guard = live.lock_shell_open().await;

    if live.shell_count() >= MAX_SHELLS_PER_HOST {
        return Err(SshError::invalid(format!(
            "This host already has {MAX_SHELLS_PER_HOST} terminals open. \
             Close one before opening another."
        )));
    }

    // Output goes back to the window that asked for it, not to every window.
    let label = webview.label().to_string();

    let handle = shell::open(&live.session, app, label, host_id, cols, rows).await?;
    let shell_id = handle.id;
    live.add_shell(handle);

    Ok(shell_id)
}

/// Shell ids currently open on a host, oldest first.
///
/// Lets the UI rebuild its tab strip after a reload without opening anything
/// new — the shells outlive the window that created them.
#[tauri::command]
pub fn list_shells(registry: State<'_, SessionRegistry>, host_id: String) -> Vec<u64> {
    registry
        .get(&host_id)
        .map(|live| live.shell_ids())
        .unwrap_or_default()
}

/// Send keystrokes to one shell.
#[tauri::command]
pub async fn write_shell(
    registry: State<'_, SessionRegistry>,
    host_id: String,
    shell_id: u64,
    data: String,
) -> SshResult<()> {
    let live = registry.require(&host_id)?;
    let shell = live
        .shell(shell_id)
        .ok_or_else(|| SshError::invalid("That terminal has been closed."))?;

    shell.write(&data).await
}

/// Tell one shell its window changed.
#[tauri::command]
pub async fn resize_shell(
    registry: State<'_, SessionRegistry>,
    host_id: String,
    shell_id: u64,
    cols: u32,
    rows: u32,
) -> SshResult<()> {
    let Some(live) = registry.get(&host_id) else {
        return Ok(());
    };
    // A resize arriving after the shell closed is normal — a pane is still
    // laying out as it unmounts — so it is ignored rather than an error.
    if let Some(shell) = live.shell(shell_id) {
        shell.resize(cols, rows).await?;
    }
    Ok(())
}

/// Close one shell and drop it from the session.
///
/// Closing by id is what keeps tabs safe: a pane tearing down can only ever
/// close its own terminal, and an id that has already gone is a no-op rather
/// than an error. This is the only path that removes a map entry, so it is
/// also where the leak would be if it were missing.
#[tauri::command]
pub async fn close_shell(
    registry: State<'_, SessionRegistry>,
    host_id: String,
    shell_id: u64,
) -> SshResult<()> {
    let Some(live) = registry.get(&host_id) else {
        return Ok(());
    };

    if let Some(shell) = live.remove_shell(shell_id) {
        shell.close().await;
    }

    Ok(())
}

/// Close one long-running stream (a followed log) and drop it.
///
/// Closing by id is safe for the same reason `close_shell` is: a pane can
/// only ever close its own stream, and an id already gone is a no-op. There
/// is deliberately no matching generic *open*: each stream-producing feature
/// exposes its own typed command, so nothing here becomes a run-anything verb.
#[tauri::command]
pub async fn close_stream(
    registry: State<'_, SessionRegistry>,
    host_id: String,
    stream_id: u64,
) -> SshResult<()> {
    let Some(live) = registry.get(&host_id) else {
        return Ok(());
    };

    if let Some(stream) = live.remove_stream(stream_id) {
        stream.close().await;
    }

    Ok(())
}

/// List the services on a connected host.
#[tauri::command]
pub async fn list_services(
    registry: State<'_, SessionRegistry>,
    host_id: String,
) -> SshResult<Vec<ServiceEntry>> {
    let live = registry.require(&host_id)?;
    let command = services::list_command(live.os)?;
    let output = live.session.exec(command, None).await?;

    if !output.succeeded() {
        return Err(SshError::Io(format!(
            "Could not list services: {}",
            output.failure_text()
        )));
    }

    Ok(services::parse_list(live.os, &output.stdout))
}

/// The exact command a service action would run, without running it.
#[tauri::command]
pub fn preview_service_action(
    registry: State<'_, SessionRegistry>,
    host_id: String,
    request: ServiceActionRequest,
) -> SshResult<ServicePlan> {
    let live = registry.require(&host_id)?;
    services::plan_action(live.os, &live.elevation, &request)
}

/// Start, stop, or restart one service.
#[tauri::command]
pub async fn service_action(
    registry: State<'_, SessionRegistry>,
    vault: State<'_, SecretVault>,
    host_id: String,
    request: ServiceActionRequest,
    password: Option<String>,
) -> SshResult<ServiceOutcome> {
    let live = registry.require(&host_id)?;
    let plan = services::plan_action(live.os, &live.elevation, &request)?;

    // Same precedence as power: the dialog's password, then the one this
    // session logged in with, then the remembered one.
    let sudo_password: Option<Zeroizing<String>> = password
        .map(Zeroizing::new)
        .or_else(|| live.login_password())
        .or_else(|| vault.recall(&host_id));

    let stdin = if plan.needs_password {
        let password = sudo_password.ok_or_else(|| {
            SshError::invalid("This host needs your account password for sudo.")
        })?;
        Some(format!("{}\n", password.as_str()).into_bytes())
    } else {
        None
    };

    let output = live.session.exec(&plan.command, stdin.as_deref()).await?;
    Ok(services::interpret_action(&plan, output))
}

/// A service's recent history: the last journal lines, or SCM events.
///
/// `display_name` matters only on Windows, where SCM events name services by
/// display name rather than service name; the filter runs in Rust, never in
/// the remote query.
#[tauri::command]
pub async fn service_log(
    registry: State<'_, SessionRegistry>,
    host_id: String,
    unit: String,
    display_name: Option<String>,
) -> SshResult<ServiceLog> {
    let live = registry.require(&host_id)?;
    let command = services::log_command(live.os, &unit)?;
    let output = live.session.exec(&command, None).await?;

    let filter = match live.os {
        OsFamily::Windows => Some(display_name.as_deref().unwrap_or(unit.as_str())),
        _ => None,
    };

    Ok(services::parse_log(live.os, &output, filter))
}

/// Follow a service's journal. Output arrives as `stream://output` events
/// addressed to the calling window; the returned id is quoted back to
/// `close_stream` when the pane is done.
#[tauri::command]
pub async fn follow_service_log(
    app: AppHandle,
    webview: tauri::Webview,
    registry: State<'_, SessionRegistry>,
    host_id: String,
    unit: String,
) -> SshResult<u64> {
    let live = registry.require(&host_id)?;
    let command = services::follow_command(live.os, &unit)?;

    // Checked before the channel opens, so a refusal never leaves a command
    // already running remotely with nothing tracking it.
    if live.stream_count() >= super::registry::MAX_STREAMS_PER_HOST {
        return Err(SshError::invalid(
            "This host already has too many followed logs open. Close one before \
             opening another.",
        ));
    }

    // Addressed like terminal output, and for the same reason: a followed
    // log contains whatever the machine writes to it.
    let label = webview.label().to_string();

    let handle = stream::open(&live.session, app, label, host_id, &command).await?;
    let stream_id = handle.id;
    live.add_stream(handle);

    Ok(stream_id)
}

/// One performance sample from a connected host.
///
/// Called on a short timer by the Performance pane while it is visible, and
/// only then — the pane owns the cadence, so an unopened tab costs nothing.
#[tauri::command]
pub async fn sample_metrics(
    registry: State<'_, SessionRegistry>,
    host_id: String,
) -> SshResult<super::metrics::HostMetrics> {
    let live = registry.require(&host_id)?;
    super::metrics::sample(&live).await
}

/// What updates a host is waiting on. Read-only — there is no install verb.
#[tauri::command]
pub async fn check_updates(
    registry: State<'_, SessionRegistry>,
    host_id: String,
) -> SshResult<super::updates::UpdateReport> {
    use super::updates;

    let live = registry.require(&host_id)?;
    let command = updates::check_command(live.os)?;
    let output = live.session.exec(command, None).await?;

    match live.os {
        OsFamily::Windows => {
            let (module_present, history) = updates::parse_windows_first_round(&output);
            if !module_present {
                return Ok(updates::UpdateReport::ModuleMissing {
                    detail: updates::module_missing_detail(),
                    installed_history: history,
                });
            }
            // The module exists, so the real query is worth its slow round
            // trip — it goes out to Microsoft's servers.
            let pending = live
                .session
                .exec_with_timeout(
                    updates::windows_pending_command(),
                    None,
                    updates::WINDOWS_PENDING_TIMEOUT,
                )
                .await?;
            Ok(updates::parse_windows_pending(&pending))
        }
        _ => Ok(updates::parse_linux(&output)),
    }
}

/// Audit a connected host: tier 0 from the handshake, tier 1 from read-only
/// commands, tier 2 as Lynis detection only.
///
/// `password` follows the sudo chain and is used only for the privileged
/// retry when the unprivileged `sshd -T` was refused.
#[tauri::command]
pub async fn remote_audit(
    app: AppHandle,
    registry: State<'_, SessionRegistry>,
    vault: State<'_, SecretVault>,
    host_id: String,
    password: Option<String>,
) -> SshResult<super::audit::RemoteAuditReport> {
    use super::audit;

    let live = registry.require(&host_id)?;

    let tier1 = if live.os.is_unix() {
        let unprivileged = live.session.exec(audit::TIER1_COMMAND, None).await?;

        let privileged = if audit::needs_privileged_retry(&unprivileged)
            && live.elevation.is_usable()
            && !matches!(live.elevation, Elevation::WindowsAdminToken)
        {
            let sudo_password: Option<Zeroizing<String>> = password
                .map(Zeroizing::new)
                .or_else(|| live.login_password())
                .or_else(|| vault.recall(&host_id));

            let stdin = match &live.elevation {
                Elevation::SudoPassword => match sudo_password {
                    Some(password) => Some(format!("{}\n", password.as_str()).into_bytes()),
                    // No password to offer: skip the retry and let the note
                    // explain, rather than watching sudo fail.
                    None => None,
                },
                _ => Some(Vec::new()),
            };

            match stdin {
                Some(stdin) => Some(
                    live.session
                        .exec(audit::TIER1_PRIVILEGED_COMMAND, Some(&stdin))
                        .await?,
                ),
                None => None,
            }
        } else {
            None
        };

        Some(audit::gather_tier1(
            &unprivileged,
            privileged.as_ref(),
            live.elevation.is_usable(),
        ))
    } else {
        None
    };

    let suppressed = crate::ssh::store::Suppressions::read_named(
        &config_dir(&app)?,
        audit::SUPPRESSIONS_FILE,
    )
    .as_set();

    let mut report = audit::assemble(
        &host_id,
        live.negotiated.as_ref(),
        tier1.as_ref(),
        &suppressed,
    );

    // A Windows host runs no tier-1 commands; say so instead of showing an
    // unexplained half-report.
    if !live.os.is_unix() {
        report.tier1_note = Some(
            "Posture checks are implemented for Unix sshd; Windows posture checks \
             are planned."
                .to_string(),
        );
    }

    Ok(report)
}

/// Dismiss or restore one remote finding, per host.
#[tauri::command]
pub fn set_remote_finding_suppressed(
    app: AppHandle,
    host_id: String,
    finding_id: String,
    suppressed: bool,
) -> SshResult<()> {
    use super::audit;

    let dir = config_dir(&app)?;
    let mut store = crate::ssh::store::Suppressions::read_named(&dir, audit::SUPPRESSIONS_FILE);

    let key = format!("{host_id}|{finding_id}");
    if suppressed {
        store.insert(key);
    } else {
        store.remove(&key);
    }

    store.write_named(&dir, audit::SUPPRESSIONS_FILE)
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
