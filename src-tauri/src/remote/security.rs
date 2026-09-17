//! Read-only security views: listening ports, firewall state, logged-in users.
//!
//! Same shape as `services.rs`: command builders and parsers are pure and
//! unit-tested. Unix scripts run under `sh -c` so a fish or csh login shell
//! cannot misread them; Windows scripts go through `-EncodedCommand` so `$_`
//! survives whichever default shell sshd was given.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use super::power::{sh_c, sudo_sh, Elevation};
use super::{CommandOutput, OsFamily};
use crate::ssh::{SshError, SshResult};

const MARKER_PREFIX: &str = "---PAROLA:";
const MARKER_SUFFIX: &str = "---";

/// Which elevated view a preview is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SecurityView {
    Ports,
    Firewall,
}

/* ── Ports ─────────────────────────────────────────────────────────────── */

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListeningPort {
    /// `tcp` or `udp`.
    pub proto: String,
    pub address: String,
    pub port: u16,
    pub pid: Option<u32>,
    pub process: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortsReport {
    pub ports: Vec<ListeningPort>,
    /// The tool that answered: `ss`, `lsof`, `Get-NetTCPConnection`, ...
    pub tool: String,
    pub command: String,
    pub elevated: bool,
    pub note: Option<String>,
}

/* ── Firewall ──────────────────────────────────────────────────────────── */

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FirewallState {
    Active,
    Inactive,
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FirewallBackend {
    pub name: String,
    pub state: FirewallState,
    pub summary: Option<String>,
    /// Raw output, shown as-is.
    pub output: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FirewallReport {
    pub backends: Vec<FirewallBackend>,
    pub command: String,
    pub elevated: bool,
    pub note: Option<String>,
}

/* ── Users ─────────────────────────────────────────────────────────────── */

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoggedInUser {
    pub user: String,
    /// A tty on Unix, a session name on Windows.
    pub terminal: Option<String>,
    pub from: Option<String>,
    pub login_time: Option<String>,
    /// Windows only: Active / Disc.
    pub state: Option<String>,
    /// Windows only.
    pub idle: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsersReport {
    pub users: Vec<LoggedInUser>,
    pub note: Option<String>,
}

/* ── Commands ──────────────────────────────────────────────────────────── */

const UNIX_PORTS_SCRIPT: &str = "PATH=\"$PATH:/usr/sbin:/sbin\"; \
    if command -v ss >/dev/null 2>&1; then echo ---PAROLA:ss---; ss -tulnp; \
    elif command -v sockstat >/dev/null 2>&1; then echo ---PAROLA:sockstat---; sockstat -46l; \
    elif command -v lsof >/dev/null 2>&1; then echo ---PAROLA:lsof---; lsof -nP -iTCP -sTCP:LISTEN -iUDP; \
    elif command -v netstat >/dev/null 2>&1; then echo ---PAROLA:netstat---; netstat -tulnp; \
    else echo ---PAROLA:none---; fi; true";

const UNIX_FIREWALL_SCRIPT: &str = "PATH=\"$PATH:/usr/sbin:/sbin\"; \
    if command -v ufw >/dev/null 2>&1; then echo ---PAROLA:ufw---; ufw status verbose 2>&1; fi; \
    if command -v firewall-cmd >/dev/null 2>&1; then echo ---PAROLA:firewalld---; firewall-cmd --state 2>&1 && firewall-cmd --list-all 2>&1; fi; \
    if command -v nft >/dev/null 2>&1; then echo ---PAROLA:nftables---; nft list ruleset 2>&1; fi; \
    if command -v iptables >/dev/null 2>&1; then echo ---PAROLA:iptables---; iptables -S 2>&1; fi; \
    if [ -x /usr/libexec/ApplicationFirewall/socketfilterfw ]; then echo ---PAROLA:appfw---; \
    /usr/libexec/ApplicationFirewall/socketfilterfw --getglobalstate 2>&1; \
    /usr/libexec/ApplicationFirewall/socketfilterfw --getstealthmode 2>&1; fi; \
    if command -v pfctl >/dev/null 2>&1; then echo ---PAROLA:pf---; pfctl -s info 2>&1 | head -n 1; pfctl -s rules 2>&1; fi; \
    if command -v ipfw >/dev/null 2>&1; then echo ---PAROLA:ipfw---; ipfw list 2>&1; fi; true";

const WINDOWS_PORTS_SCRIPT: &str = "$ErrorActionPreference='SilentlyContinue'; \
    $n=@{}; Get-Process | ForEach-Object { $n[[int]$_.Id]=$_.ProcessName }; $r=@(); \
    $r+=Get-NetTCPConnection -State Listen | ForEach-Object { [pscustomobject]@{proto='tcp';address=[string]$_.LocalAddress;port=[int]$_.LocalPort;pid=[int]$_.OwningProcess;process=$n[[int]$_.OwningProcess]} }; \
    $r+=Get-NetUDPEndpoint | ForEach-Object { [pscustomobject]@{proto='udp';address=[string]$_.LocalAddress;port=[int]$_.LocalPort;pid=[int]$_.OwningProcess;process=$n[[int]$_.OwningProcess]} }; \
    ConvertTo-Json -Compress -InputObject @($r)";

const WINDOWS_FIREWALL_SCRIPT: &str = "$ErrorActionPreference='Stop'; \
    $p=@(Get-NetFirewallProfile | ForEach-Object { [pscustomobject]@{name=[string]$_.Name;enabled=[string]$_.Enabled;inbound=[string]$_.DefaultInboundAction;outbound=[string]$_.DefaultOutboundAction} }); \
    $r=@(Get-NetFirewallRule -Enabled True -Direction Inbound | ForEach-Object { [pscustomobject]@{name=[string]$_.DisplayName;action=[string]$_.Action;profile=[string]$_.Profile} }); \
    ConvertTo-Json -Compress -Depth 3 -InputObject @{profiles=$p;rules=$r}";

fn unsupported(os: OsFamily, what: &str) -> SshError {
    SshError::unsupported(format!(
        "The remote operating system is {}, so no {what} command can be chosen safely.",
        os.label().to_lowercase()
    ))
}

/// Whether this request actually goes through sudo.
pub fn uses_sudo(elevation: &Elevation, elevate: bool) -> bool {
    elevate && matches!(elevation, Elevation::SudoPassword | Elevation::SudoNoPassword)
}

/// A Unix script as one `sh -c` command, behind sudo when elevating. Pure.
fn unix_command(script: &str, elevation: &Elevation, elevate: bool) -> String {
    if uses_sudo(elevation, elevate) {
        sudo_sh(script)
    } else {
        sh_c(script)
    }
}

/// `powershell -EncodedCommand`: base64 of the UTF-16LE script. Pure.
fn powershell_encoded(script: &str) -> String {
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    format!(
        "powershell -NoProfile -NonInteractive -EncodedCommand {}",
        base64(&bytes)
    )
}

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { TABLE[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { TABLE[n as usize & 63] as char } else { '=' });
    }
    out
}

pub fn ports_command(os: OsFamily, elevation: &Elevation, elevate: bool) -> SshResult<String> {
    match os {
        os if os.is_unix() => Ok(unix_command(UNIX_PORTS_SCRIPT, elevation, elevate)),
        OsFamily::Windows => Ok(powershell_encoded(WINDOWS_PORTS_SCRIPT)),
        other => Err(unsupported(other, "port listing")),
    }
}

pub fn firewall_command(os: OsFamily, elevation: &Elevation, elevate: bool) -> SshResult<String> {
    match os {
        os if os.is_unix() => Ok(unix_command(UNIX_FIREWALL_SCRIPT, elevation, elevate)),
        OsFamily::Windows => Ok(powershell_encoded(WINDOWS_FIREWALL_SCRIPT)),
        other => Err(unsupported(other, "firewall")),
    }
}

pub fn users_command(os: OsFamily) -> SshResult<&'static str> {
    match os {
        os if os.is_unix() => Ok("who"),
        OsFamily::Windows => Ok("quser"),
        other => Err(unsupported(other, "session listing")),
    }
}

/// The literal command an elevated view would run, for the consent prompt.
pub fn preview(os: OsFamily, elevation: &Elevation, view: SecurityView) -> SshResult<String> {
    match view {
        SecurityView::Ports => ports_command(os, elevation, true),
        SecurityView::Firewall => firewall_command(os, elevation, true),
    }
}

/* ── Shared parsing ────────────────────────────────────────────────────── */

/// Split marker-delimited output into `(name, body)` sections.
fn sections(stdout: &str) -> Vec<(&str, String)> {
    let mut out: Vec<(&str, String)> = Vec::new();
    for line in stdout.lines() {
        let trimmed = line.trim_end_matches('\r');
        if let Some(name) = trimmed
            .strip_prefix(MARKER_PREFIX)
            .and_then(|rest| rest.strip_suffix(MARKER_SUFFIX))
        {
            out.push((name, String::new()));
        } else if let Some((_, body)) = out.last_mut() {
            body.push_str(trimmed);
            body.push('\n');
        }
    }
    out
}

/// sudo refusing (wrong password, not in sudoers) prints no marker at all.
fn sudo_failure(output: &CommandOutput, elevated: bool) -> Option<SshError> {
    (elevated && !output.stdout.contains(MARKER_PREFIX)).then(|| {
        SshError::Io(format!("sudo did not run the command: {}", output.failure_text()))
    })
}

/// `127.0.0.1:22`, `[::]:80`, `*:53`, `[fe80::1]%eth0:546` -> address, port.
fn split_host_port(value: &str) -> Option<(String, u16)> {
    let (host, port) = value.rsplit_once(':')?;
    let port = port.parse().ok()?;
    let host = host.replace(['[', ']'], "");
    Some((if host.is_empty() { "*".to_string() } else { host }, port))
}

fn base_proto(value: &str) -> Option<String> {
    let lower = value.to_ascii_lowercase();
    if lower.starts_with("tcp") {
        Some("tcp".to_string())
    } else if lower.starts_with("udp") {
        Some("udp".to_string())
    } else {
        None
    }
}

fn denied(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    [
        "permission denied",
        "operation not permitted",
        "need to be root",
        "must be root",
        "authorization failed",
        "not authorized",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

/* ── Ports parsing ─────────────────────────────────────────────────────── */

pub fn parse_ports(
    os: OsFamily,
    output: &CommandOutput,
    command: String,
    elevated: bool,
) -> SshResult<PortsReport> {
    if os == OsFamily::Windows {
        return parse_windows_ports(output, command);
    }
    if let Some(error) = sudo_failure(output, elevated) {
        return Err(error);
    }

    let (tool, body) = sections(&output.stdout)
        .into_iter()
        .next()
        .ok_or_else(|| SshError::Io(format!("Could not list ports: {}", output.failure_text())))?;

    let mut ports = match tool {
        "ss" => parse_ss(&body),
        "sockstat" => parse_sockstat(&body),
        "lsof" => parse_lsof(&body),
        "netstat" => parse_netstat(&body),
        _ => {
            return Err(SshError::unsupported(
                "None of ss, sockstat, lsof, or netstat is installed on this host.",
            ))
        }
    };
    dedupe_and_sort(&mut ports);

    let note = if elevated {
        None
    } else if tool == "lsof" {
        Some("Without root, lsof lists only this account's sockets. Read again with sudo to see all of them.".to_string())
    } else if ports.iter().any(|port| port.process.is_none()) {
        Some("Process names for sockets owned by other accounts need root.".to_string())
    } else {
        None
    };

    Ok(PortsReport {
        ports,
        tool: tool.to_string(),
        command,
        elevated,
        note,
    })
}

fn dedupe_and_sort(ports: &mut Vec<ListeningPort>) {
    let mut seen = HashSet::new();
    ports.retain(|port| seen.insert((port.proto.clone(), port.address.clone(), port.port, port.pid)));
    ports.sort_by(|a, b| {
        (a.port, &a.proto, &a.address).cmp(&(b.port, &b.proto, &b.address))
    });
}

/// `ss -tulnp`: netid, state, recv-q, send-q, local, peer, [users:((...))].
fn parse_ss(body: &str) -> Vec<ListeningPort> {
    body.lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.len() < 6 {
                return None;
            }
            let proto = base_proto(fields[0])?;
            let (address, port) = split_host_port(fields[4])?;
            let rest = fields[6..].join(" ");
            let (process, pid) = parse_ss_users(&rest);
            Some(ListeningPort { proto, address, port, pid, process })
        })
        .collect()
}

/// `users:(("nginx",pid=2,fd=6),("nginx",pid=1,fd=6))` -> distinct names, first pid.
fn parse_ss_users(value: &str) -> (Option<String>, Option<u32>) {
    let mut names: Vec<String> = Vec::new();
    let mut pid = None;
    for part in value.split("(\"").skip(1) {
        let Some((name, rest)) = part.split_once('"') else { continue };
        if !names.iter().any(|existing| existing == name) {
            names.push(name.to_string());
        }
        if pid.is_none() {
            pid = rest
                .split_once("pid=")
                .and_then(|(_, tail)| tail.split(|c: char| !c.is_ascii_digit()).next())
                .and_then(|digits| digits.parse().ok());
        }
    }
    ((!names.is_empty()).then(|| names.join(", ")), pid)
}

/// `sockstat -46l`: user, command, pid, fd, proto, local, foreign.
fn parse_sockstat(body: &str) -> Vec<ListeningPort> {
    body.lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.len() < 6 || fields[0] == "USER" {
                return None;
            }
            let proto = base_proto(fields[4])?;
            let (address, port) = split_host_port(fields[5])?;
            Some(ListeningPort {
                proto,
                address,
                port,
                pid: fields[2].parse().ok(),
                process: (fields[1] != "?").then(|| fields[1].to_string()),
            })
        })
        .collect()
}

