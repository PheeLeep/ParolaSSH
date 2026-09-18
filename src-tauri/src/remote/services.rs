//! Listing, controlling, and reading the history of system services.
//!
//! Linux speaks whichever manager `platform` found: systemd (`systemctl` +
//! `journalctl`), OpenRC (`rc-status`, `rc-service`) or SysV (`service`), the
//! last two reading history from syslog. Windows uses the SCM (`sc query`,
//! `net start/stop`, `wevtutil`). macOS, BSD and init-less containers are
//! refused with the reason rather than guessed at.
//!
//! Same shape as `power.rs`: command construction and output interpretation are
//! pure and unit-tested, elevation reuses the session's known route, and unit
//! names are validated then quoted with the power module's helpers.

use serde::{Deserialize, Serialize};

use super::platform::{InitSystem, Platform};
use super::power::{double_quote, sh_c, single_quote, sudo_sh, Elevation};
use super::{CommandOutput, OsFamily};
use crate::ssh::{SshError, SshResult};

/// One service, as the list shows it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceEntry {
    /// The name actions are addressed to: a systemd unit or an SCM service name.
    pub name: String,
    /// Human description: systemd's description column, Windows' display name.
    pub description: String,
    pub state: ServiceState,
    /// The raw state text, for the row's tooltip: `loaded/active/running` on
    /// Linux, the SCM state word on Windows.
    pub detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ServiceState {
    Running,
    Stopped,
    Failed,
    /// Transitional or exotic states - start-pending, reloading, and friends.
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServiceAction {
    Start,
    Stop,
    Restart,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceActionRequest {
    pub action: ServiceAction,
    /// The service to act on - a systemd unit name or an SCM service name.
    pub unit: String,
}

/// The exact command an action would run, shown before anything executes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServicePlan {
    pub command: String,
    pub needs_password: bool,
    pub summary: String,
}

/// The result of a service action, as the UI reports it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceOutcome {
    pub command: String,
    pub summary: String,
    pub succeeded: bool,
    pub message: String,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<u32>,
}

/// A service's recent history: journal lines or SCM events.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceLog {
    pub lines: Vec<String>,
    /// Anything the host said alongside the log, usually journald's hint that
    /// this account is not in `adm`/`systemd-journal`. Passed through because
    /// an empty log and a log we may not read are different answers.
    pub note: Option<String>,
}

/// SCM event ids worth showing: 7036 state changes, 7031/7034 crashes. Never
/// interpolated - the service-name filter runs in Rust, so no user input
/// reaches this query.
const WEVTUTIL_SCM_QUERY: &str = "wevtutil qe System \
    \"/q:*[System[Provider[@Name='Service Control Manager'] and \
    (EventID=7036 or EventID=7031 or EventID=7034)]]\" /c:100 /rd:true /f:text";

/// Which tool drives services on a connected host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceManager {
    Systemd,
    OpenRc,
    SysV,
    WindowsScm,
}

impl ServiceManager {
    /// Pick the manager for a host, or explain why it has none we can drive.
    pub fn for_host(os: OsFamily, platform: &Platform) -> SshResult<Self> {
        match os {
            OsFamily::Windows => Ok(Self::WindowsScm),
            OsFamily::Linux => match platform.init {
                InitSystem::Systemd => Ok(Self::Systemd),
                InitSystem::OpenRc => Ok(Self::OpenRc),
                InitSystem::SysV => Ok(Self::SysV),
                InitSystem::None | InitSystem::Native => Err(no_manager(platform)),
            },
            OsFamily::Unknown => Err(SshError::unsupported(
                "The remote operating system is unknown, so no service command can be \
                 chosen safely.",
            )),
            other => Err(SshError::unsupported(format!(
                "Service management covers systemd, OpenRC, SysV and the Windows service \
                 manager; {} uses launchd/rc.d, which is not implemented yet.",
                other.label()
            ))),
        }
    }

    /// Whether history comes from syslog rather than the manager itself.
    fn logs_to_syslog(self) -> bool {
        matches!(self, Self::OpenRc | Self::SysV)
    }
}

fn no_manager(platform: &Platform) -> SshError {
    match platform.container {
        Some(kind) => {
            let pid1 = if platform.pid1.is_empty() { "its main process".to_string() } else { format!("`{}`", platform.pid1) };
            SshError::unsupported(format!(
                "This is a {} container with no service manager: it runs {pid1} as PID 1, \
                 and the container runtime starts and stops it. Manage it from the machine \
                 running the container.",
                kind.label()
            ))
        }
        None => SshError::unsupported(
            "No supported service manager was found on this host - it runs neither \
             systemd, OpenRC nor SysV init scripts.",
        ),
    }
}

