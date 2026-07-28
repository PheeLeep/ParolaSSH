//! Remote security posture, tiered by what it costs the host.
//!
//! * **Tier 0 — free.** Facts from the key exchange russh already performed.
//!   No remote command runs. With russh's default preference lists a SHA-1 kex
//!   or CBC cipher can never be negotiated, so those rules are dormant guards;
//!   the ones that fire are `ssh-rsa` host key and missing strict kex.
//! * **Tier 1 — read-only commands.** `sshd -T` posture, `authorized_keys`
//!   permissions, world-writable PATH directories, and (only with elevation,
//!   which `/etc/shadow` demands) empty-password accounts. What could not be
//!   checked is named in a note, never guessed at.
//!
//! Report shape follows `ssh::audit`: same `Severity`, counts, and scoring.
//! Findings are not the local `Finding` type, whose `Remediation` can be an
//! action this app runs — remote remediation is instruction text only.

use std::collections::{BTreeMap, HashSet};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use super::client::NegotiatedCrypto;
use super::CommandOutput;
use crate::ssh::audit::{Severity, SeverityCounts};

/// Where remote dismissals persist, apart from the local audit's file.
pub const SUPPRESSIONS_FILE: &str = "remote-audit-suppressions.json";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteFinding {
    /// Stable across runs: `rule|target`. Suppressions key off
    /// `host_id|rule|target`, so a dismissal on one box does not affect others.
    pub id: String,
    pub rule_id: String,
    pub severity: Severity,
    pub title: String,
    pub detail: String,
    pub location: String,
    /// The command an operator would run to fix it. Shown, never executed.
    pub instruction: Option<String>,
    pub suppressed: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteAuditReport {
    pub host_id: String,
    pub findings: Vec<RemoteFinding>,
    pub counts: SeverityCounts,
    /// 0–100, suppressed findings not counted — same scale as the key audit.
    pub score: u32,
    /// Whether the tier-1 commands ran at all (they need a Unix host).
    pub tier1_ran: bool,
    /// What tier 1 could not check, and why. `None` when everything ran.
    pub tier1_note: Option<String>,
    pub checked_at_ms: i64,
}

/// A finding before id and suppression are resolved.
struct NewFinding {
    rule_id: &'static str,
    target: String,
    severity: Severity,
    title: String,
    detail: String,
    location: String,
    instruction: Option<String>,
}

// ------------------------------------------------------------------- tier 0

/// Evaluate the negotiated crypto. Pure.
fn tier0_findings(crypto: &NegotiatedCrypto) -> Vec<NewFinding> {
    let mut findings = Vec::new();

    const SHA1_KEX: &[&str] = &[
        "diffie-hellman-group1-sha1",
        "diffie-hellman-group14-sha1",
        "diffie-hellman-group-exchange-sha1",
    ];
    if SHA1_KEX.contains(&crypto.kex.as_str()) {
        findings.push(NewFinding {
            rule_id: "crypto.kex-sha1",
            target: crypto.kex.clone(),
            severity: Severity::High,
            title: "Key exchange uses SHA-1".to_string(),
            detail: format!(
                "The connection negotiated {}, which relies on SHA-1 — broken for \
                 collision resistance since 2017.",
                crypto.kex
            ),
            location: "key exchange".to_string(),
            instruction: Some(
                "Add modern KexAlgorithms (curve25519-sha256) to sshd_config and \
                 remove the SHA-1 groups."
                    .to_string(),
            ),
        });
    }

    if crypto.cipher.ends_with("-cbc") {
        findings.push(NewFinding {
            rule_id: "crypto.cipher-cbc",
            target: crypto.cipher.clone(),
            severity: Severity::High,
            title: "CBC-mode cipher negotiated".to_string(),
            detail: format!(
                "{} is a CBC-mode cipher, vulnerable to plaintext-recovery attacks \
                 in SSH; every current default uses AES-GCM or ChaCha20-Poly1305.",
                crypto.cipher
            ),
            location: "cipher".to_string(),
            instruction: Some(
                "Remove CBC ciphers from the Ciphers list in sshd_config.".to_string(),
            ),
        });
    }

    if crypto.host_key_algorithm == "ssh-rsa" {
        findings.push(NewFinding {
            rule_id: "crypto.hostkey-sha1",
            target: "ssh-rsa".to_string(),
            severity: Severity::Medium,
            title: "Host key signed with SHA-1".to_string(),
            detail: "The server authenticated with the legacy ssh-rsa algorithm, \
                     which signs with SHA-1. The same RSA key can be served as \
                     rsa-sha2-256 on any OpenSSH from 7.2 on."
                .to_string(),
            location: "host key".to_string(),
            instruction: Some(
                "Remove ssh-rsa from HostKeyAlgorithms in sshd_config (modern \
                 OpenSSH disables it by default)."
                    .to_string(),
            ),
        });
    }

    if !crypto.strict_kex {
        findings.push(NewFinding {
            rule_id: "crypto.no-strict-kex",
            target: "strict-kex".to_string(),
            severity: Severity::Low,
            title: "No strict key exchange".to_string(),
            detail: "The server does not support strict key exchange, the \
                     mitigation for the Terrapin prefix-truncation attack \
                     (CVE-2023-48795). OpenSSH gained it in 9.6."
                .to_string(),
            location: "key exchange".to_string(),
            instruction: Some("Update the server's SSH daemon to OpenSSH 9.6 or later.".to_string()),
        });
    }

    findings
}

// ------------------------------------------------------------------- tier 1

/// The unprivileged batch: sshd posture, key file mode, and PATH hygiene,
/// separated by named markers.
///
/// `/usr/sbin` is commonly absent from a non-root PATH, hence the fallback.
pub const TIER1_COMMAND: &str = "sshd -T 2>&1 || /usr/sbin/sshd -T 2>&1; \
     echo ---PAROLA:stat---; \
     stat -c %a ~/.ssh/authorized_keys 2>/dev/null || stat -f %Lp ~/.ssh/authorized_keys 2>/dev/null; \
     echo ---PAROLA:path---; \
     for d in $(echo \"$PATH\" | tr ':' ' '); do find \"$d\" -maxdepth 0 -perm -0002 2>/dev/null; done";

/// The privileged retry, run only when the unprivileged `sshd -T` failed and a
/// sudo route exists: the daemon config again, plus the empty-password check
/// that `/etc/shadow` gates.
pub const TIER1_PRIVILEGED_COMMAND: &str = r#"sudo -S -p '' sh -c 'sshd -T 2>&1 || /usr/sbin/sshd -T 2>&1; echo ---PAROLA:shadow---; awk -F: "(\$2==\"\")" /etc/shadow | cut -d: -f1'"#;

/// Split marker-separated output into (first section, named sections). Pure.
pub fn parse_sections(stdout: &str) -> (String, BTreeMap<String, String>) {
    let mut named = BTreeMap::new();
    let mut pieces = stdout.split("---PAROLA:");

    let first = pieces.next().unwrap_or("").trim().to_string();
    for piece in pieces {
        if let Some((name, body)) = piece.split_once("---") {
            named.insert(name.trim().to_string(), body.trim().to_string());
        }
    }
    (first, named)
}

/// Whether `sshd -T` output looks like the real thing rather than an error.
fn sshd_config_is_usable(text: &str) -> bool {
    text.to_lowercase().contains("permitrootlogin")
}

/// Evaluate `sshd -T` output. Pure. Keys arrive lowercased from sshd itself.
fn sshd_findings(config: &str) -> Vec<NewFinding> {
    let value_of = |key: &str| -> Option<String> {
        config.lines().find_map(|line| {
            let (k, v) = line.trim().split_once(' ')?;
            (k.eq_ignore_ascii_case(key)).then(|| v.trim().to_string())
        })
    };
    let mut findings = Vec::new();

    if value_of("permitrootlogin").as_deref() == Some("yes") {
        findings.push(NewFinding {
            rule_id: "sshd.permit-root-login",
            target: "sshd".to_string(),
            severity: Severity::High,
            title: "Root may log in over SSH".to_string(),
            detail: "PermitRootLogin is `yes`, so the one account every attacker \
                     knows the name of can be brute-forced directly."
                .to_string(),
            location: "sshd -T".to_string(),
            instruction: Some(
                "Set `PermitRootLogin prohibit-password` (or `no`) in sshd_config."
                    .to_string(),
            ),
        });
    }

    if value_of("passwordauthentication").as_deref() == Some("yes") {
        findings.push(NewFinding {
            rule_id: "sshd.password-auth",
            target: "sshd".to_string(),
            severity: Severity::Medium,
            title: "Password authentication is enabled".to_string(),
            detail: "Passwords can be guessed at network speed; keys cannot. \
                     Every internet-facing sshd log is full of the attempts."
                .to_string(),
            location: "sshd -T".to_string(),
            instruction: Some(
                "Set `PasswordAuthentication no` once every account has a key in place."
                    .to_string(),
            ),
        });
    }

    if value_of("permitemptypasswords").as_deref() == Some("yes") {
        findings.push(NewFinding {
            rule_id: "sshd.empty-passwords",
            target: "sshd".to_string(),
            severity: Severity::Critical,
            title: "Empty passwords are permitted".to_string(),
            detail: "PermitEmptyPasswords is `yes`: any account with a blank \
                     password can be entered by anyone who finds the port."
                .to_string(),
            location: "sshd -T".to_string(),
            instruction: Some("Set `PermitEmptyPasswords no` in sshd_config.".to_string()),
        });
    }

    if value_of("x11forwarding").as_deref() == Some("yes") {
        findings.push(NewFinding {
            rule_id: "sshd.x11-forwarding",
            target: "sshd".to_string(),
            severity: Severity::Low,
            title: "X11 forwarding is enabled".to_string(),
            detail: "X11Forwarding exposes the client's display server to the \
                     remote host. Rarely needed on a server; attack surface always."
                .to_string(),
            location: "sshd -T".to_string(),
            instruction: Some("Set `X11Forwarding no` unless it is genuinely used.".to_string()),
        });
    }

    if let Some(tries) = value_of("maxauthtries").and_then(|v| v.parse::<u32>().ok()) {
        if tries > 6 {
            findings.push(NewFinding {
                rule_id: "sshd.max-auth-tries",
                target: "sshd".to_string(),
                severity: Severity::Low,
                title: format!("MaxAuthTries is {tries}"),
                detail: "More attempts per connection than the default 6 makes \
                         online password guessing that much cheaper."
                    .to_string(),
                location: "sshd -T".to_string(),
                instruction: Some("Set `MaxAuthTries 3` in sshd_config.".to_string()),
            });
        }
    }

    findings
}

/// Evaluate the `stat` mode of authorized_keys. Pure. Empty means the file
/// does not exist, which is not a finding.
fn authorized_keys_findings(mode_text: &str) -> Vec<NewFinding> {
    let mode = mode_text.trim();
    let Ok(bits) = u32::from_str_radix(mode, 8) else {
        return Vec::new();
    };

    if bits & 0o022 != 0 {
        return vec![NewFinding {
            rule_id: "files.authorized-keys-writable",
            target: "authorized_keys".to_string(),
            severity: Severity::High,
            title: format!("authorized_keys is mode {mode}"),
            detail: "Anyone who can write this file can add their own key and \
                     log in as this account."
                .to_string(),
            location: "~/.ssh/authorized_keys".to_string(),
            instruction: Some("chmod 600 ~/.ssh/authorized_keys".to_string()),
        }];
    }
    Vec::new()
}

/// Each non-empty line is a world-writable directory on PATH. Pure.
fn path_findings(text: &str) -> Vec<NewFinding> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|dir| NewFinding {
            rule_id: "files.world-writable-path",
            target: dir.to_string(),
            severity: Severity::High,
            title: format!("{dir} on PATH is world-writable"),
            detail: "Any local user can drop a binary there and wait for this \
                     account to run it."
                .to_string(),
            location: dir.to_string(),
            instruction: Some(format!("chmod o-w {dir}")),
        })
        .collect()
}

