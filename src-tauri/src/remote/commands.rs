//! Tauri commands for reaching a remote machine.
//!
//! Passwords arrive as arguments and become `Zeroizing` immediately; they are
//! never persisted, logged, or interpolated into a command string.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, State};
use zeroize::Zeroizing;

use super::client::{Credentials, Session, Target};
use super::power::{self, Elevation, PowerOutcome, PowerRequest, PowerPlan, PrivilegeReport};
use super::jump;
use super::probe::{self, ProbeResult};
use super::registry::{LiveSession, SessionRegistry};
use super::secrets::SecretVault;
use super::services::{
    self, ServiceActionRequest, ServiceEntry, ServiceLog, ServiceOutcome, ServicePlan,
};
use super::sftp::{self, DirListing};
use super::transfer_task;
use super::transfers::{Direction, Priority, TransferManager, TransferRecord, TransferRequest};
use super::{shell, stream, OsFamily};
use crate::app_paths::config_dir;
use crate::hosts::model::{AuthMethod, HostRecord};
use crate::hosts::store::{now_iso8601, HostStore};
use crate::logging;
use crate::ssh::keys::{self, PassphraseNeed};
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
    /// Shells already open on this host - empty for a fresh connection.
    pub shell_ids: Vec<u64>,
    /// Whether the session holds the password it logged in with, for `sudo`.
    pub has_login_password: bool,
}