/// `lsof -nP`: command, pid, user, fd, type, device, size/off, node, name.
fn parse_lsof(body: &str) -> Vec<ListeningPort> {
    body.lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.len() < 9 || fields[0] == "COMMAND" {
                return None;
            }
            let node = fields.iter().position(|f| *f == "TCP" || *f == "UDP")?;
            let name = fields.get(node + 1)?;
            if name.contains("->") {
                return None;
            }
            let (address, port) = split_host_port(name)?;
            Some(ListeningPort {
                proto: fields[node].to_ascii_lowercase(),
                address,
                port,
                pid: fields[1].parse().ok(),
                process: Some(fields[0].replace("\\x20", " ")),
            })
        })
        .collect()
}

/// `netstat -tulnp`: proto, recv-q, send-q, local, foreign, [state], pid/program.
fn parse_netstat(body: &str) -> Vec<ListeningPort> {
    body.lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.len() < 5 {
                return None;
            }
            let proto = base_proto(fields[0])?;
            let (address, port) = split_host_port(fields[3])?;
            let mut rest = &fields[5..];
            if rest.first().is_some_and(|f| f.chars().all(|c| c.is_ascii_uppercase())) {
                rest = &rest[1..];
            }
            let program = rest.join(" ");
            let (pid, process) = match program.split_once('/') {
                Some((pid, name)) => (pid.parse().ok(), Some(name.to_string())),
                None => (None, None),
            };
            Some(ListeningPort { proto, address, port, pid, process })
        })
        .collect()
}