/// Each non-empty line is an account with an empty password field. Pure.
fn shadow_findings(text: &str) -> Vec<NewFinding> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|account| NewFinding {
            rule_id: "accounts.empty-password",
            target: account.to_string(),
            severity: Severity::Critical,
            title: format!("Account `{account}` has no password"),
            detail: "An empty password field in /etc/shadow means this account \
                     authenticates with nothing at all."
                .to_string(),
            location: "/etc/shadow".to_string(),
            instruction: Some(format!("passwd -l {account} (or set a real password)")),
        })
        .collect()
}

// ----------------------------------------------------------------- assembly

/// Everything the command layer gathered, handed to the pure assembler.
pub struct GatheredTier1 {
    pub sshd_config: Option<String>,
    pub authorized_keys_mode: String,
    pub writable_path_dirs: String,
    pub shadow: Option<String>,
    /// Why sshd posture (and the shadow check) could not run, when they
    /// could not.
    pub note: Option<String>,
}

/// Build the report. Pure — everything remote has already happened.
pub fn assemble(
    host_id: &str,
    crypto: Option<&NegotiatedCrypto>,
    tier1: Option<&GatheredTier1>,
    suppressed_keys: &HashSet<String>,
) -> RemoteAuditReport {
    let mut new_findings = Vec::new();

    if let Some(crypto) = crypto {
        new_findings.extend(tier0_findings(crypto));
    }

    let (tier1_ran, tier1_note) = match tier1 {
        Some(gathered) => {
            if let Some(config) = gathered.sshd_config.as_deref() {
                new_findings.extend(sshd_findings(config));
            }
            new_findings.extend(authorized_keys_findings(&gathered.authorized_keys_mode));
            new_findings.extend(path_findings(&gathered.writable_path_dirs));
            if let Some(shadow) = gathered.shadow.as_deref() {
                new_findings.extend(shadow_findings(shadow));
            }
            (true, gathered.note.clone())
        }
        None => (false, None),
    };

    let findings: Vec<RemoteFinding> = new_findings
        .into_iter()
        .map(|new| {
            let id = format!("{}|{}", new.rule_id, new.target);
            let suppressed = suppressed_keys.contains(&format!("{host_id}|{id}"));
            RemoteFinding {
                id,
                rule_id: new.rule_id.to_string(),
                severity: new.severity,
                title: new.title,
                detail: new.detail,
                location: new.location,
                instruction: new.instruction,
                suppressed,
            }
        })
        .collect();

    let counts = count(&findings);
    let score = score(&findings);

    RemoteAuditReport {
        host_id: host_id.to_string(),
        findings,
        counts,
        score,
        tier1_ran,
        tier1_note,
        checked_at_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0),
    }
}