/// The command that lists services. Pure.
pub fn list_command(manager: ServiceManager) -> &'static str {
    match manager {
        // `--plain --no-legend` drops the `●` marker column and the header,
        // which is what makes the output parseable by position.
        ServiceManager::Systemd => {
            "systemctl list-units --type=service --all --plain --no-legend --no-pager"
        }
        // `--servicelist` covers every script, not only those in a runlevel.
        ServiceManager::OpenRc => "rc-status --servicelist --nocolor",
        // Some scripts answer `status` on stderr; the marker is what matters.
        ServiceManager::SysV => "service --status-all 2>&1",
        // `sc` is native; PowerShell's Get-Service costs a runtime startup.
        ServiceManager::WindowsScm => "sc query type= service state= all",
    }
}

/// Parse whichever list output this manager produces. Pure.
pub fn parse_list(manager: ServiceManager, stdout: &str) -> Vec<ServiceEntry> {
    match manager {
        ServiceManager::Systemd => parse_systemctl(stdout),
        ServiceManager::OpenRc => parse_rc_status(stdout),
        ServiceManager::SysV => parse_status_all(stdout),
        ServiceManager::WindowsScm => parse_sc_query(stdout),
    }
}

/// `rc-status --servicelist`: ` sshd   [  started  ]`, sometimes with an
/// uptime after the state word for supervised services.
fn parse_rc_status(stdout: &str) -> Vec<ServiceEntry> {
    stdout
        .lines()
        .filter_map(|line| {
            let line = strip_ansi(line);
            let (name, rest) = line.trim().split_once(char::is_whitespace)?;
            let inside = rest.trim().strip_prefix('[')?.split(']').next()?.trim();
            let word = inside.split_whitespace().next().unwrap_or("");
            let state = match word {
                "started" => ServiceState::Running,
                "stopped" => ServiceState::Stopped,
                "crashed" => ServiceState::Failed,
                _ => ServiceState::Other,
            };
            Some(ServiceEntry {
                name: name.to_string(),
                description: String::new(),
                state,
                detail: inside.to_string(),
            })
        })
        .collect()
}

/// `service --status-all`: ` [ + ]  ssh`, `-` for stopped, `?` for a script
/// with no status action.
fn parse_status_all(stdout: &str) -> Vec<ServiceEntry> {
    stdout
        .lines()
        .filter_map(|line| {
            let rest = line.trim().strip_prefix('[')?;
            let (mark, name) = rest.split_once(']')?;
            let name = name.trim();
            if name.is_empty() || name.contains(char::is_whitespace) {
                return None;
            }
            let (state, detail) = match mark.trim() {
                "+" => (ServiceState::Running, "running"),
                "-" => (ServiceState::Stopped, "stopped"),
                _ => (ServiceState::Other, "no status action"),
            };
            Some(ServiceEntry {
                name: name.to_string(),
                description: String::new(),
                state,
                detail: detail.to_string(),
            })
        })
        .collect()
}

/// Drop colour codes, in case `--nocolor` is ignored.
fn strip_ansi(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// `systemctl list-units --plain --no-legend`: four fields, then description.
fn parse_systemctl(stdout: &str) -> Vec<ServiceEntry> {
    stdout
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let name = fields.next()?.to_string();
            let load = fields.next()?;
            let active = fields.next()?;
            let sub = fields.next()?;
            let description = fields.collect::<Vec<_>>().join(" ");

            // Only unit rows: a stray footer or blank line has no `.service`.
            if !name.ends_with(".service") {
                return None;
            }

            let state = match (active, sub) {
                ("failed", _) | (_, "failed") => ServiceState::Failed,
                ("active", "running") => ServiceState::Running,
                ("inactive", _) => ServiceState::Stopped,
                ("active", _) => ServiceState::Other,
                _ => ServiceState::Other,
            };

            Some(ServiceEntry {
                name,
                description,
                state,
                detail: format!("{load}/{active}/{sub}"),
            })
        })
        .collect()
}

/// `sc query`: CRLF blocks keyed on `SERVICE_NAME:` / `DISPLAY_NAME:` / `STATE`.
/// The field labels are not localized; the state *word* is what we map.
fn parse_sc_query(stdout: &str) -> Vec<ServiceEntry> {
    let mut entries = Vec::new();
    let mut name: Option<String> = None;
    let mut display = String::new();
    let mut state = ServiceState::Other;
    let mut detail = String::new();

    let mut push = |name: &mut Option<String>, display: &mut String, state: ServiceState, detail: &mut String| {
        if let Some(name) = name.take() {
            entries.push(ServiceEntry {
                name,
                description: std::mem::take(display),
                state,
                detail: std::mem::take(detail),
            });
        }
    };

    for line in stdout.lines() {
        let line = line.trim_end_matches('\r');
        let trimmed = line.trim();

        if let Some(value) = trimmed.strip_prefix("SERVICE_NAME:") {
            // A new block: flush whatever the previous one collected.
            push(&mut name, &mut display, state, &mut detail);
            name = Some(value.trim().to_string());
            state = ServiceState::Other;
        } else if let Some(value) = trimmed.strip_prefix("DISPLAY_NAME:") {
            display = value.trim().to_string();
        } else if let Some(value) = trimmed.strip_prefix("STATE") {
            // `STATE              : 4  RUNNING` - the word is the last field.
            let word = value
                .rsplit(|c: char| c.is_whitespace())
                .next()
                .unwrap_or("")
                .to_string();
            state = match word.as_str() {
                "RUNNING" => ServiceState::Running,
                "STOPPED" => ServiceState::Stopped,
                _ => ServiceState::Other,
            };
            detail = word;
        }
    }
    push(&mut name, &mut display, state, &mut detail);

    entries
}