/// Check the port before spending time on a handshake. Needs no credentials.
#[tauri::command]
pub async fn probe_host(hostname: String, port: u16) -> SshResult<ProbeResult> {
    let mut result = probe::probe(hostname.trim(), port).await?;

    // Silence from a VPN address is usually the VPN's doing, not the host's.
    // Kept out of `probe` so that stays a pure network primitive.
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
/// `remember` keeps it for the rest of the app's run - see `secrets`.
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
    // Blinks the tray for as long as this call runs, however it ends.
    #[cfg(desktop)]
    let _connecting = crate::tray::ConnectingGuard::new(&app);

    // `Zeroizing` from here on, so the plaintext is wiped rather than left in
    // freed heap.
    let password = password.map(Zeroizing::new);
    let config_dir = config_dir(&app)?;
    let saved = HostStore::read(&config_dir).hosts;
    let host = saved
        .iter()
        .find(|host| host.id == host_id)
        .cloned()
        .ok_or_else(|| SshError::invalid("That connection no longer exists."))?;

    let credentials = build_credentials(&host, password.as_deref().map(|p| p.as_str()), &vault)?;

    let target = Target {
        hostname: host.hostname.clone(),
        port: host.port,
        username: host.username.clone(),
    };

    // Dialled outermost first; each hop tunnels through the one before it.
    let via = open_jump_chain(&saved, &host_id, &vault).await?;

    let attempt = match via {
        None => Session::connect(&target, &credentials, trust_unknown).await,
        Some(jump) => Session::connect_via(&target, &credentials, trust_unknown, jump).await,
    };

    let session = match attempt {
        Ok(session) => session,
        Err(error) => {
            // The target and the reason, never the credential.
            logging::warn(
                "ssh",
                format!("Connect to {}@{}:{} failed: {error}", target.username, target.hostname, target.port),
            );
            // Never replay a rejected password: repeated failures lock accounts.
            if host.auth_method == AuthMethod::Password {
                vault.forget(&host_id);
            }
            return Err(error);
        }
    };

    // The resolved credential, not the argument: on a reconnect the password
    // comes from the vault and the argument is `None`.
    let login_password = match &credentials {
        Credentials::Password(password) => Some(password.clone()),
        _ => None,
    };

    if remember {
        if let Some(password) = &login_password {
            vault.remember(&host_id, password.as_str());
        }
    }

    logging::info(
        "ssh",
        format!(
            "Connected to {}@{}:{} ({})",
            target.username,
            target.hostname,
            target.port,
            match host.auth_method {
                AuthMethod::Password => "password",
                AuthMethod::Publickey => "key",
                AuthMethod::Agent => "agent",
                AuthMethod::None => "none",
            }
        ),
    );

    // Read once at connect time: neither can change under a live session.
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

    // Held for the life of the session so `sudo` can reuse it, and dropped with
    // the session regardless of whether `remember` was set.
    if let Some(password) = login_password {
        live.set_login_password(password);
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
    app: AppHandle,
    registry: State<'_, SessionRegistry>,
    transfers: State<'_, Arc<TransferManager>>,
    host_id: String,
) -> SshResult<bool> {
    let disconnected = registry.disconnect(&host_id).await;
    if disconnected {
        logging::info("ssh", format!("Disconnected from host {host_id}"));
    }
    // After the session is gone, so a transfer cannot be promoted onto a
    // connection that is being torn down.
    release_host_transfers(&app, &registry, &transfers, &host_id).await;
    #[cfg(desktop)]
    crate::tray::refresh(&app);
    Ok(disconnected)
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

/// What the connect dialog must ask for before using this host's key.
///
/// Keyed by host id, not path, so the webview cannot probe arbitrary files.
/// Carries no key material.
#[tauri::command]
pub fn host_key_passphrase_need(app: AppHandle, host_id: String) -> SshResult<PassphraseNeed> {
    let host = HostStore::read(&config_dir(&app)?)
        .get(&host_id)
        .cloned()
        .ok_or_else(|| SshError::invalid("That connection no longer exists."))?;

    if host.auth_method != AuthMethod::Publickey {
        return Ok(PassphraseNeed::NotNeeded);
    }

    let Some(path) = host.key_path.as_deref() else {
        return Ok(PassphraseNeed::Unknown {
            detail: "This connection has no private key set. Edit it and choose one.".to_string(),
        });
    };

    Ok(keys::passphrase_need(&crate::ssh::paths::expand_tilde(path)))
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

    // Precedence: the dialog's password, the session's login password, then the
    // remembered one.
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

    logging::warn(
        "power",
        format!(
            "Host {host_id}: {:?} (delay {} min) - {}",
            request.action,
            request.delay_minutes,
            if outcome.succeeded { "accepted" } else { "refused" }
        ),
    );

    // An immediate shutdown or reboot takes the session with it.
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
    /// Whether the port answered - true for a reachable host we are not
    /// logged in to.
    pub reachable: bool,
    pub latency_ms: Option<u64>,
}

/// Short: every saved host is checked on one cycle, so a black-holed address
/// must not hold up the rest.
const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(3);

/// Check every saved host: are they up, and are our sessions still good?
///
/// Connected hosts get a round trip on the existing session, everything else a
/// TCP probe. Runs concurrently, so a cycle costs about one timeout in total.
#[tauri::command]
pub async fn heartbeat(
    app: AppHandle,
    registry: State<'_, SessionRegistry>,
    transfers: State<'_, Arc<TransferManager>>,
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

                // Session gone: fall through to a probe so the UI can still
                // tell "host up, session dropped" from "host down".
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

    // Reap sessions that failed their liveness check, and stop anything that
    // was transferring over them - otherwise a dropped link leaves a progress
    // bar creeping against a connection that no longer exists.
    for entry in &health {
        if !entry.connected && registry.is_connected(&entry.host_id) {
            registry.disconnect(&entry.host_id).await;
            release_host_transfers(&app, &registry, &transfers, &entry.host_id).await;
        }
    }

    // The 30-second tick is also how the tray notices a session that dropped
    // on its own, or the network coming back.
    #[cfg(desktop)]
    crate::tray::refresh(&app);

    Ok(health)
}

/// Most shells one host may hold at once. Bounded by live WebGL renderer
/// contexts (browsers cap those near sixteen), not by memory.
const MAX_SHELLS_PER_HOST: usize = 8;

/// Open an interactive shell. Output arrives as `terminal://output` events.
///
/// A host can hold several terminals as separate channels on the one
/// connection. The returned id addresses every later call and tags every event.
///
/// There is deliberately no general `run_command(String)` alongside this: a
/// terminal is arbitrary execution the user drives and watches.
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

    // Held across check and insert, so concurrent opens cannot both take the
    // last slot under the cap.
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

/// Shell ids currently open on a host, oldest first. Lets the UI rebuild its
/// tab strip after a reload; shells outlive the window that created them.
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
    // A resize arriving after the shell closed is normal (a pane still laying
    // out as it unmounts), so it is ignored rather than an error.
    if let Some(shell) = live.shell(shell_id) {
        shell.resize(cols, rows).await?;
    }
    Ok(())
}

/// Close one shell and drop it from the session. Closing by id means a pane can
/// only close its own terminal; an id already gone is a no-op, not an error.
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
/// Safe by id for the same reason as `close_shell`. There is deliberately no
/// generic *open*: each stream-producing feature exposes its own typed command.
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

    // Same precedence as power.
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
    let outcome = services::interpret_action(&plan, output);
    // The unit and the verdict. Never `outcome`'s text - that is remote output.
    logging::info(
        "services",
        format!(
            "Host {host_id}: {:?} {} - {}",
            request.action,
            request.unit,
            if outcome.succeeded { "ok" } else { "failed" }
        ),
    );
    Ok(outcome)
}

