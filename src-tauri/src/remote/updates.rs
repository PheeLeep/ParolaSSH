//! What updates a host is waiting on. Read-only, forever.
//!
//! There is deliberately no install verb in this module and there never will
//! be from this pane: silently changing packages on someone's server from a
//! GUI is the same category of mistake as installing Lynis would be. The pane
//! reports; the operator decides in a terminal.
//!
//! Detection and listing happen in **one round trip** per platform, with
//! `PAROLA_` markers separating what the parser needs to know from what the
//! tools printed. A host with no recognised package manager is an honest
//! empty state, not an error — the same philosophy as the VPN module's
//! `CliOutcome::Missing`.
//!
//! Windows honesty in particular: querying *pending* updates from the CLI
//! needs the PSWindowsUpdate module, which most machines do not have. Absent
//! module, the pane says so and shows recent installed hotfixes instead —
//! and never installs the module to improve its own answer.

use std::time::Duration;

use serde::Serialize;

use super::{CommandOutput, OsFamily};
use crate::ssh::{SshError, SshResult};

/// One pending update.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateItem {
    pub name: String,
    /// The installed version, when the manager reports it.
    pub current: Option<String>,
    pub available: String,
    /// Repository / suite the update comes from.
    pub source: String,
    pub security: bool,
}

/// One entry of Windows' installed-hotfix history.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HotfixItem {
    pub id: String,
    pub description: String,
    pub installed_on: Option<String>,
}

/// What the check found, shaped so each honest outcome renders differently.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum UpdateReport {
    /// Pending updates, listed.
    List {
        manager: String,
        updates: Vec<UpdateItem>,
        /// How many are security updates — `None` when the manager cannot say.
        security_count: Option<usize>,
    },
    /// The manager answered and the answer is "nothing pending".
    UpToDate { manager: String },
    /// No recognised package manager on the host.
    ManagerMissing { detail: String },
    /// Windows without PSWindowsUpdate: pending updates cannot be queried,
    /// but the installed history can still be shown.
    ModuleMissing {
        detail: String,
        installed_history: Vec<HotfixItem>,
    },
}

/// Everything Linux needs in one exec. `PAROLA_MGR` names the manager the
/// host actually has; `PAROLA_EXIT` captures dnf's tri-state exit code inline
/// because the compound command's own status would swallow it.
const LINUX_COMMAND: &str = "if command -v apt-get >/dev/null 2>&1; then \
     echo PAROLA_MGR=apt; apt list --upgradable 2>/dev/null; \
     elif command -v dnf >/dev/null 2>&1; then \
     echo PAROLA_MGR=dnf; dnf -q check-update; echo PAROLA_EXIT=$?; \
     else echo PAROLA_MGR=none; fi";

/// Detection plus history in one PowerShell run. Listing *pending* updates
/// happens in a second exec only when the module turned out to be present.
const WINDOWS_COMMAND: &str = "powershell -NoProfile -NonInteractive -Command \
    \"if (Get-Module -ListAvailable PSWindowsUpdate) { 'PAROLA_MODULE=yes' } \
    else { 'PAROLA_MODULE=no' }; \
    Get-HotFix | Sort-Object InstalledOn -Descending | Select-Object -First 20 \
    HotFixID,Description,@{n='InstalledOn';e={$_.InstalledOn.ToString('yyyy-MM-dd')}} | \
    ConvertTo-Json\"";

/// The pending-update query, run only when PSWindowsUpdate exists. It goes
/// out to Microsoft's servers and can genuinely take minutes.
const WINDOWS_PENDING_COMMAND: &str = "powershell -NoProfile -NonInteractive -Command \
    \"Import-Module PSWindowsUpdate; Get-WindowsUpdate | \
    Select-Object Title,KB,Size | ConvertTo-Json\"";

pub const WINDOWS_PENDING_TIMEOUT: Duration = Duration::from_secs(120);

/// The first-round command for this OS. Pure.
pub fn check_command(os: OsFamily) -> SshResult<&'static str> {
    match os {
        OsFamily::Linux => Ok(LINUX_COMMAND),
        OsFamily::Windows => Ok(WINDOWS_COMMAND),
        OsFamily::Macos | OsFamily::Bsd => Err(SshError::unsupported(format!(
            "Update checking is built for apt, dnf and Windows Update; {} \
             (softwareupdate/pkg) is not implemented yet.",
            os.label()
        ))),
        OsFamily::Unknown => Err(SshError::unsupported(
            "The remote operating system is unknown, so no update command can be \
             chosen safely.",
        )),
    }
}