/// Sequences that start a PowerShell substitution. Only these - not a bare `$`,
/// which is ordinary in a service name: SQL Server Express installs itself as
/// `MSSQL$SQLEXPRESS`.
const POWERSHELL_SUBSTITUTIONS: &[&str] = &["$(", "${", "`"];

/// Refuse names that could not be a real service before they reach a shell.
/// Spaces stay legal: Windows service names contain them.
///
/// The Windows rule exists because we cannot know which shell `sshd` uses.
/// `"…"` quotes identically in cmd.exe and PowerShell except for `$` and a
/// backtick, which PowerShell expands - so `$(…)` in a name would run against a
/// PowerShell `DefaultShell`. Refusing the substitution openers keeps one
/// quoting scheme correct on both, with no probe. A bare `$` stays legal: it
/// expands to nothing and `net` then rejects the truncated name, failing loudly
/// rather than hitting the wrong service.
fn validate_unit(manager: ServiceManager, unit: &str) -> SshResult<&str> {
    let unit = unit.trim();
    if unit.is_empty() {
        return Err(SshError::invalid("The service name is empty."));
    }
    if unit.chars().any(char::is_control) {
        return Err(SshError::invalid(
            "The service name contains control characters, which no real service has.",
        ));
    }
    // `service` and `rc-service` take no `--`, and a script name has no slash.
    if manager.logs_to_syslog() && (unit.starts_with('-') || unit.contains('/')) {
        return Err(SshError::invalid(
            "An init script name cannot start with “-” or contain “/”.",
        ));
    }
    if manager == ServiceManager::WindowsScm {
        if let Some(found) = POWERSHELL_SUBSTITUTIONS
            .iter()
            .find(|opener| unit.contains(**opener))
        {
            return Err(SshError::invalid(format!(
                "The service name contains “{found}”, which some shells read as a \
                 command to run. No real service is named that, so this is refused \
                 rather than quoted."
            )));
        }
    }
    Ok(unit)
}

/// Build the command for an action. Pure, tested below.
pub fn plan_action(
    manager: ServiceManager,
    elevation: &Elevation,
    request: &ServiceActionRequest,
) -> SshResult<ServicePlan> {
    if let Elevation::Unavailable { reason } = elevation {
        return Err(SshError::invalid(format!(
            "This account cannot manage services: {reason}"
        )));
    }

    let unit = validate_unit(manager, &request.unit)?;
    let verb = match request.action {
        ServiceAction::Start => "start",
        ServiceAction::Stop => "stop",
        ServiceAction::Restart => "restart",
    };

    // The Unix form with the unit as `$1`, for wrapping in sudo.
    let script = match manager {
        // `--` so a name starting with `-` reads as a name, not a flag.
        ServiceManager::Systemd => format!("systemctl {verb} -- \"$1\""),
        ServiceManager::OpenRc => format!("rc-service \"$1\" {verb}"),
        ServiceManager::SysV => format!("service \"$1\" {verb}"),
        ServiceManager::WindowsScm => String::new(),
    };

    let command = match manager {
        ServiceManager::Systemd => format!("systemctl {verb} -- {}", single_quote(unit)),
        ServiceManager::OpenRc => format!("rc-service {} {verb}", single_quote(unit)),
        ServiceManager::SysV => format!("service {} {verb}", single_quote(unit)),
        ServiceManager::WindowsScm => {
            // `net` waits for the transition, so its exit status means
            // something; `sc start` returns before the service does.
            let quoted = double_quote(unit);
            match request.action {
                ServiceAction::Start => format!("net start {quoted}"),
                ServiceAction::Stop => format!("net stop {quoted}"),
                ServiceAction::Restart => format!("net stop {quoted} && net start {quoted}"),
            }
        }
    };

    let needs_password = elevation.needs_password();
    let command = if manager != ServiceManager::WindowsScm
        && matches!(elevation, Elevation::SudoPassword | Elevation::SudoNoPassword)
    {
        // The unit rides in as `$1` so it is quoted once, not nested inside
        // the script's own quotes.
        format!("{} sh {}", sudo_sh(&script), single_quote(unit))
    } else {
        command
    };

    let verb = match request.action {
        ServiceAction::Start => "Start",
        ServiceAction::Stop => "Stop",
        ServiceAction::Restart => "Restart",
    };

    Ok(ServicePlan {
        summary: format!("{verb} {unit}"),
        command,
        needs_password,
    })
}

