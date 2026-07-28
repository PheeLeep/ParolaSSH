//! The tasks ParolaSSH ships.
//!
//! Every entry here is authored, per OS, in this file — never assembled from
//! user input — and the whole catalog obeys two rules:
//!
//! * **Nothing is installed.** No entry carries a package-manager verb. A task
//!   that only works once something is installed reports that it is missing
//!   and stops; the operator installs it in a terminal, deliberately.
//! * **Nothing is changed by default.** The built-ins answer questions. The
//!   two that act (`restart-ssh`, `clear-package-cache`) are marked
//!   `elevated` and carry their own warning through `danger.rs` like any
//!   other command, because being shipped by the app does not make a
//!   disruptive command less disruptive.
//!
//! A family with no entry for a task simply does not see it — `None` here is
//! how macOS and BSD opt out of systemd phrasing, the same refusal `services`
//! makes rather than guessing at launchd.

use serde::Serialize;

use crate::remote::OsFamily;

/// One shipped task. Commands are `'static` because they are literals in this
/// file; nothing interpolates into them.
#[derive(Debug, Clone, Copy)]
pub struct BuiltinTask {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    /// Whether the task is worth running at all without root. `true` means the
    /// answer is materially incomplete unprivileged — it is still the
    /// operator's choice at run time, this is only the default the UI offers.
    pub elevated: bool,
    pub linux: Option<&'static str>,
    pub macos: Option<&'static str>,
    pub windows: Option<&'static str>,
}

impl BuiltinTask {
    /// The command for this family, or `None` when the task does not apply.
    pub fn command_for(&self, os: OsFamily) -> Option<&'static str> {
        match os {
            OsFamily::Linux => self.linux,
            OsFamily::Macos => self.macos,
            // BSD shares the portable phrasing where there is one, and gets
            // nothing where there is not. Falling back to the Linux entry
            // would hand it systemd commands, which is the guess `services`
            // already refuses to make.
            OsFamily::Bsd => self.macos,
            OsFamily::Windows => self.windows,
            // Nothing is offered for a host whose OS could not be identified:
            // picking a family and hoping is how the wrong command runs.
            OsFamily::Unknown => None,
        }
    }
}

/// A built-in flattened for one host's OS — what the frontend actually lists.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuiltinTaskView {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub command: &'static str,
    pub elevated: bool,
}