/// Second-round command for Windows hosts that do have the module.
pub fn windows_pending_command() -> &'static str {
    WINDOWS_PENDING_COMMAND
}

/// Parse the Linux round. Pure, fixture-tested.
pub fn parse_linux(output: &CommandOutput) -> UpdateReport {
    let stdout = &output.stdout;

    let manager = stdout
        .lines()
        .find_map(|line| line.trim().strip_prefix("PAROLA_MGR="))
        .unwrap_or("none");

    match manager {
        "apt" => parse_apt(stdout),
        "dnf" => parse_dnf(stdout),
        _ => UpdateReport::ManagerMissing {
            detail: "Neither apt nor dnf is on this host's PATH. If it uses another \
                     package manager, check for updates from a terminal."
                .to_string(),
        },
    }
}

/// `apt list --upgradable`: `name/suite version arch [upgradable from: old]`.
fn parse_apt(stdout: &str) -> UpdateReport {
    let updates: Vec<UpdateItem> = stdout
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            // Skip our marker, apt's "Listing... Done" header, and blanks.
            let (name_and_suite, rest) = line.split_once(' ')?;
            let (name, suite) = name_and_suite.split_once('/')?;

            let mut fields = rest.split_whitespace();
            let available = fields.next()?.to_string();
            let _arch = fields.next()?;

            // `[upgradable from: 1.2.3-1]` when apt knows the installed version.
            let current = line
                .rsplit_once("upgradable from:")
                .map(|(_, tail)| tail.trim().trim_end_matches(']').to_string())
                .filter(|value| !value.is_empty());

            Some(UpdateItem {
                name: name.to_string(),
                current,
                available,
                source: suite.to_string(),
                security: suite.contains("-security"),
            })
        })
        .collect();

    if updates.is_empty() {
        return UpdateReport::UpToDate {
            manager: "apt".to_string(),
        };
    }

    let security_count = updates.iter().filter(|update| update.security).count();
    UpdateReport::List {
        manager: "apt".to_string(),
        updates,
        security_count: Some(security_count),
    }
}

/// `dnf -q check-update`: rows of `name.arch  version  repo`, exit 100 when
/// updates exist, 0 when none, anything else a real error.
fn parse_dnf(stdout: &str) -> UpdateReport {
    let exit: Option<u32> = stdout
        .lines()
        .find_map(|line| line.trim().strip_prefix("PAROLA_EXIT="))
        .and_then(|value| value.parse().ok());

    match exit {
        Some(0) => {
            return UpdateReport::UpToDate {
                manager: "dnf".to_string(),
            }
        }
        Some(100) => {} // updates exist; fall through to the rows
        _ => {
            return UpdateReport::ManagerMissing {
                detail: "dnf did not answer cleanly; check for updates from a terminal."
                    .to_string(),
            }
        }
    }

    let mut in_obsoleting = false;
    let updates: Vec<UpdateItem> = stdout
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with("PAROLA_") || trimmed.is_empty() {
                return None;
            }
            // The "Obsoleting Packages" section lists replacements, not
            // updates, and its rows would parse identically — skip it.
            if trimmed.starts_with("Obsoleting Packages") {
                in_obsoleting = true;
                return None;
            }
            if in_obsoleting {
                return None;
            }

            let fields: Vec<&str> = trimmed.split_whitespace().collect();
            if fields.len() != 3 {
                return None;
            }
            let (name, _arch) = fields[0].rsplit_once('.')?;

            Some(UpdateItem {
                name: name.to_string(),
                current: None,
                available: fields[1].to_string(),
                source: fields[2].to_string(),
                // dnf's plain check-update does not flag security per row.
                security: false,
            })
        })
        .collect();

    if updates.is_empty() {
        // Exit 100 promised rows; not finding any means the output surprised
        // us, and saying "up to date" would be a lie.
        return UpdateReport::ManagerMissing {
            detail: "dnf reported pending updates but the list could not be read; \
                     check from a terminal."
                .to_string(),
        };
    }

    UpdateReport::List {
        manager: "dnf".to_string(),
        updates,
        security_count: None,
    }
}

