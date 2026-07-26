//! Asking — or at least noticing — the Twingate client.
//!
//! Twingate publishes no local status API. Linux's headless client ships
//! `twingate status`; macOS and Windows get presence detection only, and the
//! `detail` strings say so rather than claiming more than we know.
//!
//! Linux also gets `twingate resources` — the client stating which addresses it
//! owns and whether each is still authenticated. That matters because resources
//! are often plain ranges like `192.168.1.0/24` that no heuristic could
//! attribute to a VPN.

use std::net::Ipv4Addr;

use super::{VpnKind, VpnStatus};

#[cfg(target_os = "linux")]
pub async fn status() -> VpnStatus {
    use super::{run_cli, CliOutcome};

    for candidate in ["twingate", "/usr/bin/twingate"] {
        match run_cli(candidate, &["status"]).await {
            CliOutcome::Missing => continue,
            CliOutcome::TimedOut => {
                return VpnStatus {
                    kind: VpnKind::Twingate,
                    installed: true,
                    up: false,
                    detail: "the Twingate client is not responding".to_string(),
                }
            }
            CliOutcome::Ran { stdout, .. } => return interpret(&stdout),
        }
    }

    VpnStatus::not_installed(VpnKind::Twingate)
}

/// Map the one-word answer of `twingate status` — `online`, `offline`,
/// `authenticating`, `not-running` — onto our shape. Words we have not seen
/// are shown as-is; an empty answer gets called out rather than invented.
#[cfg(any(target_os = "linux", test))]
fn interpret(stdout: &str) -> VpnStatus {
    let state = stdout.trim();
    let (up, detail) = match state {
        "online" => (true, "connected".to_string()),
        "not-running" => (false, "not running".to_string()),
        "" => (false, "state unknown".to_string()),
        other => (false, other.to_string()),
    };

    VpnStatus {
        kind: VpnKind::Twingate,
        installed: true,
        up,
        detail,
    }
}

#[cfg(target_os = "macos")]
pub async fn status() -> VpnStatus {
    use super::{run_cli, CliOutcome};

    if !std::path::Path::new("/Applications/Twingate.app").exists() {
        return VpnStatus::not_installed(VpnKind::Twingate);
    }

    // `pgrep -x` exits zero only on a match; a missing pgrep collapses to
    // "not running".
    let running = matches!(
        run_cli("/usr/bin/pgrep", &["-x", "Twingate"]).await,
        CliOutcome::Ran { success: true, .. }
    );

    presence_only(running)
}

#[cfg(target_os = "windows")]
pub async fn status() -> VpnStatus {
    use super::{run_cli, CliOutcome};

    let installed = [
        r"C:\Program Files\Twingate\Twingate.exe",
        r"C:\Program Files (x86)\Twingate\Twingate.exe",
    ]
    .iter()
    .any(|path| std::path::Path::new(path).exists());

    if !installed {
        return VpnStatus::not_installed(VpnKind::Twingate);
    }

    // `tasklist` always exits zero, so read the output, not the status code.
    let running = match run_cli(
        "tasklist",
        &["/FI", "IMAGENAME eq Twingate.exe", "/NH"],
    )
    .await
    {
        CliOutcome::Ran { stdout, .. } => stdout.contains("Twingate.exe"),
        _ => false,
    };

    presence_only(running)
}

/// The best macOS and Windows can say: the app is or is not running.
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn presence_only(running: bool) -> VpnStatus {
    VpnStatus {
        kind: VpnKind::Twingate,
        installed: true,
        up: running,
        detail: if running {
            // Running counts as up — the best signal available — but the
            // wording keeps the uncertainty visible.
            "app running (connection state not visible)".to_string()
        } else {
            "app not running".to_string()
        },
    }
}

/// One entry of `twingate resources`: an address block the client owns.
#[derive(Debug, Clone)]
pub struct TwingateResource {
    pub name: String,
    /// A single IP, a CIDR range, or a `*.domain` wildcard.
    pub address: String,
    /// An optional friendlier DNS name the admin attached.
    pub alias: Option<String>,
    /// The client's own words, e.g. "Auth expires in 4 days".
    pub auth_status: String,
}

impl TwingateResource {
    /// Whether a saved hostname falls under this resource.
    pub fn matches(&self, hostname: &str) -> bool {
        let host = hostname.trim().trim_end_matches('.').to_ascii_lowercase();
        let address = self.address.to_ascii_lowercase();

        if host == address {
            return true;
        }

        // "*.example.com" covers subdomains, not the bare domain, as with TLS.
        if let Some(domain) = address.strip_prefix('*') {
            if host.len() > domain.len() && host.ends_with(domain) {
                return true;
            }
        }

        if let Some(alias) = &self.alias {
            if host == alias.to_ascii_lowercase() {
                return true;
            }
        }

        if let (Ok(ip), Some((base, prefix))) = (host.parse::<Ipv4Addr>(), parse_cidr(&address)) {
            return in_cidr(ip, base, prefix);
        }

        false
    }

    /// Whether the auth status says access would be refused right now. The
    /// client phrases health as "Auth expires in …", so only words that clearly
    /// mean "not any more" flip this; an unrecognised phrase is treated as fine.
    pub fn needs_auth(&self) -> bool {
        let status = self.auth_status.to_ascii_lowercase();
        status.contains("required") || status.contains("expired")
    }
}

/// The addresses Twingate owns on this machine. Only the Linux client can
/// answer, and only while its service runs; everywhere else this is empty and
/// the caller falls back to heuristics.
#[cfg(target_os = "linux")]
pub async fn resources() -> Vec<TwingateResource> {
    use super::{run_cli, CliOutcome};

    for candidate in ["twingate", "/usr/bin/twingate"] {
        // `-d` strips the ANSI colouring the table carries on a terminal.
        if let CliOutcome::Ran { stdout, .. } = run_cli(candidate, &["resources", "-d"]).await {
            return parse_resources(&stdout);
        }
    }

    Vec::new()
}

#[cfg(not(target_os = "linux"))]
pub async fn resources() -> Vec<TwingateResource> {
    Vec::new()
}

/// Read the table `twingate resources` prints: tab-separated name, address,
/// alias (`-` for none), auth status. Rows that do not fit — header, errors, a
/// future format change — are skipped rather than half-parsed.
fn parse_resources(stdout: &str) -> Vec<TwingateResource> {
    stdout
        .lines()
        .skip(1) // the header row
        .filter_map(|line| {
            let mut fields = line.split('\t').map(str::trim);
            let name = fields.next()?;
            let address = fields.next()?;
            let alias = fields.next()?;
            let auth_status = fields.next()?;
            if name.is_empty() || address.is_empty() {
                return None;
            }
            Some(TwingateResource {
                name: name.to_string(),
                address: address.to_string(),
                alias: (alias != "-" && !alias.is_empty()).then(|| alias.to_string()),
                auth_status: auth_status.to_string(),
            })
        })
        .collect()
}

fn parse_cidr(address: &str) -> Option<(Ipv4Addr, u8)> {
    let (base, prefix) = address.split_once('/')?;
    let prefix: u8 = prefix.parse().ok()?;
    if prefix > 32 {
        return None;
    }
    Some((base.parse().ok()?, prefix))
}

fn in_cidr(ip: Ipv4Addr, base: Ipv4Addr, prefix: u8) -> bool {
    if prefix == 0 {
        return true;
    }
    let mask = u32::MAX << (32 - u32::from(prefix));
    (u32::from(ip) & mask) == (u32::from(base) & mask)
}