/// A service's recent history: the last journal lines, or SCM events.
///
/// `display_name` matters only on Windows, where SCM events name services by
/// display name; the filter runs in Rust, never in the remote query.
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

    // Checked before the channel opens, so a refusal never strands a command
    // running remotely with nothing tracking it.
    if live.stream_count() >= super::registry::MAX_STREAMS_PER_HOST {
        return Err(SshError::invalid(
            "This host already has too many followed logs open. Close one before \
             opening another.",
        ));
    }

    // Addressed to the calling window only, like terminal output.
    let label = webview.label().to_string();

    let handle = stream::open(&live.session, app, label, host_id, &command, None).await?;
    let stream_id = handle.id;
    live.add_stream(handle);

    Ok(stream_id)
}

/// One performance sample. The Performance pane owns the cadence and polls
/// only while visible.
#[tauri::command]
pub async fn sample_metrics(
    registry: State<'_, SessionRegistry>,
    host_id: String,
) -> SshResult<super::metrics::HostMetrics> {
    let live = registry.require(&host_id)?;
    super::metrics::sample(&live).await
}

/// What updates a host is waiting on. Read-only - there is no install verb.
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
            // Slow: this round trip goes out to Microsoft's servers.
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
/// commands.
///
/// `password` is used only for the privileged retry when the unprivileged
/// `sshd -T` was refused. `elevate: false` is honoured literally - no sudo
/// retry runs, even with a password the session already holds.
#[tauri::command]
pub async fn remote_audit(
    app: AppHandle,
    registry: State<'_, SessionRegistry>,
    vault: State<'_, SecretVault>,
    host_id: String,
    password: Option<String>,
    elevate: bool,
) -> SshResult<super::audit::RemoteAuditReport> {
    use super::audit;

    let live = registry.require(&host_id)?;
    let may_elevate = elevate && live.elevation.is_usable();

    let tier1 = if live.os.is_unix() {
        let unprivileged = live.session.exec(audit::TIER1_COMMAND, None).await?;

        let privileged = if audit::needs_privileged_retry(&unprivileged)
            && may_elevate
            && !matches!(live.elevation, Elevation::WindowsAdminToken)
        {
            let sudo_password: Option<Zeroizing<String>> = password
                .map(Zeroizing::new)
                .or_else(|| live.login_password())
                .or_else(|| vault.recall(&host_id));

            let stdin = match &live.elevation {
                // No password to offer: skip the retry, let the note explain.
                Elevation::SudoPassword => {
                    sudo_password.map(|password| format!("{}\n", password.as_str()).into_bytes())
                }
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
            may_elevate,
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

    // Declining is not the same as having no route to root, so the note must
    // not tell someone to configure sudo they already have.
    if live.os.is_unix() && !elevate && live.elevation.is_usable() && report.tier1_note.is_some() {
        report.tier1_note = Some(
            "The checks that need root were skipped: elevation was declined for this \
             run. Run the checks again and allow sudo to include them."
                .to_string(),
        );
    }

    // A Windows host runs no tier-1 commands; say so rather than show a
    // half-report.
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
/// Connect through every jump host in front of `host_id`, returning the last
/// session in the chain - the one the target is reached through.
///
/// A jump host is never prompted for: the dialog is about the target, so its
/// answer cannot stand in for another machine's key or password. An unknown
/// key or a missing password says to connect that host directly first, which
/// is where those questions can honestly be asked.
async fn open_jump_chain(
    saved: &[HostRecord],
    host_id: &str,
    vault: &SecretVault,
) -> SshResult<Option<Session>> {
    let chain = jump::resolve(saved, host_id)?;
    let mut via: Option<Session> = None;

    for hop in chain {
        let credentials = build_credentials(&hop, None, vault).map_err(|error| {
            SshError::invalid(format!(
                "Jump host “{}”: {error} Connect to it directly first.",
                hop.label
            ))
        })?;

        let target = Target {
            hostname: hop.hostname.clone(),
            port: hop.port,
            username: hop.username.clone(),
        };

        // `trust_unknown` is false at every hop, whatever was answered for the
        // target: that consent was about a different machine's key.
        let opened = match via {
            None => Session::connect(&target, &credentials, false).await,
            Some(previous) => Session::connect_via(&target, &credentials, false, previous).await,
        };

        via = Some(opened.map_err(|error| {
            logging::warn(
                "ssh",
                format!("Jump through {}@{}:{} failed", target.username, target.hostname, target.port),
            );
            SshError::invalid(format!("Jump host “{}”: {error}", hop.label))
        })?);
    }

    Ok(via)
}

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

        AuthMethod::None => Ok(Credentials::None),
    }
}

/* ── Files (SFTP) ─────────────────────────────────────────────────────── */

/// List one remote directory.
///
/// A failure drops the cached browse channel before returning: the usual cause
/// is a channel the server closed under us, and the next call should reopen
/// rather than fail forever.
#[tauri::command]
pub async fn list_remote_dir(
    registry: State<'_, SessionRegistry>,
    host_id: String,
    path: String,
) -> SshResult<DirListing> {
    let live = registry.require(&host_id)?;
    let sftp = live.browse.get_or_open(&live.session).await?;

    match sftp::list_dir(&sftp, &path).await {
        Ok(listing) => Ok(listing),
        Err(error) => {
            live.browse.reset().await;
            Err(error)
        }
    }
}

/// Where a fresh file browser should open.
#[tauri::command]
pub async fn remote_home_dir(
    registry: State<'_, SessionRegistry>,
    host_id: String,
) -> SshResult<String> {
    let live = registry.require(&host_id)?;
    let sftp = live.browse.get_or_open(&live.session).await?;

    match sftp::home_dir(&sftp).await {
        Ok(path) => Ok(path),
        Err(error) => {
            live.browse.reset().await;
            Err(error)
        }
    }
}

/// Create a directory on the remote host.
#[tauri::command]
pub async fn create_remote_dir(
    registry: State<'_, SessionRegistry>,
    host_id: String,
    path: String,
    name: String,
) -> SshResult<String> {
    // The name is the user's, but it still must not be a path: a "folder"
    // called `../..` would create somewhere they are not looking.
    sftp::safe_local_name(&name)?;

    let live = registry.require(&host_id)?;
    let sftp = live.browse.get_or_open(&live.session).await?;
    let target = sftp::join(&path, &name);

    sftp.create_dir(target.clone())
        .await
        .map_err(|error| sftp::explain_error(&format!("Could not create {target}"), &error.to_string()))?;

    Ok(target)
}

/// Delete one remote file or empty directory.
///
/// Refuses a symlink like everything else does - unlinking a link is safe in
/// itself, but the row the user clicked says "latest.log" and they would
/// reasonably expect the log to go. Refusing keeps the policy one sentence long.
#[tauri::command]
pub async fn delete_remote_entry(
    registry: State<'_, SessionRegistry>,
    host_id: String,
    path: String,
    is_dir: bool,
) -> SshResult<()> {
    let live = registry.require(&host_id)?;
    let sftp = live.browse.get_or_open(&live.session).await?;

    let metadata = sftp
        .symlink_metadata(path.clone())
        .await
        .map_err(|error| sftp::explain_error(&format!("Could not read {path}"), &error.to_string()))?;
    let kind = sftp::EntryKind::from(metadata.file_type());
    if let Some(refusal) = sftp::refuse_unless_regular(kind, &path) {
        return Err(refusal);
    }

    let result = if is_dir {
        sftp.remove_dir(path.clone()).await
    } else {
        sftp.remove_file(path.clone()).await
    };

    result.map_err(|error| sftp::explain_error(&format!("Could not delete {path}"), &error.to_string()))
}

/// Rename an entry, or move it - the same SFTP request either way, the only
/// difference being whether the destination's parent is the one it is in.
///
/// Refuses to act on a symlink, and refuses to land on anything that already
/// exists: SFTP's rename does not overwrite, and asking it to would make "move"
/// a way to destroy a file you did not name.
#[tauri::command]
pub async fn rename_remote_entry(
    registry: State<'_, SessionRegistry>,
    host_id: String,
    from: String,
    to: String,
) -> SshResult<String> {
    let live = registry.require(&host_id)?;
    let sftp = live.browse.get_or_open(&live.session).await?;

    let from = sftp::normalize(&from);
    let to = sftp::normalize(&to);
    if from == to {
        return Ok(to);
    }

    let source = sftp
        .symlink_metadata(from.clone())
        .await
        .map_err(|error| sftp::explain_error(&format!("Could not read {from}"), &error.to_string()))?;
    if let Some(refusal) = sftp::refuse_unless_regular(sftp::EntryKind::from(source.file_type()), &from) {
        return Err(refusal);
    }

    if sftp.try_exists(to.clone()).await.unwrap_or(false) {
        return Err(SshError::invalid(format!(
            "{to} already exists. Rename or remove it first."
        )));
    }

    sftp.rename(from.clone(), to.clone())
        .await
        .map_err(|error| sftp::explain_error(&format!("Could not move {from}"), &error.to_string()))?;

    Ok(to)
}

/// Copy an entry to another path on the *same* host.
///
/// Done by the server with `cp`/`Copy-Item` rather than by pulling the bytes
/// down and pushing them back: a copy that never crosses the network is both
/// far faster and does not depend on this machine staying awake. SFTP has no
/// copy request of its own, so this is the one file operation that needs a
/// shell - hence the quoting, which is the same helper the power and service
/// commands use.
#[tauri::command]
pub async fn copy_remote_entry(
    registry: State<'_, SessionRegistry>,
    host_id: String,
    from: String,
    to: String,
) -> SshResult<String> {
    let live = registry.require(&host_id)?;
    let sftp = live.browse.get_or_open(&live.session).await?;

    let from = sftp::normalize(&from);
    let to = sftp::normalize(&to);
    if from == to {
        return Err(SshError::invalid("A file cannot be copied over itself."));
    }

    let source = sftp
        .symlink_metadata(from.clone())
        .await
        .map_err(|error| sftp::explain_error(&format!("Could not read {from}"), &error.to_string()))?;
    let kind = sftp::EntryKind::from(source.file_type());
    if let Some(refusal) = sftp::refuse_unless_regular(kind, &from) {
        return Err(refusal);
    }

    // Copying a folder into itself would recurse until the disk filled.
    if kind == sftp::EntryKind::Dir && to.starts_with(&format!("{from}/")) {
        return Err(SshError::invalid(
            "A folder cannot be copied into itself.",
        ));
    }

    if sftp.try_exists(to.clone()).await.unwrap_or(false) {
        return Err(SshError::invalid(format!(
            "{to} already exists. Rename or remove it first."
        )));
    }

    let command = copy_command(live.os, &from, &to)?;
    // A large copy is disk-bound on the server and easily outlives the default
    // thirty seconds, so this is one of the few commands given real headroom.
    let output = live
        .session
        .exec_with_timeout(&command, None, Duration::from_secs(3600))
        .await?;

    if !output.succeeded() {
        return Err(sftp::explain_error(
            &format!("Could not copy {from}"),
            &output.failure_text(),
        ));
    }

    Ok(to)
}

/// The copy command for one remote OS. Pure, so the quoting is unit-tested.
fn copy_command(os: OsFamily, from: &str, to: &str) -> SshResult<String> {
    match os {
        // `-a` keeps modes and timestamps; `--` stops a path that begins with a
        // dash being read as a flag.
        os if os.is_unix() => Ok(format!(
            "cp -a -- {} {}",
            power::single_quote(from),
            power::single_quote(to)
        )),
        OsFamily::Windows => {
            // The wire form is `/C:/Users/...`; the shell wants `C:\Users\...`.
            let win = |path: &str| power::double_quote(&to_windows_path(path));
            Ok(format!(
                "powershell -NoProfile -NonInteractive -Command \
                 \"Copy-Item -LiteralPath {} -Destination {} -Recurse -Force\"",
                win(from),
                win(to)
            ))
        }
        OsFamily::Unknown => Err(SshError::unsupported(
            "This host's operating system could not be identified, so copying is not offered.",
        )),
        other => Err(SshError::unsupported(format!(
            "Copying is not supported on {}.",
            other.label()
        ))),
    }
}

/// `/C:/Users/me` → `C:\Users\me`, for a command line rather than the wire.
fn to_windows_path(path: &str) -> String {
    path.trim_start_matches('/').replace('/', "\\")
}

/// Every regular file under a folder, for a recursive transfer.
#[tauri::command]
pub async fn list_remote_tree(
    registry: State<'_, SessionRegistry>,
    host_id: String,
    path: String,
) -> SshResult<sftp::TreeListing> {
    let live = registry.require(&host_id)?;
    let sftp = live.browse.get_or_open(&live.session).await?;
    sftp::walk(&sftp, &path).await
}

/// What to do when a transfer's destination is already taken.
///
/// The caller decides, having asked the user; there is no "ask" here because
/// the backend has nobody to ask. Defaults to `KeepBoth`, so a caller that
/// forgets to choose cannot destroy anything.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OnConflict {
    #[default]
    KeepBoth,
    Overwrite,
}

/// Which of `names` already exist in a local folder, so the UI can ask before
/// anything is queued rather than after something is lost.
#[tauri::command]
pub fn local_conflicts(local_dir: String, names: Vec<String>) -> Vec<String> {
    let dir = PathBuf::from(local_dir);
    names
        .into_iter()
        .filter(|name| dir.join(name).exists())
        .collect()
}

/// The same question for a remote folder.
#[tauri::command]
pub async fn remote_conflicts(
    registry: State<'_, SessionRegistry>,
    host_id: String,
    remote_dir: String,
    names: Vec<String>,
) -> SshResult<Vec<String>> {
    let live = registry.require(&host_id)?;
    let sftp = live.browse.get_or_open(&live.session).await?;

    let mut taken = Vec::new();
    for name in names {
        let path = sftp::join(&remote_dir, &name);
        if sftp.try_exists(path).await.unwrap_or(false) {
            taken.push(name);
        }
    }
    Ok(taken)
}

/// Queue a download. Returns the transfer id; the file arrives via events.
///
/// `relative` places the file inside `local_dir`, so a recursive folder
/// transfer can mirror the tree. It comes from our own walk, but is sanitized
/// segment by segment anyway - a server-supplied path must never climb out of
/// the folder the user picked.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn enqueue_download(
    app: AppHandle,
    registry: State<'_, SessionRegistry>,
    transfers: State<'_, Arc<TransferManager>>,
    host_id: String,
    remote_path: String,
    local_dir: String,
    relative: Option<String>,
    on_conflict: Option<OnConflict>,
    priority: Option<Priority>,
) -> SshResult<u64> {
    registry.require(&host_id)?;

    let remote_path = sftp::normalize(&remote_path);
    let name = remote_path.rsplit('/').next().unwrap_or_default().to_string();
    // The server chose this name; it does not get to choose where it lands.
    let safe_name = sftp::safe_local_name(&name)?;

    let dir = PathBuf::from(&local_dir);
    if !dir.is_dir() {
        return Err(SshError::invalid(
            "Choose a folder that exists to download into.",
        ));
    }

    // Everything above the file name, each segment checked the same way.
    let mut target_dir = dir;
    if let Some(relative) = relative.as_deref() {
        for segment in relative.split('/').rev().skip(1).collect::<Vec<_>>().into_iter().rev() {
            if segment.is_empty() {
                continue;
            }
            target_dir = target_dir.join(sftp::safe_local_name(segment)?);
        }
    }

    let target = target_dir.join(&safe_name);
    let local_path = match on_conflict.unwrap_or_default() {
        OnConflict::Overwrite => target,
        OnConflict::KeepBoth => transfer_task::available_path(&target_dir, &safe_name),
    };

    let id = transfers.enqueue(TransferRequest {
        host_id: host_id.clone(),
        host_label: host_label(&app, &host_id),
        direction: Direction::Download,
        remote_path,
        local_path: local_path.to_string_lossy().to_string(),
        name: safe_name,
        priority: priority.unwrap_or_default(),
        bytes_total: None,
    });

    pump_transfers(&app, &registry, &transfers).await;
    Ok(id)
}