/// Parse the Windows first round: module presence + hotfix history. Pure.
///
/// Returns the history and whether the module exists; the caller runs the
/// pending query only in the second case.
pub fn parse_windows_first_round(output: &CommandOutput) -> (bool, Vec<HotfixItem>) {
    let stdout = &output.stdout;
    let module_present = stdout.contains("PAROLA_MODULE=yes");

    // The JSON document starts after our marker line.
    let json_start = stdout.find(['{', '[']);
    let history = json_start
        .and_then(|start| serde_json::from_str::<serde_json::Value>(stdout[start..].trim()).ok())
        .map(|value| {
            let items: Vec<&serde_json::Value> = match &value {
                serde_json::Value::Array(items) => items.iter().collect(),
                item @ serde_json::Value::Object(_) => vec![item],
                _ => Vec::new(),
            };
            items
                .into_iter()
                .filter_map(|item| {
                    Some(HotfixItem {
                        id: item.get("HotFixID")?.as_str()?.to_string(),
                        description: item
                            .get("Description")
                            .and_then(|d| d.as_str())
                            .unwrap_or("")
                            .to_string(),
                        installed_on: item
                            .get("InstalledOn")
                            .and_then(|d| d.as_str())
                            .map(str::to_string),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    (module_present, history)
}

/// Parse `Get-WindowsUpdate` JSON into the report. Pure.
pub fn parse_windows_pending(output: &CommandOutput) -> UpdateReport {
    let stdout = output.stdout.trim();

    let value: Option<serde_json::Value> = stdout
        .find(['{', '['])
        .and_then(|start| serde_json::from_str(stdout[start..].trim()).ok());

    let items: Vec<serde_json::Value> = match value {
        Some(serde_json::Value::Array(items)) => items,
        Some(item @ serde_json::Value::Object(_)) => vec![item],
        // Empty output means the query ran and found nothing pending.
        _ if stdout.is_empty() => {
            return UpdateReport::UpToDate {
                manager: "Windows Update".to_string(),
            }
        }
        _ => {
            return UpdateReport::ManagerMissing {
                detail: "Get-WindowsUpdate answered in a shape this app could not read."
                    .to_string(),
            }
        }
    };

    let updates: Vec<UpdateItem> = items
        .iter()
        .filter_map(|item| {
            Some(UpdateItem {
                name: item.get("Title")?.as_str()?.to_string(),
                current: None,
                available: item
                    .get("KB")
                    .and_then(|kb| kb.as_str())
                    .unwrap_or("")
                    .to_string(),
                source: "Windows Update".to_string(),
                security: item
                    .get("Title")
                    .and_then(|t| t.as_str())
                    .map(|t| t.contains("Security"))
                    .unwrap_or(false),
            })
        })
        .collect();

    if updates.is_empty() {
        return UpdateReport::UpToDate {
            manager: "Windows Update".to_string(),
        };
    }

    let security_count = updates.iter().filter(|update| update.security).count();
    UpdateReport::List {
        manager: "Windows Update".to_string(),
        updates,
        security_count: Some(security_count),
    }
}

/// The sentence shown when PSWindowsUpdate is absent — the normal case.
pub fn module_missing_detail() -> String {
    "Pending updates on Windows can only be queried through the PSWindowsUpdate \
     PowerShell module, which this machine does not have. This app will not \
     install it. Recent installed updates are shown below; for pending ones, \
     check Settings → Windows Update on the machine."
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output(stdout: &str) -> CommandOutput {
        CommandOutput {
            stdout: stdout.to_string(),
            stderr: String::new(),
            exit_code: Some(0),
        }
    }

    #[test]
    fn apt_rows_are_parsed_and_security_flagged() {
        let report = parse_linux(&output(
            "PAROLA_MGR=apt\n\
             Listing... Done\n\
             openssl/noble-security 3.0.13-0ubuntu3.6 amd64 [upgradable from: 3.0.13-0ubuntu3.5]\n\
             vim/noble-updates 2:9.1.0016-1ubuntu7.8 amd64 [upgradable from: 2:9.1.0016-1ubuntu7.5]\n",
        ));

        let UpdateReport::List { manager, updates, security_count } = report else {
            panic!("expected a list");
        };
        assert_eq!(manager, "apt");
        assert_eq!(updates.len(), 2);
        assert_eq!(updates[0].name, "openssl");
        assert_eq!(updates[0].available, "3.0.13-0ubuntu3.6");
        assert_eq!(updates[0].current.as_deref(), Some("3.0.13-0ubuntu3.5"));
        assert!(updates[0].security);
        assert!(!updates[1].security);
        assert_eq!(security_count, Some(1));
    }

    #[test]
    fn apt_with_no_rows_is_up_to_date() {
        let report = parse_linux(&output("PAROLA_MGR=apt\nListing... Done\n"));
        assert!(matches!(report, UpdateReport::UpToDate { manager } if manager == "apt"));
    }

    #[test]
    fn dnf_exit_100_means_updates_and_obsoleting_rows_are_skipped() {
        let report = parse_linux(&output(
            "PAROLA_MGR=dnf\n\
             kernel-core.x86_64  5.14.0-503.14.1.el9_5  baseos\n\
             openssl-libs.x86_64  1:3.2.2-6.el9_5  appstream\n\
             Obsoleting Packages\n\
             grub2-tools.x86_64  1:2.06-77.el9  baseos\n\
             PAROLA_EXIT=100\n",
        ));

        let UpdateReport::List { manager, updates, security_count } = report else {
            panic!("expected a list");
        };
        assert_eq!(manager, "dnf");
        assert_eq!(updates.len(), 2);
        assert_eq!(updates[0].name, "kernel-core");
        assert_eq!(updates[0].source, "baseos");
        // dnf's plain check cannot flag security rows; the report says so.
        assert_eq!(security_count, None);
    }

    #[test]
    fn dnf_exit_0_means_up_to_date_and_exit_1_means_trouble() {
        let clean = parse_linux(&output("PAROLA_MGR=dnf\nPAROLA_EXIT=0\n"));
        assert!(matches!(clean, UpdateReport::UpToDate { manager } if manager == "dnf"));

        let broken = parse_linux(&output("PAROLA_MGR=dnf\nError: broken repo\nPAROLA_EXIT=1\n"));
        assert!(matches!(broken, UpdateReport::ManagerMissing { .. }));
    }

    #[test]
    fn a_host_with_no_manager_is_an_honest_empty_state() {
        let report = parse_linux(&output("PAROLA_MGR=none\n"));
        let UpdateReport::ManagerMissing { detail } = report else {
            panic!("expected managerMissing");
        };
        assert!(detail.contains("apt"));
    }

    #[test]
    fn windows_first_round_reads_module_presence_and_history() {
        let stdout = "PAROLA_MODULE=no\n\
[\n\
  { \"HotFixID\": \"KB5044284\", \"Description\": \"Security Update\", \"InstalledOn\": \"2026-07-10\" },\n\
  { \"HotFixID\": \"KB5043935\", \"Description\": \"Update\", \"InstalledOn\": \"2026-06-15\" }\n\
]\n";
        let (present, history) = parse_windows_first_round(&output(stdout));
        assert!(!present);
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].id, "KB5044284");
        assert_eq!(history[0].installed_on.as_deref(), Some("2026-07-10"));

        // A single hotfix arrives as an object, not a one-element array.
        let single = "PAROLA_MODULE=yes\n{ \"HotFixID\": \"KB1\", \"Description\": \"D\" }\n";
        let (present, history) = parse_windows_first_round(&output(single));
        assert!(present);
        assert_eq!(history.len(), 1);
        assert!(history[0].installed_on.is_none());
    }

    #[test]
    fn windows_pending_list_is_parsed_and_empty_means_up_to_date() {
        let report = parse_windows_pending(&output(
            "[ { \"Title\": \"2026-07 Security Update (KB5044284)\", \"KB\": \"KB5044284\", \"Size\": \"120MB\" } ]",
        ));
        let UpdateReport::List { updates, security_count, .. } = report else {
            panic!("expected a list");
        };
        assert_eq!(updates.len(), 1);
        assert!(updates[0].security);
        assert_eq!(security_count, Some(1));

        let empty = parse_windows_pending(&output(""));
        assert!(matches!(empty, UpdateReport::UpToDate { .. }));
    }

    #[test]
    fn macos_and_unknown_are_refused_rather_than_guessed() {
        assert!(check_command(OsFamily::Macos).is_err());
        assert!(check_command(OsFamily::Bsd).is_err());
        assert!(check_command(OsFamily::Unknown).is_err());
    }
}