#[derive(Deserialize)]
struct WindowsPort {
    proto: String,
    address: String,
    port: u16,
    pid: Option<u32>,
    process: Option<String>,
}

/// PowerShell emits a bare object for one element, so accept both shapes.
fn json_list<T: for<'de> Deserialize<'de>>(value: serde_json::Value) -> Vec<T> {
    let items = match value {
        serde_json::Value::Array(items) => items,
        serde_json::Value::Null => Vec::new(),
        other => vec![other],
    };
    items
        .into_iter()
        .filter_map(|item| serde_json::from_value(item).ok())
        .collect()
}

fn parse_json(output: &CommandOutput, what: &str) -> SshResult<serde_json::Value> {
    let text = output.stdout.trim();
    if text.is_empty() {
        return Err(SshError::Io(format!("Could not read {what}: {}", output.failure_text())));
    }
    serde_json::from_str(text)
        .map_err(|error| SshError::Io(format!("Unexpected {what} output from PowerShell: {error}")))
}

fn parse_windows_ports(output: &CommandOutput, command: String) -> SshResult<PortsReport> {
    let value = parse_json(output, "listening ports")?;
    let mut ports: Vec<ListeningPort> = json_list::<WindowsPort>(value)
        .into_iter()
        .map(|port| ListeningPort {
            proto: port.proto,
            address: port.address,
            port: port.port,
            pid: port.pid,
            process: port.process.filter(|name| !name.is_empty()),
        })
        .collect();
    dedupe_and_sort(&mut ports);
    Ok(PortsReport {
        ports,
        tool: "Get-NetTCPConnection".to_string(),
        command,
        elevated: false,
        note: None,
    })
}