/// Interpret the two tier-1 batches into one gathered result. Pure.
///
/// `privileged` is the output of the sudo retry when it ran; its sshd config
/// wins if the unprivileged one was unusable.
pub fn gather_tier1(
    unprivileged: &CommandOutput,
    privileged: Option<&CommandOutput>,
    elevation_usable: bool,
) -> GatheredTier1 {
    let (first, sections) = parse_sections(&unprivileged.stdout);

    let mut sshd_config = sshd_config_is_usable(&first).then_some(first);
    let mut shadow = None;
    let mut note = None;

    if let Some(privileged) = privileged {
        let (priv_first, priv_sections) = parse_sections(&privileged.stdout);
        if sshd_config.is_none() && sshd_config_is_usable(&priv_first) {
            sshd_config = Some(priv_first);
        }
        shadow = priv_sections.get("shadow").cloned();
    }

    if sshd_config.is_none() {
        note = Some(if elevation_usable {
            "sshd -T did not answer even with sudo, so daemon posture could not \
             be checked."
                .to_string()
        } else {
            "sshd -T and the empty-password check need root. Connect as root or \
             configure sudo to include them."
                .to_string()
        });
    } else if shadow.is_none() && !elevation_usable {
        note = Some(
            "The empty-password check reads /etc/shadow, which needs root; it was \
             skipped."
                .to_string(),
        );
    }

    GatheredTier1 {
        sshd_config,
        authorized_keys_mode: sections.get("stat").cloned().unwrap_or_default(),
        writable_path_dirs: sections.get("path").cloned().unwrap_or_default(),
        shadow,
        note,
    }
}

