//! Listing, controlling, and reading the history of system services.
//!
//! Linux gets systemd (`systemctl` + `journalctl`), Windows the SCM (`sc
//! query`, `net start/stop`, `wevtutil`). macOS and BSD are refused rather than
//! guessed at: launchd and rc.d are too different for systemd phrasing to fit.
//!
//! Same shape as `power.rs`: command construction and output interpretation are
//! pure and unit-tested, elevation reuses the session's known route, and unit
//! names are validated then quoted with the power module's helpers.

use serde::{Deserialize, Serialize};

use super::power::{double_quote, single_quote, Elevation};
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
    /// Transitional or exotic states — start-pending, reloading, and friends.
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
    /// The service to act on — a systemd unit name or an SCM service name.
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
/// interpolated — the service-name filter runs in Rust, so no user input
/// reaches this query.
const WEVTUTIL_SCM_QUERY: &str = "wevtutil qe System \
    \"/q:*[System[Provider[@Name='Service Control Manager'] and \
    (EventID=7036 or EventID=7031 or EventID=7034)]]\" /c:100 /rd:true /f:text";

/// The one refusal message, shared so every entry point says the same thing.
fn unsupported_os(os: OsFamily) -> SshError {
    match os {
        OsFamily::Unknown => SshError::unsupported(
            "The remote operating system is unknown, so no service command can be \
             chosen safely.",
        ),
        _ => SshError::unsupported(format!(
            "Service management is built for systemd and the Windows service \
             manager; {} uses launchd/rc.d, which is not implemented yet.",
            os.label()
        )),
    }
}

/// The command that lists services on this OS. Pure.
pub fn list_command(os: OsFamily) -> SshResult<&'static str> {
    match os {
        // `--plain --no-legend` drops the `●` marker column and the header,
        // which is what makes the output parseable by position.
        OsFamily::Linux => {
            Ok("systemctl list-units --type=service --all --plain --no-legend --no-pager")
        }
        // `sc` is native; PowerShell's Get-Service costs a runtime startup.
        OsFamily::Windows => Ok("sc query type= service state= all"),
        other @ (OsFamily::Macos | OsFamily::Bsd | OsFamily::Unknown) => Err(unsupported_os(other)),
    }
}