/* ── Firewall parsing ──────────────────────────────────────────────────── */

pub fn parse_firewall(
    os: OsFamily,
    output: &CommandOutput,
    command: String,
    elevated: bool,
) -> SshResult<FirewallReport> {
    if os == OsFamily::Windows {
        return parse_windows_firewall(output, command);
    }
    if let Some(error) = sudo_failure(output, elevated) {
        return Err(error);
    }

    let backends: Vec<FirewallBackend> = sections(&output.stdout)
        .into_iter()
        .filter_map(|(name, body)| unix_backend(name, body))
        .collect();

    let any_denied = backends.iter().any(|backend| denied(&backend.output));
    let note = if backends.is_empty() {
        Some("No supported firewall tool was found (ufw, firewalld, nftables, iptables, pf, ipfw).".to_string())
    } else if any_denied && !elevated {
        Some("Some firewalls need root to read their rules. Read again with sudo to include them.".to_string())
    } else {
        None
    };

    Ok(FirewallReport { backends, command, elevated, note })
}

fn unix_backend(name: &str, body: String) -> Option<FirewallBackend> {
    let output = body.trim_end().to_string();
    let lines: Vec<&str> = output.lines().map(str::trim).collect();
    let find = |prefix: &str| {
        lines
            .iter()
            .find_map(|line| line.strip_prefix(prefix).map(|rest| rest.trim().to_string()))
    };

    let (label, state, summary) = match name {
        "ufw" => {
            let state = match find("Status:").as_deref() {
                Some("active") => FirewallState::Active,
                Some("inactive") => FirewallState::Inactive,
                _ => FirewallState::Unknown,
            };
            ("UFW", state, find("Default:"))
        }
        "firewalld" => {
            let state = match lines.first().copied() {
                Some("running") => FirewallState::Active,
                Some("not running") => FirewallState::Inactive,
                _ => FirewallState::Unknown,
            };
            let zone = lines
                .iter()
                .find(|line| line.contains("(active)"))
                .map(|line| format!("zone {line}"));
            ("firewalld", state, zone)
        }
        "nftables" => {
            let tables = lines.iter().filter(|line| line.starts_with("table ")).count();
            let (state, summary) = if denied(&output) {
                (FirewallState::Unknown, None)
            } else if tables == 0 {
                (FirewallState::Inactive, Some("no ruleset loaded".to_string()))
            } else {
                (FirewallState::Active, Some(plural(tables, "table")))
            };
            ("nftables", state, summary)
        }
        "iptables" => {
            if denied(&output) {
                ("iptables", FirewallState::Unknown, None)
            } else {
                let policies: Vec<String> = lines
                    .iter()
                    .filter_map(|line| line.strip_prefix("-P "))
                    .map(str::to_string)
                    .collect();
                let rules = lines.iter().filter(|line| line.starts_with("-A ")).count();
                let restrictive = policies.iter().any(|policy| !policy.ends_with("ACCEPT"));
                let state = if rules > 0 || restrictive {
                    FirewallState::Active
                } else {
                    FirewallState::Inactive
                };
                let mut summary = policies.join(", ");
                if !summary.is_empty() {
                    summary.push_str("; ");
                }
                summary.push_str(&plural(rules, "rule"));
                ("iptables", state, Some(summary))
            }
        }
        "appfw" => {
            let global = lines.first().copied().unwrap_or_default().to_ascii_lowercase();
            let state = if global.contains("disabled") {
                FirewallState::Inactive
            } else if global.contains("enabled") {
                FirewallState::Active
            } else {
                FirewallState::Unknown
            };
            let stealth = lines
                .iter()
                .find(|line| line.to_ascii_lowercase().contains("stealth"))
                .map(|line| line.to_string());
            ("Application Firewall", state, stealth)
        }
        "pf" => {
            let state = match find("Status:") {
                Some(status) if status.starts_with("Enabled") => FirewallState::Active,
                Some(status) if status.starts_with("Disabled") => FirewallState::Inactive,
                _ => FirewallState::Unknown,
            };
            ("pf", state, None)
        }
        "ipfw" => {
            let state = if denied(&output) {
                FirewallState::Unknown
            } else {
                FirewallState::Active
            };
            let rules = lines.iter().filter(|line| !line.is_empty()).count();
            ("ipfw", state, (state == FirewallState::Active).then(|| plural(rules, "rule")))
        }
        _ => return None,
    };

    Some(FirewallBackend {
        name: label.to_string(),
        state,
        summary,
        output,
    })
}