/// Queue an upload of a local file into a remote directory.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn enqueue_upload(
    app: AppHandle,
    registry: State<'_, SessionRegistry>,
    transfers: State<'_, Arc<TransferManager>>,
    host_id: String,
    local_path: String,
    remote_dir: String,
    on_conflict: Option<OnConflict>,
    priority: Option<Priority>,
) -> SshResult<u64> {
    let live = registry.require(&host_id)?;

    let source = PathBuf::from(&local_path);
    let metadata = std::fs::metadata(&source)
        .map_err(|error| SshError::io("Could not read the file you chose", error))?;
    if !metadata.is_file() {
        return Err(SshError::invalid(
            "Only regular files can be uploaded - pick a file, not a folder.",
        ));
    }

    let name = source
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .ok_or_else(|| SshError::invalid("That file has no name to upload it under."))?;

    // Uploading opens the destination with CREATE|TRUNCATE, so without this an
    // upload silently replaces whatever was already there.
    let remote_path = match on_conflict.unwrap_or_default() {
        OnConflict::Overwrite => sftp::join(&remote_dir, &name),
        OnConflict::KeepBoth => {
            let sftp = live.browse.get_or_open(&live.session).await?;
            available_remote_path(&sftp, &remote_dir, &name).await?
        }
    };

    let id = transfers.enqueue(TransferRequest {
        host_id: host_id.clone(),
        host_label: host_label(&app, &host_id),
        direction: Direction::Upload,
        remote_path,
        local_path,
        name,
        priority: priority.unwrap_or_default(),
        bytes_total: Some(metadata.len()),
    });

    pump_transfers(&app, &registry, &transfers).await;
    Ok(id)
}