/// Decide whether an action worked, from its output. Pure, fixture-tested.
pub fn interpret_action(plan: &ServicePlan, output: CommandOutput) -> ServiceOutcome {
    let succeeded = output.succeeded();

    let message = if succeeded {
        format!("{} - done.", plan.summary)
    } else {
        let text = output.failure_text();
        if text.contains("incorrect password") || text.contains("Sorry, try again") {
            "sudo rejected the password.".to_string()
        } else if text.contains("Access is denied") {
            "Windows refused the request: this account lacks the right to control \
             that service."
                .to_string()
        } else {
            text
        }
    };

    ServiceOutcome {
        command: plan.command.clone(),
        summary: plan.summary.clone(),
        succeeded,
        message,
        stdout: output.stdout,
        stderr: output.stderr,
        exit_code: output.exit_code,
    }
}

/// Picks the first readable system log into `$f`, or reports there is none.
/// Without a journal, lines naming the service in syslog are its history.
const SYSLOG_PICK: &str = "f=; for c in /var/log/syslog /var/log/messages; do \
    [ -r \"$c\" ] && f=$c && break; done; \
    [ -n \"$f\" ] || { echo NO_SYSLOG >&2; exit 0; }";

/// Stderr marker for "no readable syslog", turned into a note by `parse_log`.
const NO_SYSLOG: &str = "NO_SYSLOG";

/// The one-shot history command for a service. Pure.
pub fn log_command(manager: ServiceManager, unit: &str) -> SshResult<String> {
    let unit = validate_unit(manager, unit)?;
    Ok(match manager {
        ServiceManager::Systemd => format!(
            "journalctl -u {} -n 200 --no-pager -o short-iso",
            single_quote(unit)
        ),
        ServiceManager::OpenRc | ServiceManager::SysV => format!(
            "{} sh {}",
            sh_c(&format!("{SYSLOG_PICK}; grep -i -F -- \"$1\" \"$f\" | tail -n 200")),
            single_quote(unit)
        ),
        // The query is a constant; the per-service filter happens in Rust.
        ServiceManager::WindowsScm => WEVTUTIL_SCM_QUERY.to_string(),
    })
}

/// The follow variant, for the streaming path. The SCM event log has no
/// follow mode worth pretending about.
pub fn follow_command(manager: ServiceManager, unit: &str) -> SshResult<String> {
    let unit = validate_unit(manager, unit)?;
    match manager {
        ServiceManager::Systemd => Ok(format!(
            "journalctl -u {} -n 200 -f -o short-iso",
            single_quote(unit)
        )),
        ServiceManager::OpenRc | ServiceManager::SysV => Ok(format!(
            "{} sh {}",
            sh_c(&format!(
                "{SYSLOG_PICK}; tail -n 200 -F \"$f\" | grep --line-buffered -i -F -- \"$1\""
            )),
            single_quote(unit)
        )),
        ServiceManager::WindowsScm => Err(SshError::unsupported(
            "The Windows event log has no follow mode; refresh to see new events.",
        )),
    }
}

/// Turn log command output into what the pane shows. Pure. `filter` is the
/// display name a Windows event must mention to belong to the chosen service.
pub fn parse_log(manager: ServiceManager, output: &CommandOutput, filter: Option<&str>) -> ServiceLog {
    match manager {
        ServiceManager::WindowsScm => parse_wevtutil(&output.stdout, filter),
        _ if output.stderr.contains(NO_SYSLOG) => ServiceLog {
            lines: Vec::new(),
            note: Some(
                "This host has no journal and no readable /var/log/syslog or \
                 /var/log/messages, so there is no history to show. A container \
                 usually logs to its runtime instead - `docker logs <container>`."
                    .to_string(),
            ),
        },
        _ => {
            let lines = output
                .stdout
                .lines()
                .map(str::to_string)
                .filter(|line| !line.is_empty())
                .collect();
            let note = Some(output.stderr.trim())
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            ServiceLog { lines, note }
        }
    }
}

