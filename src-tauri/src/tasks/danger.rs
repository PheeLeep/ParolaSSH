//! Reading a command for the shapes that end badly.
//!
//! **This is a typo catcher, not a security boundary.** It matches text. A
//! command that hides its intent — behind a variable, a base64 blob, a script
//! it downloads — walks straight past every rule here, and no amount of added
//! patterns changes that. The operator writes these commands and holds the
//! credentials to run them by hand; the threat being defended against is the
//! stray `/`, the pasted line from a forum, the task written for the wrong
//! host. It never blocks: it raises the cost of pressing the button, and says
//! exactly why, and the operator decides.
//!
//! That framing decides the tuning. A rule earns its place by catching a
//! plausible *mistake*; rules that only fire on deliberate misuse are noise,
//! and noise is what teaches people to click through warnings.
//!
//! Matching is on a normalised copy — lowercased, whitespace collapsed — while
//! every message quotes the operator's original wording.

use serde::Serialize;

use crate::remote::OsFamily;

/// How much a command deserves to be stopped at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DangerLevel {
    /// Nothing matched. Not a claim that the command is safe.
    None,
    /// Worth a second look: disruptive, hard to undo, or wider than it reads.
    Caution,
    /// Data loss, or a machine that does not come back. Typed confirmation.
    Destructive,
}

impl DangerLevel {
    pub fn is_none(self) -> bool {
        matches!(self, Self::None)
    }
}

/// One rule that fired, in the operator's terms rather than the pattern's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DangerReason {
    /// Short label for the badge — "Recursive delete", "Formats a disk".
    pub label: String,
    /// What the app thinks this does, and why that is worth a pause.
    pub detail: String,
    pub level: DangerLevel,
}

/// The verdict on one command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DangerAssessment {
    /// The highest level any rule reached.
    pub level: DangerLevel,
    pub reasons: Vec<DangerReason>,
}

impl DangerAssessment {
    fn none() -> Self {
        Self {
            level: DangerLevel::None,
            reasons: Vec::new(),
        }
    }
}

/// Paths whose recursive deletion takes the machine, not just some files.
const CRITICAL_PATHS: &[&str] = &[
    "/", "/*", "/bin", "/boot", "/dev", "/etc", "/home", "/lib", "/opt", "/root", "/sbin", "/srv",
    "/usr", "/var",
];

/// Assess a command for a host of this OS family. Pure.
///
/// The family only selects which rule set applies: a `del /f /s /q c:\` in a
/// task aimed at Linux is a broken task, not a dangerous one, and reporting it
/// as dangerous would be the wrong warning.
pub fn assess(os: OsFamily, command: &str) -> DangerAssessment {
    let normalised = normalise(command);
    let mut reasons = Vec::new();

    if os.is_unix() || os == OsFamily::Unknown {
        assess_unix(&normalised, &mut reasons);
    }
    if os == OsFamily::Windows || os == OsFamily::Unknown {
        assess_windows(&normalised, &mut reasons);
    }
    // Credentials on a command line leak the same way on every platform.
    assess_secrets(&normalised, &mut reasons);

    match reasons.iter().map(|reason| reason.level).max() {
        None => DangerAssessment::none(),
        Some(level) => {
            // Worst first: the badge shows the top reason, and a Caution listed
            // above a Destructive would understate the command. The sort is
            // stable, so reasons at the same level keep the order they fire in.
            reasons.sort_by_key(|reason| std::cmp::Reverse(reason.level));
            DangerAssessment { level, reasons }
        }
    }
}

/// Lowercase, collapse runs of whitespace, and normalise the separators a
/// shell treats as "next command" so `rm  -rf   /` and `rm -rf /` read alike.
///
/// Newlines become spaces: a two-line script is two commands, and both are
/// being assessed.
fn normalise(command: &str) -> String {
    let lowered = command.to_lowercase();
    let mut out = String::with_capacity(lowered.len());
    let mut in_space = false;

    for character in lowered.chars() {
        if character.is_whitespace() {
            in_space = true;
            continue;
        }
        if in_space && !out.is_empty() {
            out.push(' ');
        }
        in_space = false;
        out.push(character);
    }
    out
}

fn push(reasons: &mut Vec<DangerReason>, level: DangerLevel, label: &str, detail: &str) {
    // A pattern that fires twice in one command is still one reason.
    if reasons.iter().any(|reason| reason.label == label) {
        return;
    }
    reasons.push(DangerReason {
        label: label.to_string(),
        detail: detail.to_string(),
        level,
    });
}