/// A remote path that is free, disambiguating as `name (1).ext` - the same
/// scheme downloads use locally, so "keep both" means one thing in both
/// directions.
async fn available_remote_path(
    sftp: &russh_sftp::client::SftpSession,
    dir: &str,
    name: &str,
) -> SshResult<String> {
    let first = sftp::join(dir, name);
    if !sftp.try_exists(first.clone()).await.unwrap_or(false) {
        return Ok(first);
    }

    let (stem, extension) = match name.rfind('.') {
        // A leading dot is the whole name (`.bashrc`), not an extension.
        Some(index) if index > 0 => (&name[..index], &name[index..]),
        _ => (name, ""),
    };

    for index in 1..1000 {
        let candidate = sftp::join(dir, &format!("{stem} ({index}){extension}"));
        if !sftp.try_exists(candidate.clone()).await.unwrap_or(false) {
            return Ok(candidate);
        }
    }

    Err(SshError::invalid(format!(
        "There are already too many copies of {name} in that folder."
    )))
}

/// Every transfer the app knows about, newest first.
#[tauri::command]
pub fn list_transfers(transfers: State<'_, Arc<TransferManager>>) -> Vec<TransferRecord> {
    transfers.snapshot()
}

/// How many transfers are running, waiting, and how many slots there are.
#[tauri::command]
pub fn transfer_summary(transfers: State<'_, Arc<TransferManager>>) -> TransferSummary {
    let (running, queued) = transfers.pending_counts();
    TransferSummary {
        running,
        queued,
        max_concurrent: transfers.max_concurrent(),
    }
}