pub const BUILTIN_TASKS: &[BuiltinTask] = &[
    BuiltinTask {
        id: "disk-usage",
        name: "Disk usage",
        description: "Free space per filesystem, then the ten largest directories under /.",
        elevated: false,
        // `-x` keeps the walk on one filesystem, so a mounted NFS share or a
        // container overlay does not turn this into a minutes-long crawl.
        linux: Some("df -h; echo; du -xh / 2>/dev/null | sort -rh | head -10"),
        macos: Some("df -h; echo; du -xh / 2>/dev/null | sort -rh | head -10"),
        windows: Some(
            "Get-PSDrive -PSProvider FileSystem | \
             Select-Object Name,@{n='UsedGB';e={[math]::Round($_.Used/1GB,1)}},\
             @{n='FreeGB';e={[math]::Round($_.Free/1GB,1)}} | Format-Table -AutoSize",
        ),
    },
    BuiltinTask {
        id: "top-processes",
        name: "Top processes",
        description: "The ten processes using the most CPU, and the ten using the most memory.",
        elevated: false,
        linux: Some(
            "echo '== by CPU =='; ps -eo pid,user,pcpu,pmem,comm --sort=-pcpu | head -11; \
             echo; echo '== by memory =='; ps -eo pid,user,pcpu,pmem,comm --sort=-pmem | head -11",
        ),
        macos: Some(
            "echo '== by CPU =='; ps -Aceo pid,user,pcpu,pmem,comm -r | head -11; \
             echo; echo '== by memory =='; ps -Aceo pid,user,pcpu,pmem,comm -m | head -11",
        ),
        windows: Some(
            "Get-Process | Sort-Object CPU -Descending | Select-Object -First 10 \
             Id,ProcessName,CPU,WS | Format-Table -AutoSize",
        ),
    },
    BuiltinTask {
        id: "failed-services",
        name: "Failed services",
        description: "Units systemd could not start, or Windows services set to auto-start that are not running.",
        elevated: false,
        linux: Some("systemctl list-units --state=failed --no-pager --no-legend || echo 'No systemd on this host.'"),
        // launchd has no equivalent listing worth phrasing this way.
        macos: None,
        windows: Some(
            "Get-Service | Where-Object { $_.StartType -eq 'Automatic' -and $_.Status -ne 'Running' } | \
             Select-Object Name,DisplayName,Status | Format-Table -AutoSize",
        ),
    },
    BuiltinTask {
        id: "listening-ports",
        name: "Listening ports",
        description: "Every socket accepting connections, with the process behind it.",
        // Without root the socket list still appears; the owning process does
        // not, which is usually the part being looked for.
        elevated: true,
        linux: Some("ss -tulpn 2>/dev/null || netstat -tulpn 2>/dev/null || echo 'Neither ss nor netstat is installed.'"),
        macos: Some("netstat -an -p tcp | grep LISTEN"),
        windows: Some("Get-NetTCPConnection -State Listen | Select-Object LocalAddress,LocalPort,OwningProcess | Format-Table -AutoSize"),
    },
    BuiltinTask {
        id: "who-is-here",
        name: "Who is logged in",
        description: "Current sessions and the last ten logins.",
        elevated: false,
        linux: Some("who -a; echo; last -n 10 2>/dev/null || echo 'No login history available.'"),
        macos: Some("who -a; echo; last -n 10"),
        windows: Some("query user 2>$null; if ($LASTEXITCODE -ne 0) { 'No interactive sessions.' }"),
    },
    BuiltinTask {
        id: "recent-errors",
        name: "Recent errors",
        description: "The last fifty error-level entries from the system log.",
        // journald restricts the full journal to root and the `adm` group.
        elevated: true,
        linux: Some("journalctl -p err -n 50 --no-pager 2>/dev/null || tail -n 50 /var/log/syslog 2>/dev/null || echo 'No readable system log.'"),
        macos: Some("log show --last 1h --style compact 2>/dev/null | grep -i error | tail -50"),
        windows: Some(
            "Get-WinEvent -FilterHashtable @{LogName='System';Level=1,2} -MaxEvents 50 | \
             Select-Object TimeCreated,ProviderName,Message | Format-List",
        ),
    },
    BuiltinTask {
        id: "reboot-required",
        name: "Is a reboot pending",
        description: "Whether the host is waiting on a restart to finish applying updates.",
        elevated: false,
        linux: Some(
            "if [ -f /var/run/reboot-required ]; then cat /var/run/reboot-required; \
             elif command -v needs-restarting >/dev/null 2>&1; then needs-restarting -r; \
             else echo 'No pending-reboot marker on this host.'; fi",
        ),
        macos: None,
        windows: Some(
            "if (Test-Path 'HKLM:\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\WindowsUpdate\\Auto Update\\RebootRequired') \
             { 'A reboot is pending.' } else { 'No reboot pending.' }",
        ),
    },
    BuiltinTask {
        id: "restart-ssh",
        name: "Restart the SSH service",
        description: "Validates the config first, and stops if it does not parse — a bad config would otherwise end every route back in.",
        elevated: true,
        // `sshd -t` before the restart is the whole point of shipping this
        // rather than leaving it to a hand-typed `systemctl restart`.
        linux: Some("sshd -t && systemctl restart sshd && systemctl --no-pager status sshd"),
        macos: None,
        windows: Some("Restart-Service sshd -PassThru | Select-Object Name,Status"),
    },
    BuiltinTask {
        id: "clear-package-cache",
        name: "Clear the package cache",
        description: "Frees the space taken by downloaded package archives. Installs nothing and removes no installed package.",
        elevated: true,
        linux: Some(
            "if command -v apt-get >/dev/null 2>&1; then apt-get clean && echo 'apt cache cleared.'; \
             elif command -v dnf >/dev/null 2>&1; then dnf clean packages; \
             elif command -v pacman >/dev/null 2>&1; then pacman -Sc --noconfirm; \
             else echo 'No supported package manager found.'; fi",
        ),
        macos: None,
        windows: None,
    },
];

/// The built-ins that apply to this OS, flattened with their command.
pub fn for_os(os: OsFamily) -> Vec<BuiltinTaskView> {
    BUILTIN_TASKS
        .iter()
        .filter_map(|task| {
            task.command_for(os).map(|command| BuiltinTaskView {
                id: task.id,
                name: task.name,
                description: task.description,
                command,
                elevated: task.elevated,
            })
        })
        .collect()
}