fn assess_unix(text: &str, reasons: &mut Vec<DangerReason>) {
    // ---- recursive delete -------------------------------------------------
    if let Some(targets) = recursive_delete_targets(text) {
        let critical: Vec<&str> = targets
            .iter()
            .copied()
            .filter(|target| CRITICAL_PATHS.contains(target))
            .collect();

        if !critical.is_empty() {
            push(
                reasons,
                DangerLevel::Destructive,
                "Recursive delete of a system path",
                &format!(
                    "`rm` is set to recurse and force, and one of its targets is {}. \
                     There is no undo and no confirmation prompt on the host.",
                    critical.join(", ")
                ),
            );
        } else {
            push(
                reasons,
                DangerLevel::Caution,
                "Recursive delete",
                "`rm -r` with `-f` removes a whole tree without asking. Check the \
                 path is the one you mean — a trailing slash or an unset variable \
                 can widen it.",
            );
        }
    }

    // An unquoted variable next to a recursive delete is the classic way a
    // narrow command becomes a wide one.
    if (text.contains("rm -rf $") || text.contains("rm -fr $") || text.contains("rm -r -f $"))
        && !text.contains("rm -rf \"$")
    {
        push(
            reasons,
            DangerLevel::Destructive,
            "Delete path comes from a variable",
            "If the variable is empty or unset, the shell expands this to a delete \
             of the current directory or of `/`. Quote it, or set it in the same \
             command.",
        );
    }

    // ---- whole-device writes ---------------------------------------------
    if text.contains("mkfs") || text.contains("mke2fs") {
        push(
            reasons,
            DangerLevel::Destructive,
            "Formats a filesystem",
            "`mkfs` writes a new filesystem over whatever the device held. Every \
             file on it is gone the moment it completes.",
        );
    }
    if text.contains("of=/dev/sd")
        || text.contains("of=/dev/nvme")
        || text.contains("of=/dev/vd")
        || text.contains("of=/dev/hd")
        || text.contains("of=/dev/disk")
    {
        push(
            reasons,
            DangerLevel::Destructive,
            "Writes directly to a disk",
            "`dd` to a whole device bypasses the filesystem and overwrites the \
             partition table with it. The wrong device letter destroys the wrong \
             disk, and both look identical until it is too late.",
        );
    }
    if text.contains("> /dev/sd") || text.contains(">/dev/sd") {
        push(
            reasons,
            DangerLevel::Destructive,
            "Redirects output onto a disk device",
            "Anything written to a raw block device overwrites the data at that \
             offset, including the partition table.",
        );
    }

    // ---- the machine stops answering -------------------------------------
    if starts_or_follows(text, "shutdown")
        || starts_or_follows(text, "poweroff")
        || starts_or_follows(text, "halt")
        || starts_or_follows(text, "reboot")
        || text.contains("init 0")
        || text.contains("init 6")
        || text.contains("systemctl isolate")
    {
        push(
            reasons,
            DangerLevel::Caution,
            "Stops or restarts the machine",
            "Every session on this host ends, including this one, and the host has \
             to come back by itself. ParolaSSH has a Power pane that shows the \
             pending job and can cancel it.",
        );
    }
    // Restarting sshd is the one that locks you out of your own recovery.
    if text.contains("restart ssh") || text.contains("restart sshd") || text.contains("stop ssh") {
        push(
            reasons,
            DangerLevel::Caution,
            "Restarts the SSH service",
            "This is the service carrying this session. A bad `sshd_config` takes \
             the daemon down with no way back in over SSH — have console access \
             ready, or run `sshd -t` first.",
        );
    }

    // ---- the host stops defending itself ---------------------------------
    if text.contains("iptables -f")
        || text.contains("nft flush ruleset")
        || text.contains("ufw disable")
        || text.contains("systemctl stop firewalld")
        || text.contains("setenforce 0")
    {
        push(
            reasons,
            DangerLevel::Destructive,
            "Disables host protection",
            "This drops the firewall or SELinux enforcement for everyone, not just \
             for what you are debugging, and it stays down until something puts it \
             back.",
        );
    }

    // ---- permissions -----------------------------------------------------
    if text.contains("chmod -r 777") || text.contains("chmod 777 /") || text.contains("chmod -r a+rwx")
    {
        push(
            reasons,
            DangerLevel::Destructive,
            "Makes files world-writable",
            "Mode 777 lets every account on the host rewrite these files. Applied \
             recursively it is close to impossible to undo, because the original \
             modes are not recorded anywhere.",
        );
    }
    if (text.contains("chown -r") || text.contains("chmod -r")) && touches_system_root(text) {
        push(
            reasons,
            DangerLevel::Destructive,
            "Rewrites ownership across a system path",
            "Recursive `chown`/`chmod` over a system directory breaks `sudo`, SSH \
             key permissions, and setuid binaries. Recovery usually means a \
             reinstall.",
        );
    }

    // ---- code from the network -------------------------------------------
    if pipes_download_to_shell(text) {
        push(
            reasons,
            DangerLevel::Destructive,
            "Runs code downloaded from the network",
            "Whatever the URL returns today executes on this host — nothing is \
             pinned, reviewed, or logged. If the task is worth keeping, fetch the \
             script, read it, then run it.",
        );
    }

    // ---- accounts and evidence -------------------------------------------
    if starts_or_follows(text, "userdel") || starts_or_follows(text, "groupdel") {
        push(
            reasons,
            DangerLevel::Caution,
            "Removes an account",
            "Files owned by the account are left behind with a numeric owner, and \
             anything running as it stops working.",
        );
    }
    if text.contains("rm -rf /var/log")
        || text.contains("history -c")
        || text.contains("truncate -s 0 /var/log")
        || text.contains("journalctl --vacuum-time=0")
    {
        push(
            reasons,
            DangerLevel::Caution,
            "Erases the record of what happened",
            "Logs are how the next incident gets diagnosed. Rotating them is a \
             normal job; deleting them removes the evidence with the disk usage.",
        );
    }
    if text.contains("> /etc/passwd")
        || text.contains(">/etc/passwd")
        || text.contains("> /etc/shadow")
        || text.contains(">/etc/shadow")
        || text.contains("> /etc/fstab")
        || text.contains(">/etc/fstab")
    {
        push(
            reasons,
            DangerLevel::Destructive,
            "Overwrites a critical system file",
            "A single `>` truncates the file before anything is written. Losing \
             `/etc/passwd`, `/etc/shadow` or `/etc/fstab` means the host does not \
             boot or nobody can log in.",
        );
    }

    // ---- blunt process control -------------------------------------------
    if starts_or_follows(text, "killall") || text.contains("pkill -9") || text.contains("kill -9 -1")
    {
        push(
            reasons,
            DangerLevel::Caution,
            "Kills processes by name",
            "`-9` gives nothing a chance to flush or shut down cleanly, and a name \
             match can hit more processes than intended.",
        );
    }

    // ---- package removal --------------------------------------------------
    if text.contains("apt-get remove")
        || text.contains("apt remove")
        || text.contains("apt-get purge")
        || text.contains("apt purge")
        || text.contains("dnf remove")
        || text.contains("yum remove")
        || text.contains("pacman -r")
        || text.contains("zypper remove")
    {
        push(
            reasons,
            DangerLevel::Caution,
            "Removes installed packages",
            "Dependency resolution can take far more with it than the package \
             named — run it once by hand and read the list before saving it as a \
             one-click task.",
        );
    }

    // ---- the classics -----------------------------------------------------
    if text.replace(' ', "").contains(":(){:|:&};:") {
        push(
            reasons,
            DangerLevel::Destructive,
            "Fork bomb",
            "This spawns processes until the host cannot start another one. It \
             does not stop on its own and usually needs a hard reset.",
        );
    }
}