/// Stop a transfer. A queued one settles at once; a running one stops at its
/// next chunk. Freeing a slot immediately pumps the queue.
#[tauri::command]
pub async fn cancel_transfer(
    app: AppHandle,
    registry: State<'_, SessionRegistry>,
    transfers: State<'_, Arc<TransferManager>>,
    transfer_id: u64,
) -> SshResult<()> {
    transfers.cancel(transfer_id);
    transfer_task::emit_changed(&app);
    pump_transfers(&app, &registry, &transfers).await;
    Ok(())
}

/// Re-rank a waiting transfer.
#[tauri::command]
pub async fn set_transfer_priority(
    app: AppHandle,
    registry: State<'_, SessionRegistry>,
    transfers: State<'_, Arc<TransferManager>>,
    transfer_id: u64,
    priority: Priority,
) -> SshResult<()> {
    transfers.set_priority(transfer_id, priority);
    transfer_task::emit_changed(&app);
    // A bump cannot free a slot, but it can change who takes the next one.
    pump_transfers(&app, &registry, &transfers).await;
    Ok(())
}

/// Change how many transfers may run at once. Raising it starts more now.
#[tauri::command]
pub async fn set_max_concurrent_transfers(
    app: AppHandle,
    registry: State<'_, SessionRegistry>,
    transfers: State<'_, Arc<TransferManager>>,
    value: usize,
) -> SshResult<usize> {
    let applied = transfers.set_max_concurrent(value);
    pump_transfers(&app, &registry, &transfers).await;
    transfer_task::emit_changed(&app);
    Ok(applied)
}