fn plural(count: usize, noun: &str) -> String {
    if count == 1 {
        format!("1 {noun}")
    } else {
        format!("{count} {noun}s")
    }
}

#[derive(Deserialize)]
struct WindowsFirewall {
    #[serde(default)]
    profiles: serde_json::Value,
    #[serde(default)]
    rules: serde_json::Value,
}

#[derive(Deserialize)]
struct WindowsProfile {
    name: String,
    enabled: String,
    inbound: String,
    outbound: String,
}

#[derive(Deserialize)]
struct WindowsRule {
    name: String,
    action: String,
    profile: String,
}

fn parse_windows_firewall(output: &CommandOutput, command: String) -> SshResult<FirewallReport> {
    let value = parse_json(output, "the firewall")?;
    let parsed: WindowsFirewall = serde_json::from_value(value)
        .map_err(|error| SshError::Io(format!("Unexpected firewall output from PowerShell: {error}")))?;

    let mut rules: Vec<WindowsRule> = json_list(parsed.rules);
    rules.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    let backends = json_list::<WindowsProfile>(parsed.profiles)
        .into_iter()
        .map(|profile| {
            let applies: Vec<String> = rules
                .iter()
                .filter(|rule| rule.profile == "Any" || rule.profile.contains(&profile.name))
                .map(|rule| format!("{:<6} {}", rule.action, rule.name))
                .collect();
            FirewallBackend {
                name: format!("Windows Firewall ({})", profile.name),
                state: match profile.enabled.as_str() {
                    "True" => FirewallState::Active,
                    "False" => FirewallState::Inactive,
                    _ => FirewallState::Unknown,
                },
                summary: Some(format!(
                    "inbound {}, outbound {}; {} enabled inbound",
                    profile.inbound,
                    profile.outbound,
                    plural(applies.len(), "rule")
                )),
                output: applies.join("\n"),
            }
        })
        .collect();

    Ok(FirewallReport {
        backends,
        command,
        elevated: false,
        note: None,
    })
}

/* ── Users parsing ─────────────────────────────────────────────────────── */