fn assess_windows(text: &str, reasons: &mut Vec<DangerReason>) {
    if starts_or_follows(text, "format ") || text.contains("format c:") {
        push(
            reasons,
            DangerLevel::Destructive,
            "Formats a volume",
            "Everything on the volume is gone when this completes.",
        );
    }
    if (text.contains("del /f") || text.contains("del /s") || text.contains("rd /s"))
        && (text.contains("c:\\") || text.contains("%systemroot%") || text.contains("%windir%"))
    {
        push(
            reasons,
            DangerLevel::Destructive,
            "Recursive delete of a system path",
            "A forced recursive delete under the system drive removes files \
             Windows needs to start.",
        );
    }
    if text.contains("remove-item") && text.contains("-recurse") && text.contains("-force") {
        push(
            reasons,
            DangerLevel::Caution,
            "Recursive delete",
            "`-Recurse -Force` deletes the whole tree without confirmation, \
             including read-only and hidden items.",
        );
    }
    if text.contains("vssadmin delete shadows") || text.contains("wbadmin delete catalog") {
        push(
            reasons,
            DangerLevel::Destructive,
            "Deletes backups and shadow copies",
            "This removes the restore points that would undo any other mistake on \
             this host. It is also the exact move ransomware makes first.",
        );
    }
    if text.contains("advfirewall set") && text.contains("state off") {
        push(
            reasons,
            DangerLevel::Destructive,
            "Disables the firewall",
            "Every profile turned off applies to the whole machine and stays off \
             until something turns it back on.",
        );
    }
    if starts_or_follows(text, "shutdown /s")
        || starts_or_follows(text, "shutdown /r")
        || text.contains("restart-computer")
        || text.contains("stop-computer")
    {
        push(
            reasons,
            DangerLevel::Caution,
            "Stops or restarts the machine",
            "Every session on this host ends, including this one. The Power pane \
             shows a pending job and can cancel it.",
        );
    }
    if text.contains("downloadstring") || (text.contains("irm ") && text.contains("| iex")) {
        push(
            reasons,
            DangerLevel::Destructive,
            "Runs code downloaded from the network",
            "Whatever the URL returns today executes on this host — nothing is \
             pinned, reviewed, or logged.",
        );
    }
    if starts_or_follows(text, "bcdedit") || starts_or_follows(text, "diskpart") {
        push(
            reasons,
            DangerLevel::Caution,
            "Edits boot or partition configuration",
            "Mistakes here are usually only visible at the next boot, which is \
             also when they stop the host coming back.",
        );
    }
}