/// Drop settled rows from the list. Pending ones stay.
#[tauri::command]
pub fn clear_finished_transfers(app: AppHandle, transfers: State<'_, Arc<TransferManager>>) -> usize {
    let cleared = transfers.clear_finished();
    transfer_task::emit_changed(&app);
    cleared
}

/// Start whatever the queue says should be running.
///
/// Called after every event that could free or claim a slot. The pump guard is
/// held across the spawns so two callers cannot both see the same free slot;
/// this is the one place a lock is deliberately held across an await, matching
/// `LiveSession::lock_shell_open`.
pub async fn pump_transfers(
    app: &AppHandle,
    registry: &SessionRegistry,
    transfers: &Arc<TransferManager>,
) {
    let _guard = transfers.lock_pump().await;

    let orders = transfers.take_ready(&|host_id| registry.is_connected(host_id));
    for order in orders {
        let Some(live) = registry.get(&order.host_id) else {
            // Disconnected between the check and here. Fail it rather than
            // spawn a task that can only fail on its first round trip.
            transfers.finish(order.id, Some("The host disconnected.".to_string()));
            continue;
        };

        tokio::spawn(transfer_task::run(
            app.clone(),
            Arc::clone(transfers),
            live,
            order.id,
            order.host_id,
            order.direction,
            order.remote_path,
            order.local_path,
            order.cancel,
        ));
    }

    transfer_task::emit_changed(app);
}

/// Fail everything belonging to a host that has gone away, then let the queue
/// hand the freed slots to whoever is still connected.
///
/// Lives here rather than on `SessionRegistry` so the registry keeps knowing
/// nothing about transfers - the two are joined at the command layer, not
/// wired into each other.
pub async fn release_host_transfers(
    app: &AppHandle,
    registry: &SessionRegistry,
    transfers: &Arc<TransferManager>,
    host_id: &str,
) {
    if transfers.fail_host(host_id, "The host disconnected.") > 0 {
        transfer_task::emit_changed(app);
    }
    pump_transfers(app, registry, transfers).await;
}

