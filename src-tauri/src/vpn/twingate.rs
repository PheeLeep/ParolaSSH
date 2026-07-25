//! Asking — or at least noticing — the Twingate client.
//!
//! Twingate publishes no local status API. Linux gets a real answer because
//! the headless client ships a `twingate status` command; macOS and Windows
//! get presence detection only — the app is either running or it is not, and
//! whether it is *connected* stays its secret. The `detail` strings are
//! honest about that difference so the UI never claims more than we know.
//!
//! Linux also gets `twingate resources`, and that one matters more than it
//! looks: Twingate resources are often plain private ranges like
//! `192.168.1.0/24` that no address heuristic could ever attribute to a VPN —
//! the same IP works directly when on premise and through Twingate when not.
//! The resource list is the client saying outright which addresses it owns,
//! and whether each still has valid authentication.

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

    // `pgrep -x` exits zero only on a match. Missing pgrep would be a very
    // strange macOS install; it collapses to "not running", the safe answer
    // for advice that only triggers on it being down.
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

    // `tasklist` always exits zero, so the answer is in whether the filtered
    // output names the process rather than in the status code.
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
            // Running is treated as up because it is the best signal
            // available, but the wording keeps the uncertainty visible.
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

        // "*.example.com" covers subdomains, not the bare domain — the same
        // reading a TLS certificate would give it.
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

    /// Whether the auth status says access would be refused right now.
    ///
    /// The client phrases health as "Auth expires in …", so only the words
    /// that clearly mean "not any more" flip this — an unrecognised phrase
    /// is treated as fine rather than sending someone to re-authenticate
    /// for nothing.
    pub fn needs_auth(&self) -> bool {
        let status = self.auth_status.to_ascii_lowercase();
        status.contains("required") || status.contains("expired")
    }
}

/// The addresses Twingate owns on this machine.
///
/// Only the Linux client can answer, and only while its service is running —
/// everywhere else this is empty and the caller falls back to heuristics.
/// A stopped service costs us the list too; nothing to do about that beyond
/// what the plain up/down advice already covers.
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

/// Read the tab-separated table `twingate resources` prints.
///
/// Columns are space-padded for the eye and tab-separated for us:
/// name, address, alias (`-` for none), auth status. Anything that does not
/// fit that shape — the header, an error line, a future format change — is
/// skipped rather than half-parsed.
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