/// A password on a command line is readable in the remote process list by every
/// account on the host, and lands in shell history. This app sends secrets to
/// stdin everywhere else; a task should not be the exception by accident.
fn assess_secrets(text: &str, reasons: &mut Vec<DangerReason>) {
    let leaks = text.contains("sshpass -p")
        || text.contains("--password=")
        || text.contains("--password ")
        || text.contains("-pass pass:")
        || text.contains("mysql -p")
        || text.contains("curl -u ")
        || text.contains("convertto-securestring -asplaintext");

    if leaks {
        push(
            reasons,
            DangerLevel::Caution,
            "Password on the command line",
            "Arguments are visible to every account on the host through `ps`, and \
             they are written to shell history. Read the secret from a file or an \
             environment variable the task sets instead.",
        );
    }
}

/// Whether `rm` is being run recursively *and* forced, and what it points at.
///
/// Both flags are required before anything is reported: `rm -r` alone prompts,
/// and `rm -f` alone cannot take a tree. Flags are matched in the combined
/// (`-rf`) and separate (`-r -f`) spellings, plus the long forms.
fn recursive_delete_targets(text: &str) -> Option<Vec<&str>> {
    let mut words = text.split(' ').peekable();
    let mut found = false;

    while let Some(word) = words.next() {
        if word != "rm" {
            continue;
        }

        let mut recursive = false;
        let mut forced = false;
        let mut targets = Vec::new();

        for rest in words.by_ref() {
            if rest.starts_with("--") {
                if rest == "--recursive" {
                    recursive = true;
                } else if rest == "--force" {
                    forced = true;
                }
                continue;
            }
            if let Some(flags) = rest.strip_prefix('-') {
                if flags.contains('r') || flags.contains('R') {
                    recursive = true;
                }
                if flags.contains('f') {
                    forced = true;
                }
                continue;
            }
            // A separator ends this `rm`'s argument list; the loop then looks
            // for the next `rm` in the same string.
            if matches!(rest, "&&" | "||" | ";" | "|") {
                break;
            }
            targets.push(rest.trim_end_matches(';'));
        }

        if recursive && forced {
            found = true;
            if !targets.is_empty() {
                return Some(targets);
            }
        }
    }

    // `rm -rf` with the path in a variable still counts, with no target to name.
    found.then(Vec::new)
}

/// Whether a recursive ownership/permission change points at a system path.
fn touches_system_root(text: &str) -> bool {
    CRITICAL_PATHS
        .iter()
        .filter(|path| **path != "/" && **path != "/*")
        .any(|path| text.contains(&format!(" {path}")))
        || text.contains(" /")
            && text
                .split(' ')
                .any(|word| word == "/" || word == "/*" || word == "~")
}