/// Whether the unprivileged batch's sshd section warrants the sudo retry.
pub fn needs_privileged_retry(unprivileged: &CommandOutput) -> bool {
    let (first, _) = parse_sections(&unprivileged.stdout);
    !sshd_config_is_usable(&first)
}

// ------------------------------------------------------------------ scoring
// Same shape as ssh::audit: one rule costs its weight once, half again for
// repeats, nothing beyond.

fn severity_weight(severity: Severity) -> u32 {
    match severity {
        Severity::Critical => 25,
        Severity::High => 12,
        Severity::Medium => 6,
        Severity::Low => 2,
        Severity::Info => 0,
    }
}

fn count(findings: &[RemoteFinding]) -> SeverityCounts {
    let mut counts = SeverityCounts::default();
    for finding in findings.iter().filter(|f| !f.suppressed) {
        match finding.severity {
            Severity::Critical => counts.critical += 1,
            Severity::High => counts.high += 1,
            Severity::Medium => counts.medium += 1,
            Severity::Low => counts.low += 1,
            Severity::Info => counts.info += 1,
        }
    }
    counts
}

fn score(findings: &[RemoteFinding]) -> u32 {
    let mut per_rule: BTreeMap<&str, (Severity, u32)> = BTreeMap::new();
    for finding in findings.iter().filter(|finding| !finding.suppressed) {
        let entry = per_rule
            .entry(finding.rule_id.as_str())
            .or_insert((finding.severity, 0));
        entry.0 = entry.0.max(finding.severity);
        entry.1 += 1;
    }

    let penalty: u32 = per_rule
        .values()
        .map(|(severity, count)| {
            let weight = severity_weight(*severity);
            if *count <= 1 {
                weight
            } else {
                weight * 3 / 2
            }
        })
        .sum();

    100u32.saturating_sub(penalty)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn modern_crypto() -> NegotiatedCrypto {
        NegotiatedCrypto {
            kex: "curve25519-sha256".to_string(),
            host_key_algorithm: "ssh-ed25519".to_string(),
            cipher: "chacha20-poly1305@openssh.com".to_string(),
            client_mac: "hmac-sha2-256".to_string(),
            server_mac: "hmac-sha2-256".to_string(),
            strict_kex: true,
        }
    }

    fn output(stdout: &str) -> CommandOutput {
        CommandOutput {
            stdout: stdout.to_string(),
            stderr: String::new(),
            exit_code: Some(0),
        }
    }

    #[test]
    fn a_modern_handshake_produces_no_tier0_findings() {
        assert!(tier0_findings(&modern_crypto()).is_empty());
    }

    #[test]
    fn tier0_flags_each_weak_algorithm() {
        let mut crypto = modern_crypto();
        crypto.kex = "diffie-hellman-group14-sha1".to_string();
        crypto.cipher = "aes256-cbc".to_string();
        crypto.host_key_algorithm = "ssh-rsa".to_string();
        crypto.strict_kex = false;

        let findings = tier0_findings(&crypto);
        let rules: Vec<&str> = findings.iter().map(|f| f.rule_id).collect();
        assert_eq!(
            rules,
            vec![
                "crypto.kex-sha1",
                "crypto.cipher-cbc",
                "crypto.hostkey-sha1",
                "crypto.no-strict-kex"
            ]
        );
        assert_eq!(findings[0].severity, Severity::High);
        assert_eq!(findings[2].severity, Severity::Medium);
        assert_eq!(findings[3].severity, Severity::Low);
    }

    #[test]
    fn sshd_posture_rules_fire_on_the_risky_values() {
        let config = "\
permitrootlogin yes\n\
passwordauthentication yes\n\
permitemptypasswords yes\n\
x11forwarding yes\n\
maxauthtries 10\n";
        let findings = sshd_findings(config);
        let rules: Vec<&str> = findings.iter().map(|f| f.rule_id).collect();
        assert_eq!(
            rules,
            vec![
                "sshd.permit-root-login",
                "sshd.password-auth",
                "sshd.empty-passwords",
                "sshd.x11-forwarding",
                "sshd.max-auth-tries"
            ]
        );

        // The safe values are all quiet.
        let quiet = sshd_findings(
            "permitrootlogin prohibit-password\npasswordauthentication no\n\
             permitemptypasswords no\nx11forwarding no\nmaxauthtries 3\n",
        );
        assert!(quiet.is_empty());
    }

    #[test]
    fn authorized_keys_mode_is_judged_on_group_and_world_write() {
        assert!(authorized_keys_findings("600").is_empty());
        assert!(authorized_keys_findings("644").is_empty()); // readable is fine
        assert_eq!(authorized_keys_findings("664").len(), 1);
        assert_eq!(authorized_keys_findings("666").len(), 1);
        // Missing file (empty stat section) is not a finding.
        assert!(authorized_keys_findings("").is_empty());
        assert!(authorized_keys_findings("garbage").is_empty());
    }

    #[test]
    fn each_writable_path_dir_and_empty_password_account_is_a_finding() {
        let paths = path_findings("/usr/local/bin\n/opt/tools\n");
        assert_eq!(paths.len(), 2);
        assert!(paths.iter().all(|f| f.severity == Severity::High));

        let accounts = shadow_findings("guest\nkiosk\n");
        assert_eq!(accounts.len(), 2);
        assert!(accounts.iter().all(|f| f.severity == Severity::Critical));
        assert!(accounts[0].title.contains("guest"));
    }

    #[test]
    fn sections_are_split_on_their_markers() {
        let stdout = "permitrootlogin no\nport 22\n\
                      ---PAROLA:stat---\n600\n\
                      ---PAROLA:path---\n\n";
        let (first, sections) = parse_sections(stdout);
        assert!(first.contains("permitrootlogin"));
        assert_eq!(sections.get("stat").map(String::as_str), Some("600"));
        assert_eq!(sections.get("path").map(String::as_str), Some(""));
    }

    #[test]
    fn a_permission_denied_sshd_run_asks_for_the_privileged_retry() {
        let denied = output(
            "sshd re-exec requires execution with an absolute path\n\
             Permission denied\n---PAROLA:stat---\n600\n---PAROLA:path---\n",
        );
        assert!(needs_privileged_retry(&denied));

        let fine = output("permitrootlogin no\n---PAROLA:stat---\n600\n");
        assert!(!needs_privileged_retry(&fine));
    }

    #[test]
    fn gather_prefers_the_privileged_config_and_notes_what_was_skipped() {
        let unprivileged = output(
            "Permission denied\n---PAROLA:stat---\n600\n---PAROLA:path---\n",
        );

        // With a sudo retry, the config and the shadow check both arrive.
        let privileged = output("permitrootlogin yes\n---PAROLA:shadow---\nguest\n");
        let gathered = gather_tier1(&unprivileged, Some(&privileged), true);
        assert!(gathered.sshd_config.unwrap().contains("permitrootlogin"));
        assert_eq!(gathered.shadow.as_deref(), Some("guest"));
        assert!(gathered.note.is_none());

        // Without elevation, the degradation is named, not glossed over.
        let gathered = gather_tier1(&unprivileged, None, false);
        assert!(gathered.sshd_config.is_none());
        assert!(gathered.note.unwrap().contains("need root"));
    }

    #[test]
    fn suppression_is_keyed_per_host() {
        let mut crypto = modern_crypto();
        crypto.host_key_algorithm = "ssh-rsa".to_string();

        let mut suppressed = HashSet::new();
        suppressed.insert("host-a|crypto.hostkey-sha1|ssh-rsa".to_string());

        // Host A dismissed it; host B still sees it in full.
        let report_a = assemble("host-a", Some(&crypto), None, &suppressed);
        assert!(report_a.findings[0].suppressed);
        assert_eq!(report_a.score, 100);

        let report_b = assemble("host-b", Some(&crypto), None, &suppressed);
        assert!(!report_b.findings[0].suppressed);
        assert_eq!(report_b.score, 94);
    }

    #[test]
    fn scoring_matches_the_local_audit_shape() {
        let tier1 = GatheredTier1 {
            sshd_config: Some("permitrootlogin yes\n".to_string()),
            authorized_keys_mode: "600".to_string(),
            writable_path_dirs: "/opt/a\n/opt/b\n".to_string(),
            shadow: None,
            note: None,
        };
        let report = assemble("h", Some(&modern_crypto()), Some(&tier1), &HashSet::new());

        // permit-root-login (12) + world-writable-path twice (12 * 3/2 = 18).
        assert_eq!(report.score, 100 - 12 - 18);
        assert_eq!(report.counts.high, 3);
        assert!(report.tier1_ran);
    }

    #[test]
    fn windows_reports_tier0_only_with_tier1_not_run() {
        let report = assemble("h", Some(&modern_crypto()), None, &HashSet::new());
        assert!(!report.tier1_ran);
        assert!(report.findings.is_empty());
        assert_eq!(report.score, 100);
    }
}