/// Parse whichever list output this OS produces. Pure.
pub fn parse_list(os: OsFamily, stdout: &str) -> Vec<ServiceEntry> {
    match os {
        OsFamily::Windows => parse_sc_query(stdout),
        _ => parse_systemctl(stdout),
    }
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
            // `STATE              : 4  RUNNING` — the word is the last field.
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

/// Sequences that start a PowerShell substitution. Only these — not a bare `$`,
/// which is ordinary in a service name: SQL Server Express installs itself as
/// `MSSQL$SQLEXPRESS`.
const POWERSHELL_SUBSTITUTIONS: &[&str] = &["$(", "${", "`"];

/// Refuse names that could not be a real service before they reach a shell.
/// Spaces stay legal: Windows service names contain them.
///
/// The Windows rule exists because we cannot know which shell `sshd` uses.
/// `"…"` quotes identically in cmd.exe and PowerShell except for `$` and a
/// backtick, which PowerShell expands — so `$(…)` in a name would run against a
/// PowerShell `DefaultShell`. Refusing the substitution openers keeps one
/// quoting scheme correct on both, with no probe. A bare `$` stays legal: it
/// expands to nothing and `net` then rejects the truncated name, failing loudly
/// rather than hitting the wrong service.
fn validate_unit(os: OsFamily, unit: &str) -> SshResult<&str> {
    let unit = unit.trim();
    if unit.is_empty() {
        return Err(SshError::invalid("The service name is empty."));
    }
    if unit.chars().any(char::is_control) {
        return Err(SshError::invalid(
            "The service name contains control characters, which no real service has.",
        ));
    }
    if os == OsFamily::Windows {
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
    os: OsFamily,
    elevation: &Elevation,
    request: &ServiceActionRequest,
) -> SshResult<ServicePlan> {
    if let Elevation::Unavailable { reason } = elevation {
        return Err(SshError::invalid(format!(
            "This account cannot manage services: {reason}"
        )));
    }

    let unit = validate_unit(os, &request.unit)?;

    let command = match os {
        OsFamily::Linux => {
            let verb = match request.action {
                ServiceAction::Start => "start",
                ServiceAction::Stop => "stop",
                ServiceAction::Restart => "restart",
            };
            // `--` so a name starting with `-` reads as a name, not a flag.
            format!("systemctl {verb} -- {}", single_quote(unit))
        }
        OsFamily::Windows => {
            // `net` waits for the transition, so its exit status means
            // something; `sc start` returns before the service does.
            let quoted = double_quote(unit);
            match request.action {
                ServiceAction::Start => format!("net start {quoted}"),
                ServiceAction::Stop => format!("net stop {quoted}"),
                ServiceAction::Restart => format!("net stop {quoted} && net start {quoted}"),
            }
        }
        other @ (OsFamily::Macos | OsFamily::Bsd | OsFamily::Unknown) => return Err(unsupported_os(other)),
    };

    let needs_password = elevation.needs_password();
    let command = if os != OsFamily::Windows
        && matches!(elevation, Elevation::SudoPassword | Elevation::SudoNoPassword)
    {
        format!("sudo -S -p '' {command}")
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
        format!("{} — done.", plan.summary)
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

/// The one-shot history command for a service. Pure.
pub fn log_command(os: OsFamily, unit: &str) -> SshResult<String> {
    let unit = validate_unit(os, unit)?;
    match os {
        OsFamily::Linux => Ok(format!(
            "journalctl -u {} -n 200 --no-pager -o short-iso",
            single_quote(unit)
        )),
        // The query is a constant; the per-service filter happens in Rust.
        OsFamily::Windows => Ok(WEVTUTIL_SCM_QUERY.to_string()),
        other @ (OsFamily::Macos | OsFamily::Bsd | OsFamily::Unknown) => Err(unsupported_os(other)),
    }
}

/// The follow variant, for the streaming path. Linux only: the SCM event log
/// has no follow mode worth pretending about.
pub fn follow_command(os: OsFamily, unit: &str) -> SshResult<String> {
    let unit = validate_unit(os, unit)?;
    match os {
        OsFamily::Linux => Ok(format!(
            "journalctl -u {} -n 200 -f -o short-iso",
            single_quote(unit)
        )),
        OsFamily::Windows => Err(SshError::unsupported(
            "The Windows event log has no follow mode; refresh to see new events.",
        )),
        other @ (OsFamily::Macos | OsFamily::Bsd | OsFamily::Unknown) => Err(unsupported_os(other)),
    }
}

/// Turn log command output into what the pane shows. Pure. `filter` is the
/// display name a Windows event must mention to belong to the chosen service.
pub fn parse_log(os: OsFamily, output: &CommandOutput, filter: Option<&str>) -> ServiceLog {
    match os {
        OsFamily::Windows => parse_wevtutil(&output.stdout, filter),
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

    fn request(action: ServiceAction, unit: &str) -> ServiceActionRequest {
        ServiceActionRequest {
            action,
            unit: unit.to_string(),
        }
    }

    fn command_for(os: OsFamily, elevation: &Elevation, req: &ServiceActionRequest) -> String {
        plan_action(os, elevation, req).unwrap().command
    }

    #[test]
    fn linux_restart_wraps_sudo_and_quotes_the_unit() {
        let plan = plan_action(
            OsFamily::Linux,
            &Elevation::SudoPassword,
            &request(ServiceAction::Restart, "cron.service"),
        )
        .unwrap();
        assert_eq!(plan.command, "sudo -S -p '' systemctl restart -- 'cron.service'");
        assert!(plan.needs_password);

        // NOPASSWD keeps the prefix but drops the prompt.
        let quiet = plan_action(
            OsFamily::Linux,
            &Elevation::SudoNoPassword,
            &request(ServiceAction::Stop, "nginx.service"),
        )
        .unwrap();
        assert_eq!(quiet.command, "sudo -S -p '' systemctl stop -- 'nginx.service'");
        assert!(!quiet.needs_password);

        // Root runs it bare.
        assert_eq!(
            command_for(
                OsFamily::Linux,
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
            command_for(OsFamily::Windows, &token, &request(ServiceAction::Restart, "Spooler")),
            "net stop \"Spooler\" && net start \"Spooler\""
        );
        // Spaces are legal in Windows service names, so they must quote clean.
        assert_eq!(
            command_for(OsFamily::Windows, &token, &request(ServiceAction::Start, "Print Spooler")),
            "net start \"Print Spooler\""
        );
    }

    #[test]
    fn a_hostile_unit_name_cannot_break_out_of_its_quotes() {
        let hostile = request(ServiceAction::Stop, "x'; rm -rf /; echo '");
        let unix = command_for(OsFamily::Linux, &Elevation::NotNeeded, &hostile);
        // The `'` is closed-escaped-reopened, so the payload stays inert text.
        assert_eq!(unix, r"systemctl stop -- 'x'\''; rm -rf /; echo '\'''");

        let hostile = request(ServiceAction::Stop, "x\" & del C:\\ & \"");
        let windows = command_for(
            OsFamily::Windows,
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
                plan_action(OsFamily::Windows, &token, &request(ServiceAction::Stop, hostile))
                    .is_err(),
                "“{hostile}” should be refused, not quoted"
            );
        }

        // The same names are inert on Linux, which single-quotes properly, so
        // the rule must not leak across and refuse a legal unit there.
        assert!(plan_action(
            OsFamily::Linux,
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
                OsFamily::Windows,
                &Elevation::WindowsAdminToken,
                &request(ServiceAction::Restart, "MSSQL$SQLEXPRESS")
            ),
            "net stop \"MSSQL$SQLEXPRESS\" && net start \"MSSQL$SQLEXPRESS\""
        );
    }

    #[test]
    fn a_unit_name_with_a_newline_is_refused() {
        let sneaky = request(ServiceAction::Start, "cron\nreboot");
        assert!(plan_action(OsFamily::Linux, &Elevation::NotNeeded, &sneaky).is_err());
        assert!(plan_action(OsFamily::Linux, &Elevation::NotNeeded, &request(ServiceAction::Start, "")).is_err());
        assert!(plan_action(OsFamily::Linux, &Elevation::NotNeeded, &request(ServiceAction::Start, "  ")).is_err());
    }

    #[test]
    fn macos_is_refused_rather_than_guessed() {
        let req = request(ServiceAction::Start, "com.apple.something");
        assert!(plan_action(OsFamily::Macos, &Elevation::NotNeeded, &req).is_err());
        assert!(plan_action(OsFamily::Bsd, &Elevation::NotNeeded, &req).is_err());
        assert!(plan_action(OsFamily::Unknown, &Elevation::NotNeeded, &req).is_err());
        assert!(list_command(OsFamily::Macos).is_err());
        assert!(log_command(OsFamily::Bsd, "x").is_err());
    }

    #[test]
    fn parses_systemctl_plain_output() {
        let fixture = "\
cron.service      loaded active   running Regular background program processing daemon
nginx.service     loaded inactive dead    A high performance web server and a reverse proxy server
apparmor.service  loaded failed   failed  Load AppArmor profiles
ghost.service     not-found inactive dead ghost.service
";
        let entries = parse_list(OsFamily::Linux, fixture);
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
        let entries = parse_list(OsFamily::Linux, "\n \nnot a unit line\n");
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

        let entries = parse_list(OsFamily::Windows, fixture);
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
            log_command(OsFamily::Linux, "cron.service").unwrap(),
            "journalctl -u 'cron.service' -n 200 --no-pager -o short-iso"
        );
        assert_eq!(
            follow_command(OsFamily::Linux, "cron.service").unwrap(),
            "journalctl -u 'cron.service' -n 200 -f -o short-iso"
        );
        // The Windows query is a constant: nothing the user typed is in it.
        let windows = log_command(OsFamily::Windows, "anything' OR 1=1").unwrap();
        assert!(!windows.contains("anything"));
        assert!(follow_command(OsFamily::Windows, "Spooler").is_err());
    }

    #[test]
    fn journal_permission_hint_is_passed_through_as_a_note() {
        let output = CommandOutput {
            stdout: "-- No entries --\n".into(),
            stderr: "Hint: You are currently not seeing messages from other users and the system.".into(),
            exit_code: Some(0),
        };
        let log = parse_log(OsFamily::Linux, &output, None);
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

        let all = parse_log(OsFamily::Windows, &output, None);
        assert_eq!(all.lines.len(), 2);
        assert!(all.lines[0].starts_with("2026-07-20T10:15:30.123  [7036]  The Print Spooler"));

        // Multi-line description text is joined, and filtering is by the
        // display name the event mentions, case-insensitively.
        let filtered = parse_log(OsFamily::Windows, &output, Some("windows update"));
        assert_eq!(filtered.lines.len(), 1);
        assert!(filtered.lines[0].contains("terminated unexpectedly"));
    }

    #[test]
    fn action_outcomes_name_the_common_failures() {
        let plan = plan_action(
            OsFamily::Linux,
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
        assert_eq!(outcome.message, "Restart cron.service — done.");

        let denied = CommandOutput {
            stdout: "System error 5 has occurred.\r\n\r\nAccess is denied.\r\n".into(),
            stderr: String::new(),
            exit_code: Some(2),
        };
        let plan = plan_action(
            OsFamily::Windows,
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
            OsFamily::Windows,
            &blocked,
            &request(ServiceAction::Stop, "Spooler")
        )
        .is_err());
    }
}