/// A download piped straight into an interpreter, in any of its spellings.
fn pipes_download_to_shell(text: &str) -> bool {
    let downloads = text.contains("curl ") || text.contains("wget ") || text.contains("fetch ");
    if !downloads {
        return false;
    }
    ["| sh", "|sh", "| bash", "|bash", "| zsh", "| python", "|python", "| perl"]
        .iter()
        .any(|shell| text.contains(shell))
}

/// Whether `verb` appears as a command rather than inside another word — at the
/// start, or after a separator. Keeps `reboot` from firing on `reboot-required`
/// and `halt` from firing on `--halt-on-error`.
fn starts_or_follows(text: &str, verb: &str) -> bool {
    let verb = verb.trim_end();
    for (index, _) in text.match_indices(verb) {
        let before_ok = match index {
            0 => true,
            _ => {
                let preceding = text[..index].trim_end();
                preceding.is_empty()
                    || preceding.ends_with(';')
                    || preceding.ends_with('&')
                    || preceding.ends_with('|')
                    || preceding.ends_with("sudo")
                    || preceding.ends_with("doas")
            }
        };
        let after = &text[index + verb.len()..];
        let after_ok = after.is_empty() || after.starts_with(' ') || after.starts_with(';');

        if before_ok && after_ok {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn linux(command: &str) -> DangerAssessment {
        assess(OsFamily::Linux, command)
    }

    fn labels(assessment: &DangerAssessment) -> Vec<String> {
        assessment
            .reasons
            .iter()
            .map(|reason| reason.label.clone())
            .collect()
    }

    #[test]
    fn ordinary_commands_raise_nothing() {
        for command in [
            "df -h",
            "uptime",
            "systemctl status sshd",
            "journalctl -u nginx --since '1 hour ago'",
            "ls -la /var/log",
            "tar czf /backup/srv.tgz /srv",
            "docker ps -a",
        ] {
            let assessment = linux(command);
            assert!(
                assessment.level.is_none(),
                "{command} should be clean, got {:?}",
                labels(&assessment)
            );
        }
    }

    #[test]
    fn recursive_delete_of_root_is_destructive() {
        for command in ["rm -rf /", "rm -fr /*", "rm -r -f /etc", "rm --recursive --force /usr"] {
            let assessment = linux(command);
            assert_eq!(
                assessment.level,
                DangerLevel::Destructive,
                "{command} → {:?}",
                labels(&assessment)
            );
        }
    }

    #[test]
    fn a_scoped_recursive_delete_is_only_caution() {
        let assessment = linux("rm -rf /srv/app/cache");
        assert_eq!(assessment.level, DangerLevel::Caution);
        assert_eq!(labels(&assessment), vec!["Recursive delete"]);
    }

    #[test]
    fn a_non_forced_or_non_recursive_delete_says_nothing() {
        // `rm -r` prompts, `rm -f` cannot take a tree. Neither alone is the
        // shape that ruins a machine, and warning on them trains people to
        // click through.
        assert!(linux("rm -r /srv/old").level.is_none());
        assert!(linux("rm -f /tmp/lockfile").level.is_none());
        assert!(linux("rm /tmp/one-file").level.is_none());
    }

    #[test]
    fn an_unquoted_variable_target_is_called_out_by_name() {
        let assessment = linux("rm -rf $BUILD_DIR/dist");
        assert_eq!(assessment.level, DangerLevel::Destructive);
        assert!(labels(&assessment).contains(&"Delete path comes from a variable".to_string()));

        // Quoted, it is an ordinary recursive delete again.
        let quoted = linux("rm -rf \"$BUILD_DIR/dist\"");
        assert_eq!(quoted.level, DangerLevel::Caution);
    }

    #[test]
    fn disk_and_filesystem_writes_are_destructive() {
        assert_eq!(linux("mkfs.ext4 /dev/sdb1").level, DangerLevel::Destructive);
        assert_eq!(
            linux("dd if=/dev/zero of=/dev/sda bs=1M").level,
            DangerLevel::Destructive
        );
        assert_eq!(linux("cat image >/dev/sdb").level, DangerLevel::Destructive);
    }

    #[test]
    fn downloads_piped_into_a_shell_are_destructive() {
        assert_eq!(
            linux("curl -fsSL https://example.com/i.sh | sh").level,
            DangerLevel::Destructive
        );
        assert_eq!(
            linux("wget -qO- https://example.com/i.sh | bash").level,
            DangerLevel::Destructive
        );
        // A download that is only saved is not the same thing.
        assert!(linux("curl -fsSL https://example.com/i.sh -o /tmp/i.sh")
            .level
            .is_none());
    }

    #[test]
    fn the_fork_bomb_is_recognised_through_its_spacing() {
        assert_eq!(linux(":(){ :|:& };:").level, DangerLevel::Destructive);
        assert_eq!(linux(":(){:|:&};:").level, DangerLevel::Destructive);
    }

    #[test]
    fn restarting_sshd_warns_that_it_carries_this_session() {
        let assessment = linux("systemctl restart sshd");
        assert_eq!(assessment.level, DangerLevel::Caution);
        assert!(assessment.reasons[0].detail.contains("sshd_config"));
    }

    #[test]
    fn a_verb_inside_another_word_does_not_fire() {
        // The false positives that would make the feature untrustworthy.
        assert!(linux("cat /var/run/reboot-required").level.is_none());
        assert!(linux("make --halt-on-error build").level.is_none());
        assert!(linux("grep shutdown /var/log/syslog").level.is_none());
        assert!(linux("ls /etc/init.d").level.is_none());
    }

    #[test]
    fn a_password_argument_is_flagged_on_any_platform() {
        for os in [OsFamily::Linux, OsFamily::Windows] {
            let assessment = assess(os, "mysqldump --password=hunter2 app > dump.sql");
            assert_eq!(assessment.level, DangerLevel::Caution, "{os:?}");
            assert!(labels(&assessment).contains(&"Password on the command line".to_string()));
        }
    }

    #[test]
    fn the_rule_set_follows_the_host_os() {
        // A Windows delete aimed at a Linux host is a broken task, not a
        // dangerous one — warning about it would be the wrong warning.
        assert!(assess(OsFamily::Linux, "del /f /s /q c:\\temp").level.is_none());
        assert_eq!(
            assess(OsFamily::Windows, "del /f /s /q c:\\windows").level,
            DangerLevel::Destructive
        );

        // An unidentified host gets both sets: nothing is known about it, so
        // the more cautious reading is the honest one.
        assert_eq!(
            assess(OsFamily::Unknown, "rm -rf /").level,
            DangerLevel::Destructive
        );
        assert_eq!(
            assess(OsFamily::Unknown, "format c:").level,
            DangerLevel::Destructive
        );
    }

    #[test]
    fn windows_shadow_copy_deletion_is_destructive() {
        let assessment = assess(OsFamily::Windows, "vssadmin delete shadows /all /quiet");
        assert_eq!(assessment.level, DangerLevel::Destructive);
    }

    #[test]
    fn the_worst_reason_is_listed_first() {
        // Two rules fire at two levels; the badge takes the top one.
        let assessment = linux("rm -rf / && history -c");
        assert_eq!(assessment.level, DangerLevel::Destructive);
        assert_eq!(assessment.reasons[0].level, DangerLevel::Destructive);
        assert!(assessment.reasons.len() >= 2);
        assert!(assessment
            .reasons
            .last()
            .is_some_and(|reason| reason.level == DangerLevel::Caution));
    }

    #[test]
    fn deleting_the_log_directory_is_caution_not_destruction() {
        // `/var` is a critical path; `/var/log` is not the same claim. Losing
        // the logs is bad and recoverable, and calling it destruction would
        // spend the word that `rm -rf /` needs.
        let assessment = linux("rm -rf /var/log");
        assert_eq!(assessment.level, DangerLevel::Caution);
        assert!(labels(&assessment).contains(&"Erases the record of what happened".to_string()));
    }

    #[test]
    fn a_rule_firing_twice_is_still_one_reason() {
        let assessment = linux("rm -rf /etc && rm -rf /usr");
        let deletes = assessment
            .reasons
            .iter()
            .filter(|reason| reason.label.contains("Recursive delete"))
            .count();
        assert_eq!(deletes, 1);
    }

    #[test]
    fn casing_and_spacing_do_not_hide_a_match() {
        assert_eq!(linux("RM   -rf    /").level, DangerLevel::Destructive);
        assert_eq!(linux("Mkfs.ext4 /dev/sdb1").level, DangerLevel::Destructive);
    }

    #[test]
    fn every_line_of_a_multi_line_script_is_assessed() {
        let assessment = linux("cd /tmp\nls -la\nrm -rf /");
        assert_eq!(assessment.level, DangerLevel::Destructive);
    }
}