pub fn parse_users(os: OsFamily, output: &CommandOutput) -> SshResult<UsersReport> {
    if os == OsFamily::Windows {
        return parse_quser(output);
    }
    if !output.succeeded() && output.stdout.trim().is_empty() {
        return Err(SshError::Io(format!("Could not list sessions: {}", output.failure_text())));
    }
    Ok(UsersReport {
        users: parse_who(&output.stdout),
        note: None,
    })
}

/// `who`: user, tty, login time tokens, optional `(host)`.
fn parse_who(stdout: &str) -> Vec<LoggedInUser> {
    stdout
        .lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.len() < 2 {
                return None;
            }
            let mut middle = &fields[2..];
            let mut from = None;
            if let Some(last) = middle.last() {
                if let Some(host) = last.strip_prefix('(').and_then(|s| s.strip_suffix(')')) {
                    from = (!host.is_empty()).then(|| host.to_string());
                    middle = &middle[..middle.len() - 1];
                }
            }
            Some(LoggedInUser {
                user: fields[0].to_string(),
                terminal: Some(fields[1].to_string()),
                from,
                login_time: (!middle.is_empty()).then(|| middle.join(" ")),
                state: None,
                idle: None,
            })
        })
        .collect()
}

fn parse_quser(output: &CommandOutput) -> SshResult<UsersReport> {
    let note = Some("Windows lists console and Remote Desktop sessions; OpenSSH logons may not appear.".to_string());
    let combined = format!("{}\n{}", output.stdout, output.stderr).to_ascii_lowercase();

    if combined.contains("no user exists") {
        return Ok(UsersReport { users: Vec::new(), note });
    }
    if combined.contains("not recognized") || combined.contains("not found") {
        return Ok(UsersReport {
            users: Vec::new(),
            note: Some("quser is not available on this edition of Windows, so sessions cannot be listed.".to_string()),
        });
    }
    if !output.succeeded() && output.stdout.trim().is_empty() {
        return Err(SshError::Io(format!("Could not list sessions: {}", output.failure_text())));
    }

    let users = output
        .stdout
        .lines()
        .skip(1)
        .filter_map(|line| {
            let fields: Vec<&str> = line.trim_end_matches('\r').split_whitespace().collect();
            let digits = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit());
            // A disconnected session has no session name, which shifts the ID left.
            let id = (1..=2).find(|&index| fields.get(index).is_some_and(|f| digits(f)))?;
            let clean = |value: &str| (value != "." && !value.eq_ignore_ascii_case("none")).then(|| value.to_string());
            Some(LoggedInUser {
                user: fields[0].trim_start_matches('>').to_string(),
                terminal: (id == 2).then(|| fields[1].to_string()),
                from: None,
                login_time: fields.get(id + 3..).filter(|rest| !rest.is_empty()).map(|rest| rest.join(" ")),
                state: fields.get(id + 1).map(|s| s.to_string()),
                idle: fields.get(id + 2).and_then(|s| clean(s)),
            })
        })
        .collect();

    Ok(UsersReport { users, note })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn out(stdout: &str) -> CommandOutput {
        CommandOutput {
            stdout: stdout.to_string(),
            stderr: String::new(),
            exit_code: Some(0),
        }
    }

    #[test]
    fn base64_matches_known_vectors() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn powershell_script_is_utf16le_encoded() {
        // "a" in UTF-16LE is 61 00.
        assert!(powershell_encoded("a").ends_with("-EncodedCommand YQA="));
    }

    #[test]
    fn unix_scripts_quote_cleanly_and_sudo_only_when_asked() {
        assert!(!UNIX_PORTS_SCRIPT.contains('\''));
        assert!(!UNIX_FIREWALL_SCRIPT.contains('\''));

        let plain = ports_command(OsFamily::Linux, &Elevation::SudoPassword, false).unwrap();
        assert!(plain.starts_with("sh -c 'exec </dev/null; "));
        let sudo = ports_command(OsFamily::Linux, &Elevation::SudoPassword, true).unwrap();
        assert!(sudo.starts_with("sudo -S -p '' sh -c 'exec </dev/null; "));
        let root = ports_command(OsFamily::Linux, &Elevation::NotNeeded, true).unwrap();
        assert!(root.starts_with("sh -c 'exec </dev/null; "));
        let firewall = firewall_command(OsFamily::Linux, &Elevation::SudoPassword, true).unwrap();
        assert!(firewall.starts_with("sudo -S -p '' sh -c 'exec </dev/null; "));
        assert!(ports_command(OsFamily::Unknown, &Elevation::NotNeeded, false).is_err());
    }

    #[test]
    fn commands_never_carry_the_password() {
        // Builders take no password at all; this pins that the preview shown in
        // the prompt is exactly what runs and holds nothing secret.
        for os in [OsFamily::Linux, OsFamily::Macos, OsFamily::Bsd] {
            let ports = ports_command(os, &Elevation::SudoPassword, true).unwrap();
            assert_eq!(preview(os, &Elevation::SudoPassword, SecurityView::Ports).unwrap(), ports);
            assert!(ports.contains("sudo -S -p ''"));
        }
    }

    #[test]
    fn parses_ss_with_and_without_process() {
        let stdout = "---PAROLA:ss---\n\
Netid State  Recv-Q Send-Q Local Address:Port  Peer Address:Port Process\n\
udp   UNCONN 0      0      127.0.0.53%lo:53         0.0.0.0:*    users:((\"systemd-resolve\",pid=600,fd=13))\n\
tcp   LISTEN 0      511             [::]:80            [::]:*    users:((\"nginx\",pid=2,fd=6),(\"nginx\",pid=1,fd=6))\n\
tcp   LISTEN 0      4096         0.0.0.0:22         0.0.0.0:*\n";
        let report = parse_ports(OsFamily::Linux, &out(stdout), String::new(), false).unwrap();
        assert_eq!(report.tool, "ss");
        assert_eq!(
            report.ports,
            vec![
                ListeningPort { proto: "tcp".into(), address: "0.0.0.0".into(), port: 22, pid: None, process: None },
                ListeningPort { proto: "udp".into(), address: "127.0.0.53%lo".into(), port: 53, pid: Some(600), process: Some("systemd-resolve".into()) },
                ListeningPort { proto: "tcp".into(), address: "::".into(), port: 80, pid: Some(2), process: Some("nginx".into()) },
            ]
        );
        assert!(report.note.is_some());
    }

    #[test]
    fn parses_netstat_tcp_and_udp_rows() {
        let body = "Active Internet connections (only servers)\n\
Proto Recv-Q Send-Q Local Address  Foreign Address State  PID/Program name\n\
tcp   0      0      0.0.0.0:22     0.0.0.0:*       LISTEN 812/sshd: /usr/sbin\n\
udp   0      0      0.0.0.0:68     0.0.0.0:*              -\n";
        let ports = parse_netstat(body);
        assert_eq!(ports.len(), 2);
        assert_eq!(ports[0].pid, Some(812));
        assert_eq!(ports[0].process.as_deref(), Some("sshd: /usr/sbin"));
        assert_eq!(ports[1].proto, "udp");
        assert_eq!(ports[1].process, None);
    }

    #[test]
    fn parses_lsof_and_skips_connected_udp() {
        let body = "COMMAND   PID USER   FD   TYPE DEVICE SIZE/OFF NODE NAME\n\
launchd     1 root   10u  IPv6 0x1      0t0  TCP *:22 (LISTEN)\n\
launchd     1 root   11u  IPv6 0x1      0t0  TCP *:22 (LISTEN)\n\
mDNS\\x20R  200 _mdns  6u  IPv4 0x2      0t0  UDP *:5353\n\
ntpd      300 root   5u  IPv4 0x3      0t0  UDP 10.0.0.2:123->10.0.0.1:123\n";
        let mut ports = parse_lsof(body);
        dedupe_and_sort(&mut ports);
        assert_eq!(ports.len(), 2);
        assert_eq!(ports[1].process.as_deref(), Some("mDNS R"));
    }

    #[test]
    fn parses_sockstat() {
        let body = "USER COMMAND PID FD PROTO LOCAL ADDRESS FOREIGN ADDRESS\n\
root sshd    812 3  tcp6  *:22          *:*\n\
?    ?       ?   ?  udp4  *:514         *:*\n";
        let ports = parse_sockstat(body);
        assert_eq!(ports[0].pid, Some(812));
        assert_eq!(ports[1].proto, "udp");
        assert_eq!(ports[1].process, None);
    }

    #[test]
    fn missing_tool_and_sudo_refusal_are_errors() {
        assert!(parse_ports(OsFamily::Linux, &out("---PAROLA:none---\n"), String::new(), false).is_err());
        let refused = CommandOutput {
            stdout: String::new(),
            stderr: "Sorry, try again.".into(),
            exit_code: Some(1),
        };
        let error = parse_firewall(OsFamily::Linux, &refused, String::new(), true).unwrap_err();
        assert!(error.to_string().contains("Sorry"));
    }

    #[test]
    fn parses_windows_ports_single_object_and_array() {
        let single = r#"{"proto":"tcp","address":"0.0.0.0","port":3389,"pid":1100,"process":"svchost"}"#;
        let report = parse_ports(OsFamily::Windows, &out(single), String::new(), false).unwrap();
        assert_eq!(report.ports.len(), 1);

        let array = r#"[{"proto":"udp","address":"::","port":500,"pid":4,"process":null},{"proto":"tcp","address":"0.0.0.0","port":22,"pid":9,"process":"sshd"}]"#;
        let report = parse_ports(OsFamily::Windows, &out(array), String::new(), false).unwrap();
        assert_eq!(report.ports[0].port, 22);
        assert_eq!(report.ports[1].process, None);
    }

    #[test]
    fn linux_firewall_states_unprivileged() {
        let stdout = "---PAROLA:ufw---\nERROR: You need to be root to run this script\n\
---PAROLA:nftables---\nOperation not permitted (you must be root)\n\
---PAROLA:iptables---\niptables v1.8.7 (nf_tables): Permission denied (you must be root)\n";
        let report = parse_firewall(OsFamily::Linux, &out(stdout), String::new(), false).unwrap();
        assert_eq!(report.backends.len(), 3);
        assert!(report.backends.iter().all(|b| b.state == FirewallState::Unknown));
        assert!(report.note.unwrap().contains("sudo"));
    }

    #[test]
    fn linux_firewall_states_elevated() {
        let stdout = "---PAROLA:ufw---\nStatus: active\nLogging: on (low)\n\
Default: deny (incoming), allow (outgoing), disabled (routed)\n\
---PAROLA:firewalld---\nnot running\n\
---PAROLA:nftables---\ntable inet filter {\n}\ntable ip nat {\n}\n\
---PAROLA:iptables---\n-P INPUT ACCEPT\n-P FORWARD ACCEPT\n-P OUTPUT ACCEPT\n";
        let report = parse_firewall(OsFamily::Linux, &out(stdout), String::new(), true).unwrap();
        let [ufw, firewalld, nft, ipt] = &report.backends[..] else { panic!() };
        assert_eq!(ufw.state, FirewallState::Active);
        assert_eq!(ufw.summary.as_deref(), Some("deny (incoming), allow (outgoing), disabled (routed)"));
        assert_eq!(firewalld.state, FirewallState::Inactive);
        assert_eq!(nft.summary.as_deref(), Some("2 tables"));
        assert_eq!(ipt.state, FirewallState::Inactive);
        assert!(report.note.is_none());
    }

    #[test]
    fn no_firewall_tool_is_a_note() {
        let report = parse_firewall(OsFamily::Bsd, &out(""), String::new(), false).unwrap();
        assert!(report.backends.is_empty());
        assert!(report.note.is_some());
    }

    #[test]
    fn parses_windows_firewall_profiles() {
        let stdout = r#"{"profiles":[{"name":"Domain","enabled":"True","inbound":"NotConfigured","outbound":"NotConfigured"},{"name":"Public","enabled":"False","inbound":"Block","outbound":"Allow"}],"rules":{"name":"Remote Desktop","action":"Allow","profile":"Domain, Private"}}"#;
        let report = parse_firewall(OsFamily::Windows, &out(stdout), String::new(), false).unwrap();
        assert_eq!(report.backends[0].state, FirewallState::Active);
        assert!(report.backends[0].output.contains("Remote Desktop"));
        assert_eq!(report.backends[1].state, FirewallState::Inactive);
        assert!(report.backends[1].output.is_empty());
    }

    #[test]
    fn parses_who_linux_and_macos() {
        let stdout = "alice    pts/0        2026-09-17 10:00 (192.0.2.10)\n\
bob      tty1         2026-09-17 09:00\n\
carol    console  Sep 17 08:00 \n";
        let users = parse_who(stdout);
        assert_eq!(users[0].from.as_deref(), Some("192.0.2.10"));
        assert_eq!(users[0].login_time.as_deref(), Some("2026-09-17 10:00"));
        assert_eq!(users[1].from, None);
        assert_eq!(users[2].login_time.as_deref(), Some("Sep 17 08:00"));
    }

    #[test]
    fn parses_quser_active_and_disconnected() {
        let stdout = " USERNAME              SESSIONNAME        ID  STATE   IDLE TIME  LOGON TIME\r\n\
>administrator         rdp-tcp#0           2  Active          .  9/17/2026 10:00 AM\r\n \
bob                                       3  Disc         1:05  9/17/2026 9:00 AM\r\n";
        let report = parse_quser(&out(stdout)).unwrap();
        let [admin, bob] = &report.users[..] else { panic!() };
        assert_eq!(admin.user, "administrator");
        assert_eq!(admin.terminal.as_deref(), Some("rdp-tcp#0"));
        assert_eq!(admin.idle, None);
        assert_eq!(admin.login_time.as_deref(), Some("9/17/2026 10:00 AM"));
        assert_eq!(bob.terminal, None);
        assert_eq!(bob.state.as_deref(), Some("Disc"));
        assert_eq!(bob.idle.as_deref(), Some("1:05"));
    }

    #[test]
    fn quser_with_no_sessions_is_empty() {
        let none = CommandOutput {
            stdout: String::new(),
            stderr: "No User exists for *\r\n".into(),
            exit_code: Some(1),
        };
        assert!(parse_quser(&none).unwrap().users.is_empty());
    }
}