/// `wevtutil /f:text` blocks: `Event[n]:` then indented fields, with the
/// description spilling over following lines.
fn parse_wevtutil(stdout: &str, filter: Option<&str>) -> ServiceLog {
    let mut events: Vec<(String, String, String)> = Vec::new(); // date, id, description
    let mut current: Option<(String, String, Vec<String>)> = None;
    let mut in_description = false;

    let mut push = |current: &mut Option<(String, String, Vec<String>)>| {
        if let Some((date, id, description)) = current.take() {
            events.push((date, id, description.join(" ").trim().to_string()));
        }
    };

    for line in stdout.lines() {
        let line = line.trim_end_matches('\r');
        let trimmed = line.trim();

        if trimmed.starts_with("Event[") {
            push(&mut current);
            current = Some((String::new(), String::new(), Vec::new()));
            in_description = false;
        } else if let Some(entry) = current.as_mut() {
            if let Some(value) = trimmed.strip_prefix("Date:") {
                entry.0 = value.trim().to_string();
                in_description = false;
            } else if let Some(value) = trimmed.strip_prefix("Event ID:") {
                entry.1 = value.trim().to_string();
                in_description = false;
            } else if let Some(value) = trimmed.strip_prefix("Description:") {
                in_description = true;
                let value = value.trim();
                if !value.is_empty() {
                    entry.2.push(value.to_string());
                }
            } else if in_description {
                if trimmed.is_empty() {
                    continue;
                }
                // Field labels end the description; free text continues it.
                if is_wevtutil_field(trimmed) {
                    in_description = false;
                } else {
                    entry.2.push(trimmed.to_string());
                }
            }
        }
    }
    push(&mut current);

    let matches = |description: &str| match filter {
        Some(filter) => description.to_lowercase().contains(&filter.to_lowercase()),
        None => true,
    };

    let lines = events
        .into_iter()
        .filter(|(_, _, description)| matches(description))
        .map(|(date, id, description)| format!("{date}  [{id}]  {description}"))
        .collect();

    ServiceLog { lines, note: None }
}

