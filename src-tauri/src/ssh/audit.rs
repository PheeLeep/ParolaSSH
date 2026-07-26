//! The security audit: a catalog of rules run over a scanned SSH directory.
//!
//! Rules are deliberately explicit rather than clever. Each one owns its
//! severity, the sentence explaining why it matters, and either a fix we can
//! apply or the exact command the user should run themselves.
//!
//! `audit` takes everything it needs as arguments so it can be run against
//! fixture directories in tests on any platform.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use super::config::{SshConfig, MACOS_ONLY_KEYWORDS};
use super::keys::{KdfInfo, KeyFormat, KeyScan, PublicKeyPairing, SshKey};
use super::known_hosts::KnownHosts;
use super::paths::{symlink_target, SshPaths};
use super::perms::{self, KeyPermissions};
use super::SshResult;

/// RSA below this is considered broken rather than merely dated.
const RSA_BROKEN_BITS: u32 = 2048;
/// Current guidance for new RSA keys.
const RSA_WEAK_BITS: u32 = 3072;
/// `ssh-keygen` has defaulted to 16 bcrypt rounds since OpenSSH 9.4.
const MIN_KDF_ROUNDS: u32 = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Low,
    Medium,
    High,
    Critical,
}

impl Severity {
    /// How much this knocks off the overall score.
    fn weight(self) -> u32 {
        match self {
            Self::Critical => 25,
            Self::High => 12,
            Self::Medium => 6,
            Self::Low => 2,
            Self::Info => 0,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Remediation {
    /// We can fix this ourselves, on request.
    #[serde(rename_all = "camelCase")]
    RestrictPermissions { path: String, is_dir: bool },
    /// The user has to decide — we show the command, we do not run it.
    #[serde(rename_all = "camelCase")]
    Manual { instruction: String },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    /// Stable across runs: `rule|target`. Suppressions key off this.
    pub id: String,
    pub rule_id: String,
    pub severity: Severity,
    pub title: String,
    /// One sentence on why it matters — this is what makes a report useful.
    pub detail: String,
    /// Human-readable location, e.g. `id_rsa` or `config:12`.
    pub location: String,
    pub path: Option<String>,
    pub remediation: Option<Remediation>,
    pub suppressed: bool,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SeverityCounts {
    pub critical: usize,
    pub high: usize,
    pub medium: usize,
    pub low: usize,
    pub info: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditReport {
    pub ssh_dir: String,
    pub dir_exists: bool,
    /// Set when `~/.ssh` is a symlink — fixes would touch the target.
    pub symlink_target: Option<String>,
    pub directory_permissions: KeyPermissions,
    pub findings: Vec<Finding>,
    pub counts: SeverityCounts,
    /// 0–100. Suppressed findings do not count against it.
    pub score: u32,
    pub key_count: usize,
    pub scanned_at_ms: i64,
}

/// A finding before its id and suppression state are resolved. A struct because
/// the fields are all strings and options, easy to transpose positionally.
struct NewFinding<'a> {
    rule_id: &'a str,
    /// The stable thing this is about — a fingerprint, or a path. Combined
    /// with `rule_id` to form the id that suppressions key off.
    target: &'a str,
    severity: Severity,
    title: String,
    detail: String,
    location: String,
    path: Option<String>,
    remediation: Option<Remediation>,
}

struct RuleContext<'a> {
    suppressed: &'a HashSet<String>,
    findings: Vec<Finding>,
}

impl RuleContext<'_> {
    fn add(&mut self, new: NewFinding) {
        let id = format!("{}|{}", new.rule_id, new.target);
        let suppressed = self.suppressed.contains(&id);

        self.findings.push(Finding {
            id,
            rule_id: new.rule_id.to_string(),
            severity: new.severity,
            title: new.title,
            detail: new.detail,
            location: new.location,
            path: new.path,
            remediation: new.remediation,
            suppressed,
        });
    }

    /// Add a finding about a specific key. Target, location and path derive
    /// from the key, so call sites supply only what varies.
    fn add_key(
        &mut self,
        key: &SshKey,
        rule_id: &str,
        severity: Severity,
        title: impl Into<String>,
        detail: impl Into<String>,
        remediation: Option<Remediation>,
    ) {
        self.add(NewFinding {
            rule_id,
            target: &key.id,
            severity,
            title: title.into(),
            detail: detail.into(),
            location: key.file_name.clone(),
            path: Some(key.path.clone()),
            remediation,
        });
    }
}

/// Run the full audit against an already-scanned directory.
pub fn audit(
    ssh_dir: &Path,
    scan: &KeyScan,
    config: &SshConfig,
    known_hosts: &KnownHosts,
    suppressed: &HashSet<String>,
) -> AuditReport {
    let mut context = RuleContext {
        suppressed,
        findings: Vec::new(),
    };

    let dir_exists = ssh_dir.is_dir();
    let directory_permissions = if dir_exists {
        perms::read_permissions(ssh_dir)
    } else {
        KeyPermissions::Unknown {
            reason: "Directory does not exist".to_string(),
        }
    };

    if dir_exists {
        check_directory(&mut context, ssh_dir, &directory_permissions);
        check_special_files(&mut context, ssh_dir);
        check_keys(&mut context, scan);
        check_config(&mut context, ssh_dir, config);
        check_known_hosts(&mut context, ssh_dir, known_hosts);
    }

    // Most severe first; ties broken by rule so the order is stable between
    // runs rather than depending on directory iteration order.
    context.findings.sort_by(|a, b| {
        b.severity
            .cmp(&a.severity)
            .then_with(|| a.rule_id.cmp(&b.rule_id))
            .then_with(|| a.location.cmp(&b.location))
    });

    let counts = count(&context.findings);
    let score = score(&context.findings);

    AuditReport {
        ssh_dir: ssh_dir.to_string_lossy().to_string(),
        dir_exists,
        symlink_target: symlink_target(ssh_dir).map(|p| p.to_string_lossy().to_string()),
        directory_permissions,
        findings: context.findings,
        counts,
        score,
        key_count: scan.keys.len(),
        scanned_at_ms: now_millis(),
    }
}

/// Scan and audit `ssh_dir` in one call.
pub fn run(ssh_dir: &Path, suppressed: &HashSet<String>) -> SshResult<AuditReport> {
    let paths = SshPaths::new(ssh_dir);
    let config = SshConfig::read(&paths.config());
    let known_hosts = KnownHosts::read(&paths.known_hosts());
    let scan = super::keys::scan(ssh_dir, &config.identity_files())?;

    Ok(audit(ssh_dir, &scan, &config, &known_hosts, suppressed))
}

// ---------------------------------------------------------------- directory

fn check_directory(context: &mut RuleContext, ssh_dir: &Path, permissions: &KeyPermissions) {
    let dir_path = ssh_dir.to_string_lossy().to_string();

    let too_open = match permissions {
        KeyPermissions::Posix { mode, .. } => *mode & 0o077 != 0,
        other => other.is_exposed().unwrap_or(false),
    };

    if too_open {
        context.add(NewFinding {
            rule_id: "perm.dir",
            target: &dir_path,
            severity: Severity::High,
            title: "SSH directory is accessible to other users".into(),
            detail: format!(
                "Anyone with an account on this machine can list your keys ({}). \
                 ssh also refuses to use some files from a world-accessible directory.",
                permissions.describe()
            ),
            location: dir_path.clone(),
            path: Some(dir_path.clone()),
            remediation: Some(Remediation::RestrictPermissions {
                path: dir_path.clone(),
                is_dir: true,
            }),
        });
    }

    if let Some(target) = symlink_target(ssh_dir) {
        context.add(NewFinding {
            rule_id: "dir.symlink",
            target: &dir_path,
            severity: Severity::Info,
            title: "SSH directory is a symlink".into(),
            detail: format!(
                "It points at {}. Permissions are audited on the target, and any fix applied \
                 here changes that file — check whether it is tracked in a dotfiles repository.",
                target.display()
            ),
            location: dir_path.clone(),
            path: Some(dir_path.clone()),
            remediation: None,
        });
    }
}

fn check_special_files(context: &mut RuleContext, ssh_dir: &Path) {
    let paths = SshPaths::new(ssh_dir);

    for (path, rule_id, title, detail) in [
        (
            paths.authorized_keys(),
            "perm.authorized-keys",
            "authorized_keys is writable by other users",
            "Another user could append their own public key and log in as you. sshd refuses to \
             read this file while it is group- or world-writable, so key logins may already be \
             failing.",
        ),
        (
            paths.config(),
            "perm.config-writable",
            "SSH config is writable by other users",
            "Another user could redirect your connections by editing this file. ssh refuses to \
             start when its config is writable by anyone but the owner.",
        ),
    ] {
        if !path.is_file() {
            continue;
        }

        let permissions = perms::read_permissions(&path);
        if permissions.is_group_or_world_writable() != Some(true) {
            continue;
        }

        let path_string = path.to_string_lossy().to_string();
        context.add(NewFinding {
            rule_id,
            target: &path_string,
            severity: Severity::Critical,
            title: title.into(),
            detail: format!("{detail} Current permissions: {}.", permissions.describe()),
            location: file_label(ssh_dir, &path),
            path: Some(path_string.clone()),
            remediation: Some(Remediation::RestrictPermissions {
                path: path_string.clone(),
                is_dir: false,
            }),
        });
    }
}

// --------------------------------------------------------------------- keys

fn check_keys(context: &mut RuleContext, scan: &KeyScan) {
    for key in &scan.keys {
        check_key_permissions(context, key);
        check_key_algorithm(context, key);
        check_key_encryption(context, key);
        check_key_hygiene(context, key);
    }

    check_duplicates(context, scan);

    for orphan in &scan.orphan_public_keys {
        context.add(NewFinding {
            rule_id: "key.orphan-public",
            target: &orphan.path,
            severity: Severity::Info,
            title: "Public key with no matching private key".into(),
            detail: "The private half is missing or stored elsewhere. Harmless on its own, but \
                     it usually means a key was moved or partly deleted."
                .into(),
            location: orphan.file_name.clone(),
            path: Some(orphan.path.clone()),
            remediation: None,
        });
    }
}

fn check_key_permissions(context: &mut RuleContext, key: &SshKey) {
    match key.permissions.is_exposed() {
        Some(true) => context.add_key(
            key,
            "perm.key.exposed",
            Severity::Critical,
            format!("Private key {} is readable by other users", key.file_name),
            format!(
                "Anyone with an account on this machine can copy this key and use it. \
                 Current permissions: {}. ssh will also refuse to use it.",
                key.permissions.describe()
            ),
            Some(Remediation::RestrictPermissions {
                path: key.path.clone(),
                is_dir: false,
            }),
        ),

        // Never report "unknown" as safe — say so, and let the user decide.
        None => context.add_key(
            key,
            "perm.key.unknown",
            Severity::Low,
            format!("Could not read permissions for {}", key.file_name),
            format!(
                "The audit cannot confirm this key is protected: {}.",
                key.permissions.describe()
            ),
            None,
        ),

        Some(false) => {}
    }
}

fn check_key_algorithm(context: &mut RuleContext, key: &SshKey) {
    let replace_with_ed25519 = || {
        Some(Remediation::Manual {
            instruction: "ssh-keygen -t ed25519 -C \"your comment\"".to_string(),
        })
    };

    match key.algorithm_id.as_str() {
        "dsa" => context.add_key(
            key,
            "algo.dsa",
            Severity::Critical,
            format!("{} uses DSA", key.file_name),
            "DSA keys are fixed at 1024 bits and were removed from OpenSSH entirely in 9.8. \
             Replace this key with Ed25519.",
            replace_with_ed25519(),
        ),

        "rsa" => match key.bits {
            Some(bits) if bits < RSA_BROKEN_BITS => context.add_key(
                key,
                "algo.rsa-broken",
                Severity::Critical,
                format!("{} is a {}-bit RSA key", key.file_name, bits),
                format!(
                    "{bits}-bit RSA is below the minimum every current SSH implementation \
                     accepts and is considered breakable. Replace it."
                ),
                replace_with_ed25519(),
            ),
            Some(bits) if bits < RSA_WEAK_BITS => context.add_key(
                key,
                "algo.rsa-weak",
                Severity::Medium,
                format!("{} is a {}-bit RSA key", key.file_name, bits),
                format!(
                    "{bits}-bit RSA still works but is below current guidance of {RSA_WEAK_BITS} \
                     bits. Ed25519 is smaller, faster and stronger."
                ),
                replace_with_ed25519(),
            ),
            _ => {}
        },

        "ecdsa" => context.add_key(
            key,
            "algo.ecdsa-nist",
            Severity::Info,
            format!("{} uses ECDSA on a NIST curve", key.file_name),
            "ECDSA is not broken, but its NIST curves have a less transparent provenance than \
             Ed25519 and the signature scheme is unforgiving of bad randomness. Prefer Ed25519 \
             for new keys.",
            None,
        ),

        _ => {}
    }
}

fn check_key_encryption(context: &mut RuleContext, key: &SshKey) {
    match &key.kdf {
        KdfInfo::LegacyPemMd5 { cipher } => context.add_key(
            key,
            "fmt.legacy-pem-md5",
            Severity::High,
            format!("{} uses the old PEM encryption scheme", key.file_name),
            format!(
                "This key is encrypted with {cipher} using a single MD5 pass over your \
                 passphrase — a GPU can try billions of guesses per second against it. \
                 Re-encrypt it in the OpenSSH format, which uses bcrypt."
            ),
            Some(Remediation::Manual {
                instruction: format!("ssh-keygen -p -o -f \"{}\"", key.path),
            }),
        ),

        KdfInfo::Bcrypt { rounds } if *rounds < MIN_KDF_ROUNDS => context.add_key(
            key,
            "kdf.low-rounds",
            Severity::Low,
            format!("{} uses {} KDF rounds", key.file_name, rounds),
            format!(
                "Current ssh-keygen uses {MIN_KDF_ROUNDS} rounds. More rounds make an offline \
                 attack on the passphrase proportionally slower."
            ),
            Some(Remediation::Manual {
                instruction: format!("ssh-keygen -p -a {MIN_KDF_ROUNDS} -f \"{}\"", key.path),
            }),
        ),

        KdfInfo::None if !key.encrypted => context.add_key(
            key,
            "key.unencrypted",
            Severity::Medium,
            format!("{} has no passphrase", key.file_name),
            "Anyone who gets a copy of this file — a backup, a stolen laptop, a synced folder — \
             can use it immediately. This is a deliberate trade-off for agent and CI workflows; \
             dismiss the finding if that is the case here.",
            Some(Remediation::Manual {
                instruction: format!("ssh-keygen -p -f \"{}\"", key.path),
            }),
        ),

        _ => {}
    }

    if key.format == KeyFormat::LegacyPem && !matches!(key.kdf, KdfInfo::LegacyPemMd5 { .. }) {
        context.add_key(
            key,
            "fmt.legacy-pem",
            Severity::Medium,
            format!("{} is stored in the legacy PEM format", key.file_name),
            "The pre-2014 PEM format cannot hold a comment and has no modern passphrase \
             protection. Convert it to the OpenSSH format.",
            Some(Remediation::Manual {
                instruction: format!("ssh-keygen -p -o -f \"{}\"", key.path),
            }),
        );
    }
}

fn check_key_hygiene(context: &mut RuleContext, key: &SshKey) {
    if key.is_stray_copy() {
        context.add_key(
            key,
            "file.stray-secret",
            Severity::Medium,
            format!("{} looks like a leftover copy of a key", key.file_name),
            "Backup copies are easy to forget about and just as usable as the original. If you \
             no longer need it, delete it; if you do, give it a real name.",
            None,
        );
    }

    if let Some(error) = &key.parse_error {
        context.add_key(
            key,
            "key.parse-error",
            Severity::Low,
            format!("Could not parse {}", key.file_name),
            format!("The file looks like a private key but could not be read: {error}."),
            None,
        );
    }

    check_key_pairing(context, key);
}

/// Does the `.pub` sidecar actually belong to this key?
fn check_key_pairing(context: &mut RuleContext, key: &SshKey) {
    match &key.pairing {
        PublicKeyPairing::Mismatched {
            private_fingerprint,
            public_fingerprint,
        } => context.add_key(
            key,
            "key.pub-mismatch",
            Severity::High,
            format!("{}.pub belongs to a different key", key.file_name),
            format!(
                "The sidecar is the file you copy onto servers and into GitHub, so anywhere you \
                 installed it will reject this key with no useful error. The private key is \
                 {private_fingerprint}, but {}.pub is {public_fingerprint}. Regenerate the \
                 sidecar from the private key.",
                key.file_name
            ),
            Some(Remediation::Manual {
                instruction: format!("ssh-keygen -y -f \"{}\" > \"{}.pub\"", key.path, key.path),
            }),
        ),

        PublicKeyPairing::Unreadable { reason } => context.add_key(
            key,
            "key.pub-unreadable",
            Severity::Low,
            format!("{}.pub could not be read", key.file_name),
            format!("{reason} Anything installed from it may not match this key."),
            Some(Remediation::Manual {
                instruction: format!("ssh-keygen -y -f \"{}\" > \"{}.pub\"", key.path, key.path),
            }),
        ),

        PublicKeyPairing::Missing if key.parse_error.is_none() => context.add_key(
            key,
            "key.no-public",
            Severity::Info,
            format!("{} has no .pub file", key.file_name),
            "Not a security problem — the public key can always be derived — but some tools \
             expect the file to exist.",
            Some(Remediation::Manual {
                instruction: format!("ssh-keygen -y -f \"{}\" > \"{}.pub\"", key.path, key.path),
            }),
        ),

        // Matched, Unverifiable, or missing on a key we could not parse.
        _ => {}
    }
}

fn check_duplicates(context: &mut RuleContext, scan: &KeyScan) {
    let mut by_fingerprint: BTreeMap<&str, Vec<&SshKey>> = BTreeMap::new();
    for key in &scan.keys {
        if let Some(fingerprint) = &key.fingerprint {
            by_fingerprint.entry(fingerprint).or_default().push(key);
        }
    }

    for (fingerprint, keys) in by_fingerprint {
        if keys.len() < 2 {
            continue;
        }

        let names: Vec<&str> = keys.iter().map(|key| key.file_name.as_str()).collect();
        context.add(NewFinding {
            rule_id: "key.duplicate",
            // Keyed on the fingerprint, not a path, so the finding survives one
            // of the copies being renamed.
            target: fingerprint,
            severity: Severity::Info,
            title: "The same key exists in more than one file".into(),
            detail: format!(
                "{} are the same key. Revoking it means removing every copy.",
                names.join(", ")
            ),
            location: names.join(", "),
            path: keys.first().map(|key| key.path.clone()),
            remediation: None,
        });
    }
}

// ------------------------------------------------------------------- config

fn check_config(context: &mut RuleContext, ssh_dir: &Path, config: &SshConfig) {
    if !config.exists {
        return;
    }

    let config_path = SshPaths::new(ssh_dir).config();
    let path_string = config_path.to_string_lossy().to_string();

    for (block, directive) in config.all_directives() {
        let keyword = directive.keyword_lower();
        let where_ = format!("config:{}", directive.line);
        let target = format!("{path_string}:{}", directive.line);
        let scope = block.label();

        match keyword.as_str() {
            "stricthostkeychecking" if directive.is_no() => context.add(NewFinding {
                rule_id: "cfg.strict-host-key-checking-off",
                target: &target,
                severity: Severity::High,
                title: "Host key checking is disabled".into(),
                detail: format!(
                    "For `{scope}`, ssh will accept any host key without asking. That removes \
                     the only protection against a machine-in-the-middle impersonating the server."
                ),
                location: where_,
                path: Some(path_string.clone()),
                remediation: Some(Remediation::Manual {
                    instruction: "Remove the directive, or set it to `ask`.".to_string(),
                }),
            }),

            "userknownhostsfile" if directive.value.contains("/dev/null") => {
                context.add(NewFinding {
                    rule_id: "cfg.known-hosts-devnull",
                    target: &target,
                    severity: Severity::High,
                    title: "Host keys are discarded".into(),
                    detail: format!(
                        "For `{scope}`, verified host keys are written to /dev/null, so every \
                         connection starts from zero trust and a swapped server key is never \
                         noticed."
                    ),
                    location: where_,
                    path: Some(path_string.clone()),
                    remediation: Some(Remediation::Manual {
                        instruction: "Point UserKnownHostsFile at a real file, or remove the line."
                            .to_string(),
                    }),
                })
            }

            "forwardagent" if directive.is_yes() => {
                let wildcard = block.is_wildcard();
                context.add(NewFinding {
                    rule_id: "cfg.forward-agent",
                    target: &target,
                    // Under `Host *` this reaches every server, not just one.
                    severity: if wildcard {
                        Severity::High
                    } else {
                        Severity::Medium
                    },
                    title: "Agent forwarding is enabled".into(),
                    detail: format!(
                        "Anyone with root on `{scope}` can use your local agent to authenticate \
                         as you elsewhere, for as long as you stay connected.{}",
                        if wildcard {
                            " This is set under `Host *`, so it applies to every server you \
                             connect to."
                        } else {
                            ""
                        }
                    ),
                    location: where_,
                    path: Some(path_string.clone()),
                    remediation: Some(Remediation::Manual {
                        instruction: "Prefer `ProxyJump` for reaching hosts behind a bastion."
                            .to_string(),
                    }),
                });
            }

            "identityfile" if block.is_wildcard() => context.add(NewFinding {
                rule_id: "cfg.wildcard-identity",
                target: &target,
                severity: Severity::Medium,
                title: "An identity file is offered to every host".into(),
                detail: "Under `Host *`, ssh offers this key to every server you connect to, \
                         which tells each of them your public key. Scope IdentityFile to the \
                         hosts that need it, or add `IdentitiesOnly yes`."
                    .into(),
                location: where_,
                path: Some(path_string.clone()),
                remediation: None,
            }),

            // Valid on a Mac, inert elsewhere.
            other if MACOS_ONLY_KEYWORDS.contains(&other) && !cfg!(target_os = "macos") => {
                context.add(NewFinding {
                    rule_id: "cfg.macos-only",
                    target: &target,
                    severity: Severity::Info,
                    title: format!("`{}` only works on macOS", directive.keyword),
                    detail: "This directive is ignored on this platform. Harmless, but it will \
                             not do what it does on a Mac."
                        .into(),
                    location: where_,
                    path: Some(path_string.clone()),
                    remediation: None,
                })
            }

            _ => {}
        }
    }
}

// -------------------------------------------------------------- known_hosts

fn check_known_hosts(context: &mut RuleContext, ssh_dir: &Path, known_hosts: &KnownHosts) {
    if !known_hosts.exists {
        return;
    }

    let path = SshPaths::new(ssh_dir).known_hosts();
    let path_string = path.to_string_lossy().to_string();

    let unhashed = known_hosts.unhashed();
    if !unhashed.is_empty() {
        context.add(NewFinding {
            rule_id: "kh.unhashed",
            target: &path_string,
            severity: Severity::Low,
            title: format!("{} host entries are stored in the clear", unhashed.len()),
            detail: "known_hosts is a readable list of every machine you connect to. Anyone who \
                     gets this file — or malware running as you — learns your entire \
                     infrastructure."
                .into(),
            location: "known_hosts".into(),
            path: Some(path_string.clone()),
            remediation: Some(Remediation::Manual {
                instruction: format!("ssh-keygen -H -f \"{path_string}\""),
            }),
        });
    }

    let deprecated = known_hosts.deprecated();
    if !deprecated.is_empty() {
        let lines: Vec<String> = deprecated
            .iter()
            .take(5)
            .map(|entry| entry.line.to_string())
            .collect();

        context.add(NewFinding {
            rule_id: "kh.deprecated-algo",
            target: &path_string,
            severity: Severity::Medium,
            title: format!(
                "{} host entries pin a deprecated key algorithm",
                deprecated.len()
            ),
            detail: format!(
                "Entries on line{} {} use ssh-dss or SHA-1 ssh-rsa. Those host keys will stop \
                 being accepted by current OpenSSH, and re-verifying now is safer than being \
                 prompted to trust a new key later.",
                if lines.len() == 1 { "" } else { "s" },
                lines.join(", ")
            ),
            location: "known_hosts".into(),
            path: Some(path_string.clone()),
            remediation: None,
        });
    }
}

// ------------------------------------------------------------------ scoring

fn count(findings: &[Finding]) -> SeverityCounts {
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

/// Score the directory out of 100, with diminishing returns per rule.
///
/// Repeats of a rule are one problem, not many: each costs full weight once,
/// half for the second instance, nothing beyond. Charging full weight per
/// instance would saturate the score and stop distinguishing "needs tidying"
/// from "actively compromised". `counts` still reports every finding.
fn score(findings: &[Finding]) -> u32 {
    // Collapse to one entry per rule, keeping the worst severity seen — a rule
    // like `cfg.forward-agent` varies by context.
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
            let weight = severity.weight();
            if *count <= 1 {
                weight
            } else {
                weight * 3 / 2
            }
        })
        .sum();

    100u32.saturating_sub(penalty)
}

fn file_label(ssh_dir: &Path, path: &Path) -> String {
    path.strip_prefix(ssh_dir)
        .unwrap_or(path)
        .to_string_lossy()
        .to_string()
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}