/// Look one up by id for a given OS. `None` covers both "no such task" and
/// "not offered on this OS", which the caller reports the same way.
pub fn find(id: &str, os: OsFamily) -> Option<BuiltinTaskView> {
    BUILTIN_TASKS
        .iter()
        .find(|task| task.id == id)
        .and_then(|task| {
            task.command_for(os).map(|command| BuiltinTaskView {
                id: task.id,
                name: task.name,
                description: task.description,
                command,
                elevated: task.elevated,
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::danger::{assess, DangerLevel};

    #[test]
    fn ids_are_unique() {
        let mut ids: Vec<_> = BUILTIN_TASKS.iter().map(|task| task.id).collect();
        ids.sort_unstable();
        let count = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), count, "two built-ins share an id");
    }

    #[test]
    fn no_builtin_installs_anything() {
        // The rule the removed Lynis tier existed to keep. Asserted rather
        // than documented, so a future entry cannot quietly break it.
        const FORBIDDEN: &[&str] = &[
            "apt-get install",
            "apt install",
            "dnf install",
            "yum install",
            // Trailing space on purpose: `pacman -S pkg` installs, `pacman -Sc`
            // clears the cache, and the two differ by exactly that character.
            "pacman -s ",
            "zypper install",
            "brew install",
            "pip install",
            "npm install",
            "winget install",
            "choco install",
            "curl | sh",
        ];

        for task in BUILTIN_TASKS {
            for command in [task.linux, task.macos, task.windows].into_iter().flatten() {
                let lowered = command.to_lowercase();
                for verb in FORBIDDEN {
                    assert!(
                        !lowered.contains(verb),
                        "built-in `{}` contains `{verb}`",
                        task.id
                    );
                }
            }
        }
    }

    #[test]
    fn no_builtin_is_destructive() {
        // A shipped task may be disruptive — restarting sshd is — but nothing
        // the app authors should reach the level that demands typed
        // confirmation. If a new entry does, it needs a deliberate decision,
        // not a passing test.
        for task in BUILTIN_TASKS {
            for (os, command) in [
                (OsFamily::Linux, task.linux),
                (OsFamily::Macos, task.macos),
                (OsFamily::Windows, task.windows),
            ] {
                let Some(command) = command else { continue };
                let assessment = assess(os, command);
                assert_ne!(
                    assessment.level,
                    DangerLevel::Destructive,
                    "built-in `{}` on {os:?} assessed destructive: {:?}",
                    task.id,
                    assessment.reasons
                );
            }
        }
    }

    #[test]
    fn an_unknown_os_is_offered_nothing() {
        assert!(
            for_os(OsFamily::Unknown).is_empty(),
            "a host whose OS is unidentified must not be offered a guess"
        );
    }

    #[test]
    fn each_family_gets_only_what_was_written_for_it() {
        let linux = for_os(OsFamily::Linux);
        assert!(linux.iter().any(|task| task.id == "clear-package-cache"));

        // macOS has no systemd phrasing, so those entries are absent rather
        // than approximated.
        let macos = for_os(OsFamily::Macos);
        assert!(!macos.iter().any(|task| task.id == "failed-services"));
        assert!(!macos.iter().any(|task| task.id == "restart-ssh"));
        assert!(macos.iter().any(|task| task.id == "disk-usage"));

        let windows = for_os(OsFamily::Windows);
        assert!(windows.iter().any(|task| task.id == "failed-services"));
        assert!(!windows.iter().any(|task| task.id == "clear-package-cache"));
    }

    #[test]
    fn lookup_respects_the_os() {
        assert!(find("restart-ssh", OsFamily::Linux).is_some());
        assert!(find("restart-ssh", OsFamily::Macos).is_none());
        assert!(find("no-such-task", OsFamily::Linux).is_none());
    }

    #[test]
    fn bsd_falls_back_to_the_portable_phrasing() {
        // BSD gets the macOS entry where there is one, since both are closer
        // to each other than either is to systemd.
        assert!(find("disk-usage", OsFamily::Bsd).is_some());
        assert!(find("failed-services", OsFamily::Bsd).is_none());
    }
}