/// The labels wevtutil prints, so description text containing a colon is not
/// mistaken for a field.
fn is_wevtutil_field(line: &str) -> bool {
    const FIELDS: &[&str] = &[
        "Log Name:", "Source:", "Date:", "Event ID:", "Task:", "Level:",
        "Opcode:", "Keyword:", "User:", "User Name:", "Computer:",
    ];
    FIELDS.iter().any(|field| line.starts_with(field))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::platform::ContainerKind;

    fn request(action: ServiceAction, unit: &str) -> ServiceActionRequest {
        ServiceActionRequest {
            action,
            unit: unit.to_string(),
        }
    }

    fn command_for(os: ServiceManager, elevation: &Elevation, req: &ServiceActionRequest) -> String {
        plan_action(os, elevation, req).unwrap().command
    }

    #[test]
    fn linux_restart_wraps_sudo_and_quotes_the_unit() {
        let plan = plan_action(
            ServiceManager::Systemd,
            &Elevation::SudoPassword,
            &request(ServiceAction::Restart, "cron.service"),
        )
        .unwrap();
        assert_eq!(
            plan.command,
            "sudo -S -p '' sh -c 'exec </dev/null; systemctl restart -- \"$1\"' sh 'cron.service'"
        );
        assert!(plan.needs_password);

        // NOPASSWD keeps the prefix but drops the prompt.
        let quiet = plan_action(
            ServiceManager::Systemd,
            &Elevation::SudoNoPassword,
            &request(ServiceAction::Stop, "nginx.service"),
        )
        .unwrap();
        assert_eq!(
            quiet.command,
            "sudo -S -p '' sh -c 'exec </dev/null; systemctl stop -- \"$1\"' sh 'nginx.service'"
        );
        assert!(!quiet.needs_password);

        // Root runs it bare.
        assert_eq!(
            command_for(
                ServiceManager::Systemd,
                &Elevation::NotNeeded,
                &request(ServiceAction::Start, "sshd.service")
            ),
            "systemctl start -- 'sshd.service'"
        );
    }

    #[test]
    fn windows_restart_chains_net_stop_and_start() {
        let token = Elevation::WindowsAdminToken;
        assert_eq!(
            command_for(ServiceManager::WindowsScm, &token, &request(ServiceAction::Restart, "Spooler")),
            "net stop \"Spooler\" && net start \"Spooler\""
        );
        // Spaces are legal in Windows service names, so they must quote clean.
        assert_eq!(
            command_for(ServiceManager::WindowsScm, &token, &request(ServiceAction::Start, "Print Spooler")),
            "net start \"Print Spooler\""
        );
    }

    #[test]
    fn a_hostile_unit_name_cannot_break_out_of_its_quotes() {
        let hostile = request(ServiceAction::Stop, "x'; rm -rf /; echo '");
        let unix = command_for(ServiceManager::Systemd, &Elevation::NotNeeded, &hostile);
        // The `'` is closed-escaped-reopened, so the payload stays inert text.
        assert_eq!(unix, r"systemctl stop -- 'x'\''; rm -rf /; echo '\'''");

        let hostile = request(ServiceAction::Stop, "x\" & del C:\\ & \"");
        let windows = command_for(
            ServiceManager::WindowsScm,
            &Elevation::WindowsAdminToken,
            &hostile,
        );
        // cmd.exe has no escape for an embedded quote; it is dropped instead.
        assert_eq!(windows, "net stop \"x' & del C:\\ & '\"");
    }

    /// `"..."` quotes identically in cmd.exe and PowerShell, so the only
    /// divergence is interpolation. Refusing the substitution openers keeps one
    /// quoting scheme correct on both without probing for the remote shell.
    #[test]
    fn windows_refuses_powershell_substitutions_in_a_unit_name() {
        let token = Elevation::WindowsAdminToken;
        for hostile in ["$(calc)", "Spooler$(whoami)", "${env:PATH}", "a`nb"] {
            assert!(
                plan_action(ServiceManager::WindowsScm, &token, &request(ServiceAction::Stop, hostile))
                    .is_err(),
                "“{hostile}” should be refused, not quoted"
            );
        }

        // The same names are inert on Linux, which single-quotes properly, so
        // the rule must not leak across and refuse a legal unit there.
        assert!(plan_action(
            ServiceManager::Systemd,
            &Elevation::NotNeeded,
            &request(ServiceAction::Stop, "weird$(name).service")
        )
        .is_ok());
    }

    /// A bare `$` is ordinary: SQL Server Express is literally `MSSQL$SQLEXPRESS`.
    /// Refusing it to be safe would break managing a real, common service.
    #[test]
    fn a_dollar_in_a_windows_service_name_stays_legal() {
        assert_eq!(
            command_for(
                ServiceManager::WindowsScm,
                &Elevation::WindowsAdminToken,
                &request(ServiceAction::Restart, "MSSQL$SQLEXPRESS")
            ),
            "net stop \"MSSQL$SQLEXPRESS\" && net start \"MSSQL$SQLEXPRESS\""
        );
    }

    #[test]
    fn a_unit_name_with_a_newline_is_refused() {
        let sneaky = request(ServiceAction::Start, "cron\nreboot");
        assert!(plan_action(ServiceManager::Systemd, &Elevation::NotNeeded, &sneaky).is_err());
        assert!(plan_action(ServiceManager::Systemd, &Elevation::NotNeeded, &request(ServiceAction::Start, "")).is_err());
        assert!(plan_action(ServiceManager::Systemd, &Elevation::NotNeeded, &request(ServiceAction::Start, "  ")).is_err());
    }

    #[test]
    fn macos_is_refused_rather_than_guessed() {
        let req = request(ServiceAction::Start, "com.apple.something");
        let _ = req;
        let native = Platform::native();
        for os in [OsFamily::Macos, OsFamily::Bsd, OsFamily::Unknown] {
            assert!(ServiceManager::for_host(os, &native).is_err(), "{os:?}");
        }
    }

    #[test]
    fn parses_systemctl_plain_output() {
        let fixture = "\
cron.service      loaded active   running Regular background program processing daemon
nginx.service     loaded inactive dead    A high performance web server and a reverse proxy server
apparmor.service  loaded failed   failed  Load AppArmor profiles
ghost.service     not-found inactive dead ghost.service
";
        let entries = parse_list(ServiceManager::Systemd, fixture);
        assert_eq!(entries.len(), 4);

        assert_eq!(entries[0].name, "cron.service");
        assert_eq!(entries[0].state, ServiceState::Running);
        assert_eq!(entries[0].detail, "loaded/active/running");
        assert_eq!(
            entries[0].description,
            "Regular background program processing daemon"
        );

        assert_eq!(entries[1].state, ServiceState::Stopped);
        assert_eq!(entries[2].state, ServiceState::Failed);
        assert_eq!(entries[3].state, ServiceState::Stopped);
        assert_eq!(entries[3].detail, "not-found/inactive/dead");
    }

    #[test]
    fn systemctl_parser_ignores_non_unit_lines() {
        let entries = parse_list(ServiceManager::Systemd, "\n \nnot a unit line\n");
        assert!(entries.is_empty());
    }

    #[test]
    fn parses_sc_query_blocks() {
        // CRLF and the indentation `sc` really prints.
        let fixture = "SERVICE_NAME: Spooler\r\n\
DISPLAY_NAME: Print Spooler\r\n\
        TYPE               : 110  WIN32_OWN_PROCESS (interactive)\r\n\
        STATE              : 4  RUNNING\r\n\
                                (STOPPABLE, NOT_PAUSABLE, IGNORES_SHUTDOWN)\r\n\
        WIN32_EXIT_CODE    : 0  (0x0)\r\n\
\r\n\
SERVICE_NAME: wuauserv\r\n\
DISPLAY_NAME: Windows Update\r\n\
        TYPE               : 20  WIN32_SHARE_PROCESS\r\n\
        STATE              : 1  STOPPED\r\n\
        WIN32_EXIT_CODE    : 0  (0x0)\r\n";

        let entries = parse_list(ServiceManager::WindowsScm, fixture);
        assert_eq!(entries.len(), 2);

        assert_eq!(entries[0].name, "Spooler");
        assert_eq!(entries[0].description, "Print Spooler");
        assert_eq!(entries[0].state, ServiceState::Running);
        assert_eq!(entries[0].detail, "RUNNING");

        assert_eq!(entries[1].name, "wuauserv");
        assert_eq!(entries[1].state, ServiceState::Stopped);
    }

    #[test]
    fn log_commands_quote_the_unit() {
        assert_eq!(
            log_command(ServiceManager::Systemd, "cron.service").unwrap(),
            "journalctl -u 'cron.service' -n 200 --no-pager -o short-iso"
        );
        assert_eq!(
            follow_command(ServiceManager::Systemd, "cron.service").unwrap(),
            "journalctl -u 'cron.service' -n 200 -f -o short-iso"
        );
        // The Windows query is a constant: nothing the user typed is in it.
        let windows = log_command(ServiceManager::WindowsScm, "anything' OR 1=1").unwrap();
        assert!(!windows.contains("anything"));
        assert!(follow_command(ServiceManager::WindowsScm, "Spooler").is_err());
    }

    #[test]
    fn journal_permission_hint_is_passed_through_as_a_note() {
        let output = CommandOutput {
            stdout: "-- No entries --\n".into(),
            stderr: "Hint: You are currently not seeing messages from other users and the system.".into(),
            exit_code: Some(0),
        };
        let log = parse_log(ServiceManager::Systemd, &output, None);
        assert_eq!(log.lines, vec!["-- No entries --"]);
        assert!(log.note.unwrap().contains("not seeing messages"));
    }

    #[test]
    fn wevtutil_events_are_parsed_and_filtered_by_display_name() {
        let fixture = "\
Event[0]:\r\n\
  Log Name: System\r\n\
  Source: Service Control Manager\r\n\
  Date: 2026-07-20T10:15:30.123\r\n\
  Event ID: 7036\r\n\
  Task: N/A\r\n\
  Level: Information\r\n\
  Computer: WIN-TEST\r\n\
  Description: \r\n\
The Print Spooler service entered the running state.\r\n\
\r\n\
Event[1]:\r\n\
  Log Name: System\r\n\
  Source: Service Control Manager\r\n\
  Date: 2026-07-19T08:00:01.000\r\n\
  Event ID: 7034\r\n\
  Task: N/A\r\n\
  Level: Error\r\n\
  Computer: WIN-TEST\r\n\
  Description: \r\n\
The Windows Update service terminated unexpectedly.  It has done this 1 time(s).\r\n";

        let output = CommandOutput {
            stdout: fixture.into(),
            stderr: String::new(),
            exit_code: Some(0),
        };

        let all = parse_log(ServiceManager::WindowsScm, &output, None);
        assert_eq!(all.lines.len(), 2);
        assert!(all.lines[0].starts_with("2026-07-20T10:15:30.123  [7036]  The Print Spooler"));

        // Multi-line description text is joined, and filtering is by the
        // display name the event mentions, case-insensitively.
        let filtered = parse_log(ServiceManager::WindowsScm, &output, Some("windows update"));
        assert_eq!(filtered.lines.len(), 1);
        assert!(filtered.lines[0].contains("terminated unexpectedly"));
    }

    #[test]
    fn action_outcomes_name_the_common_failures() {
        let plan = plan_action(
            ServiceManager::Systemd,
            &Elevation::SudoPassword,
            &request(ServiceAction::Restart, "cron.service"),
        )
        .unwrap();

        let refused = CommandOutput {
            stdout: String::new(),
            stderr: "sudo: 1 incorrect password attempt".into(),
            exit_code: Some(1),
        };
        let outcome = interpret_action(&plan, refused);
        assert!(!outcome.succeeded);
        assert_eq!(outcome.message, "sudo rejected the password.");

        let ok = CommandOutput {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: Some(0),
        };
        let outcome = interpret_action(&plan, ok);
        assert!(outcome.succeeded);
        assert_eq!(outcome.message, "Restart cron.service - done.");

        let denied = CommandOutput {
            stdout: "System error 5 has occurred.\r\n\r\nAccess is denied.\r\n".into(),
            stderr: String::new(),
            exit_code: Some(2),
        };
        let plan = plan_action(
            ServiceManager::WindowsScm,
            &Elevation::WindowsAdminToken,
            &request(ServiceAction::Stop, "Spooler"),
        )
        .unwrap();
        let outcome = interpret_action(&plan, denied);
        assert!(!outcome.succeeded);
        assert!(outcome.message.contains("right to control"));
    }

    #[test]
    fn an_unavailable_elevation_is_refused_before_anything_runs() {
        let blocked = Elevation::Unavailable {
            reason: "standard user".into(),
        };
        assert!(plan_action(
            ServiceManager::WindowsScm,
            &blocked,
            &request(ServiceAction::Stop, "Spooler")
        )
        .is_err());
    }

    fn platform(init: InitSystem, container: Option<ContainerKind>, pid1: &str) -> Platform {
        Platform { init, container, pid1: pid1.into(), has_shutdown: true }
    }

    #[test]
    fn linux_picks_the_manager_the_host_runs() {
        let pick = |init| ServiceManager::for_host(OsFamily::Linux, &platform(init, None, "init")).unwrap();
        assert_eq!(pick(InitSystem::Systemd), ServiceManager::Systemd);
        assert_eq!(pick(InitSystem::OpenRc), ServiceManager::OpenRc);
        assert_eq!(pick(InitSystem::SysV), ServiceManager::SysV);
        assert_eq!(
            ServiceManager::for_host(OsFamily::Windows, &Platform::native()).unwrap(),
            ServiceManager::WindowsScm
        );
    }

    #[test]
    fn an_init_less_container_explains_itself() {
        let bare = platform(InitSystem::None, Some(ContainerKind::Docker), "node");
        let message = ServiceManager::for_host(OsFamily::Linux, &bare).unwrap_err().to_string();
        assert!(message.contains("Docker container with no service manager"), "{message}");
        assert!(message.contains("`node` as PID 1"), "{message}");

        let odd = platform(InitSystem::None, None, "");
        let message = ServiceManager::for_host(OsFamily::Linux, &odd).unwrap_err().to_string();
        assert!(message.contains("neither systemd, OpenRC nor SysV"), "{message}");
    }

    #[test]
    fn parses_service_status_all() {
        // Captured from a Kali rolling container.
        let fixture = " [ - ]  apparmor\n [ ? ]  hwclock.sh\n [ + ]  ssh\n [ - ]  procps\nnot a line\n";
        let entries = parse_list(ServiceManager::SysV, fixture);
        let summary: Vec<_> = entries.iter().map(|e| (e.name.as_str(), e.state)).collect();
        assert_eq!(
            summary,
            [
                ("apparmor", ServiceState::Stopped),
                ("hwclock.sh", ServiceState::Other),
                ("ssh", ServiceState::Running),
                ("procps", ServiceState::Stopped),
            ]
        );
    }

    #[test]
    fn parses_rc_status_servicelist() {
        // Alpine's layout, colour codes included in case `--nocolor` is ignored.
        let fixture = " sshd                    [  started  ]\n \
 crond                   [  stopped  ]\n \
 \u{1b}[1mnginx\u{1b}[0m                   [  crashed  ]\n \
 dbus                    [  started 00:12:04 (0)  ]\n";
        let entries = parse_list(ServiceManager::OpenRc, fixture);
        let summary: Vec<_> = entries.iter().map(|e| (e.name.as_str(), e.state)).collect();
        assert_eq!(
            summary,
            [
                ("sshd", ServiceState::Running),
                ("crond", ServiceState::Stopped),
                ("nginx", ServiceState::Failed),
                ("dbus", ServiceState::Running),
            ]
        );
        assert_eq!(entries[3].detail, "started 00:12:04 (0)");
    }

    #[test]
    fn sysv_and_openrc_actions_wrap_sudo_like_systemd() {
        let restart = request(ServiceAction::Restart, "ssh");
        assert_eq!(command_for(ServiceManager::SysV, &Elevation::NotNeeded, &restart), "service 'ssh' restart");
        assert_eq!(command_for(ServiceManager::OpenRc, &Elevation::NotNeeded, &restart), "rc-service 'ssh' restart");
        assert_eq!(
            command_for(ServiceManager::SysV, &Elevation::SudoPassword, &restart),
            "sudo -S -p '' sh -c 'exec </dev/null; service \"$1\" restart' sh 'ssh'"
        );
    }

    #[test]
    fn init_script_names_cannot_pose_as_flags_or_paths() {
        for name in ["--status-all", "../../bin/sh", "/etc/init.d/ssh"] {
            let hostile = request(ServiceAction::Stop, name);
            assert!(plan_action(ServiceManager::SysV, &Elevation::NotNeeded, &hostile).is_err(), "{name}");
            assert!(plan_action(ServiceManager::OpenRc, &Elevation::NotNeeded, &hostile).is_err(), "{name}");
        }
    }

    #[test]
    fn without_a_journal_history_comes_from_syslog() {
        let command = log_command(ServiceManager::SysV, "ssh").unwrap();
        assert!(command.contains("/var/log/syslog /var/log/messages"), "{command}");
        assert!(command.ends_with("sh 'ssh'"), "{command}");
        let follow = follow_command(ServiceManager::OpenRc, "sshd").unwrap();
        assert!(follow.contains("tail -n 200 -F"), "{follow}");

        let missing = CommandOutput { stdout: String::new(), stderr: "NO_SYSLOG\n".into(), exit_code: Some(0) };
        let log = parse_log(ServiceManager::SysV, &missing, None);
        assert!(log.lines.is_empty());
        assert!(log.note.unwrap().contains("docker logs"));
    }
}