/// The host's display name, for a transfer row that must still make sense after
/// the connection is gone.
fn host_label(app: &AppHandle, host_id: &str) -> String {
    config_dir(app)
        .ok()
        .map(|dir| HostStore::read(&dir))
        .and_then(|store| {
            store
                .hosts
                .iter()
                .find(|host| host.id == host_id)
                .map(|host| host.label.clone())
        })
        .unwrap_or_else(|| host_id.to_string())
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferSummary {
    pub running: usize,
    pub queued: usize,
    pub max_concurrent: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_unix_copy_quotes_both_paths() {
        let command = copy_command(OsFamily::Linux, "/tmp/a b", "/tmp/c").unwrap();
        assert_eq!(command, "cp -a -- '/tmp/a b' '/tmp/c'");
    }

    #[test]
    fn a_hostile_path_cannot_break_out_of_its_quoting() {
        let command =
            copy_command(OsFamily::Linux, "/tmp/x'; rm -rf ~; echo '", "/tmp/y").unwrap();
        // The injection survives only as literal characters inside the quotes.
        assert!(command.starts_with("cp -a -- '/tmp/x'\\''; rm -rf ~; echo '"));
        assert!(!command.contains("; rm -rf ~; echo ' '/tmp/y'"));
    }

    #[test]
    fn a_windows_copy_uses_a_shell_path_not_the_wire_path() {
        let command = copy_command(OsFamily::Windows, "/C:/Users/me/a.txt", "/C:/tmp/a.txt")
            .unwrap();
        assert!(command.contains(r#""C:\Users\me\a.txt""#), "{command}");
        assert!(!command.contains("/C:/"), "{command}");
    }

    #[test]
    fn copying_is_refused_where_we_cannot_phrase_it() {
        assert!(copy_command(OsFamily::Unknown, "/a", "/b").is_err());
        let message = copy_command(OsFamily::Macos, "/a", "/b");
        // macOS is unix-like, so it takes the `cp` branch rather than failing.
        assert!(message.is_ok());
    }

    #[test]
    fn to_windows_path_drops_the_wire_prefix() {
        assert_eq!(to_windows_path("/C:/Users/me"), r"C:\Users\me");
        assert_eq!(to_windows_path("/tmp/x"), r"tmp\x");
    }

    fn host(auth_method: AuthMethod) -> HostRecord {
        HostRecord {
            id: "h-1".into(),
            label: "Test".into(),
            hostname: "example.test".into(),
            port: 22,
            username: "operator".into(),
            auth_method,
            key_path: None,
            group: String::new(),
            tags: Vec::new(),
            notes: None,
            proxy_jump: None,
            last_connected: None,
        }
    }

    fn password_of(credentials: &Credentials) -> Option<String> {
        match credentials {
            Credentials::Password(password) => Some(password.to_string()),
            _ => None,
        }
    }

    /// The invariant `connect_host` relies on to hand sudo a password: on a
    /// reconnect the UI sends nothing and the vault supplies the credential.
    /// Reading the argument instead of the resolved credential is what left a
    /// remembered-password session unable to elevate.
    #[test]
    fn a_reconnect_resolves_its_password_from_the_vault() {
        let vault = SecretVault::new();
        vault.remember("h-1", "from-the-vault");

        let credentials = build_credentials(&host(AuthMethod::Password), None, &vault).unwrap();
        assert_eq!(password_of(&credentials).as_deref(), Some("from-the-vault"));
    }

    #[test]
    fn a_supplied_password_wins_over_the_vault() {
        let vault = SecretVault::new();
        vault.remember("h-1", "stale");

        let credentials =
            build_credentials(&host(AuthMethod::Password), Some("typed-now"), &vault).unwrap();
        assert_eq!(password_of(&credentials).as_deref(), Some("typed-now"));
    }

    #[test]
    fn key_and_agent_carry_no_login_password() {
        let vault = SecretVault::new();
        vault.remember("h-1", "irrelevant");

        // A key's passphrase is not a login password and must never reach sudo.
        let mut keyed = host(AuthMethod::Publickey);
        keyed.key_path = Some("~/.ssh/id_ed25519".into());
        let credentials = build_credentials(&keyed, Some("passphrase"), &vault).unwrap();
        assert!(password_of(&credentials).is_none());

        let credentials = build_credentials(&host(AuthMethod::Agent), None, &vault).unwrap();
        assert!(password_of(&credentials).is_none());
    }

    /// Tailscale SSH authenticates the node over WireGuard and then offers only
    /// SSH's `none`. Without this, such a host cannot be connected to at all.
    #[test]
    fn none_auth_sends_no_credential_even_with_one_available() {
        let vault = SecretVault::new();
        vault.remember("h-1", "should-not-be-sent");

        let credentials = build_credentials(&host(AuthMethod::None), Some("typed"), &vault).unwrap();
        assert!(matches!(credentials, Credentials::None));
        assert!(password_of(&credentials).is_none());
    }

    #[test]
    fn password_auth_without_any_source_is_refused() {
        let vault = SecretVault::new();
        assert!(build_credentials(&host(AuthMethod::Password), None, &vault).is_err());
        assert!(build_credentials(&host(AuthMethod::Password), Some(""), &vault).is_err());
    }
}
