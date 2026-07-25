//! Discovering and describing the private keys in an SSH directory.
//!
//! Nothing here returns key material. A private key is read, parsed for its
//! metadata, and the buffer is zeroized before the function returns; only the
//! public half is ever handed to the caller.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::Serialize;
use ssh_key::{public::KeyData, Algorithm, EcdsaCurve, HashAlg, Kdf, PrivateKey, PublicKey};
use zeroize::Zeroize;

use super::perms::{self, KeyPermissions};
use super::{SshError, SshResult};

/// Files in `~/.ssh` that are never private keys.
const NON_KEY_FILES: &[&str] = &[
    "config",
    "known_hosts",
    "known_hosts.old",
    "authorized_keys",
    "authorized_keys2",
    "environment",
    "rc",
    "agent.env",
];

/// Suffixes that suggest a stray copy of a secret rather than a key in use.
const STRAY_SUFFIXES: &[&str] = &[".bak", ".old", ".orig", ".save", "~", ".swp", ".tmp", ".copy"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum KeyFormat {
    /// `-----BEGIN OPENSSH PRIVATE KEY-----`, the modern format.
    OpenSsh,
    /// `-----BEGIN RSA/DSA/EC PRIVATE KEY-----`, the pre-2014 PEM format.
    LegacyPem,
    /// `-----BEGIN [ENCRYPTED] PRIVATE KEY-----`
    Pkcs8,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum KdfInfo {
    /// No passphrase — the key is usable by anyone who can read the file.
    None,
    /// The OpenSSH format's bcrypt-pbkdf.
    #[serde(rename_all = "camelCase")]
    Bcrypt { rounds: u32 },
    /// Legacy PEM encryption: a single unsalted-ish MD5 pass. Fast to attack.
    #[serde(rename_all = "camelCase")]
    LegacyPemMd5 { cipher: String },
    Unknown,
}

/// Whether the `.pub` sidecar actually belongs to the private key.
///
/// A stale `.pub` is a real failure mode rather than a tidiness issue: it is
/// the file people copy onto servers and into GitHub, so a mismatch means
/// authentication fails with no useful error — the server offers a key the
/// client cannot prove it holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PublicKeyPairing {
    /// The `.pub` file's fingerprint matches the private key.
    Matched,

    /// The `.pub` file is a *different* key.
    #[serde(rename_all = "camelCase")]
    Mismatched {
        private_fingerprint: String,
        public_fingerprint: String,
    },

    /// No `.pub` file alongside the private key.
    Missing,

    /// The `.pub` file exists but is not a valid public key.
    #[serde(rename_all = "camelCase")]
    Unreadable { reason: String },

    /// The private key could not be parsed, so there is nothing to compare.
    Unverifiable,
}

/// A private key found on disk, described without exposing its secret half.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SshKey {
    /// Fingerprint where parseable, otherwise the path — stable across scans.
    pub id: String,
    pub path: String,
    pub file_name: String,
    /// Machine-readable family: `ed25519`, `rsa`, `ecdsa`, `dsa`, `sk-ed25519`…
    pub algorithm_id: String,
    /// Human label, e.g. `"RSA 4096"`.
    pub algorithm: String,
    pub bits: Option<u32>,
    pub fingerprint: Option<String>,
    pub comment: Option<String>,
    pub encrypted: bool,
    pub kdf: KdfInfo,
    pub format: KeyFormat,
    pub public_key_path: Option<String>,
    pub public_key_openssh: Option<String>,
    /// Whether the `.pub` sidecar really belongs to this key.
    pub pairing: PublicKeyPairing,
    pub permissions: KeyPermissions,
    /// Milliseconds since the Unix epoch; the frontend renders it.
    pub modified_ms: Option<i64>,
    /// Set when the file looks like a key but could not be parsed.
    pub parse_error: Option<String>,
}

impl SshKey {
    pub fn is_stray_copy(&self) -> bool {
        let lower = self.file_name.to_lowercase();
        STRAY_SUFFIXES.iter().any(|suffix| lower.ends_with(suffix))
    }
}

/// A `.pub` file with no matching private key.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrphanPublicKey {
    pub path: String,
    pub file_name: String,
    pub fingerprint: Option<String>,
    pub comment: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct KeyScan {
    pub keys: Vec<SshKey>,
    pub orphan_public_keys: Vec<OrphanPublicKey>,
}

/// Scan `ssh_dir` for private keys, plus any extra paths (e.g. `IdentityFile`
/// entries pointing outside the directory).
pub fn scan(ssh_dir: &Path, extra_paths: &[PathBuf]) -> SshResult<KeyScan> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    let mut public_files: BTreeMap<PathBuf, ()> = BTreeMap::new();

    if ssh_dir.is_dir() {
        let entries = std::fs::read_dir(ssh_dir)
            .map_err(|error| SshError::io("Could not read the SSH directory", error))?;

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }

            let name = entry.file_name().to_string_lossy().to_string();
            // Compare case-insensitively: Windows and default macOS
            // filesystems will happily hand back `Config` or `Known_Hosts`.
            let lower = name.to_lowercase();

            if lower.ends_with(".pub") {
                public_files.insert(path, ());
                continue;
            }
            if NON_KEY_FILES.contains(&lower.as_str()) {
                continue;
            }

            candidates.push(path);
        }
    }

    for path in extra_paths {
        if path.is_file() && !candidates.contains(path) {
            candidates.push(path.clone());
        }
    }

    candidates.sort();

    let mut keys = Vec::new();
    for path in candidates {
        if let Some(key) = inspect(&path)? {
            // Its public half is accounted for; don't report it as an orphan.
            public_files.remove(&public_path_for(&path));
            keys.push(key);
        }
    }

    let orphan_public_keys = public_files
        .keys()
        .map(|path| describe_public(path))
        .collect();

    Ok(KeyScan {
        keys,
        orphan_public_keys,
    })
}

fn public_path_for(private: &Path) -> PathBuf {
    let mut name = private.file_name().unwrap_or_default().to_os_string();
    name.push(".pub");
    private.with_file_name(name)
}

/// Read a file and describe it, or return `None` if it is not a private key.
pub fn inspect(path: &Path) -> SshResult<Option<SshKey>> {
    let mut bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        // A key we cannot read is not a key we can audit; skip rather than
        // failing the whole scan.
        Err(_) => return Ok(None),
    };

    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(4096)]).to_string();
    let format = detect_format(&head);
    if format == KeyFormat::Unknown {
        bytes.zeroize();
        return Ok(None);
    }

    let permissions = perms::read_permissions(path);
    let modified_ms = modified_millis(path);
    let public_path = public_path_for(path);
    let public_key_path = public_path
        .is_file()
        .then(|| public_path.to_string_lossy().to_string());

    let mut key = SshKey {
        id: path.to_string_lossy().to_string(),
        path: path.to_string_lossy().to_string(),
        file_name: path.file_name().unwrap_or_default().to_string_lossy().to_string(),
        algorithm_id: "unknown".to_string(),
        algorithm: "Unknown".to_string(),
        bits: None,
        fingerprint: None,
        comment: None,
        encrypted: false,
        kdf: KdfInfo::Unknown,
        format,
        public_key_path,
        public_key_openssh: None,
        pairing: PublicKeyPairing::Unverifiable,
        permissions,
        modified_ms,
        parse_error: None,
    };

    match format {
        KeyFormat::OpenSsh => match PrivateKey::from_openssh(&bytes) {
            Ok(parsed) => describe_openssh(&mut key, &parsed),
            Err(error) => key.parse_error = Some(error.to_string()),
        },
        // ssh-key does not decode these, and we would not want to: the point
        // is to flag them, and the header carries everything needed for that.
        KeyFormat::LegacyPem | KeyFormat::Pkcs8 => describe_legacy(&mut key, &head),
        KeyFormat::Unknown => unreachable!("filtered above"),
    }

    // Whatever the private key itself yielded, before any `.pub` fallback —
    // comparing the sidecar against a fingerprint that *came from* the sidecar
    // would always match and prove nothing.
    let private_fingerprint = key.fingerprint.clone();

    let described_public = key
        .public_key_path
        .as_ref()
        .map(|path| describe_public(Path::new(path)));

    key.pairing = match (&private_fingerprint, &described_public) {
        (_, None) => PublicKeyPairing::Missing,
        // A legacy PEM we could not decode: nothing to compare against.
        (None, Some(_)) => PublicKeyPairing::Unverifiable,
        (Some(_), Some(public)) => match &public.fingerprint {
            None => PublicKeyPairing::Unreadable {
                reason: "The .pub file is not a valid OpenSSH public key.".to_string(),
            },
            Some(public_fingerprint) if public_fingerprint == private_fingerprint.as_ref().unwrap() => {
                PublicKeyPairing::Matched
            }
            Some(public_fingerprint) => PublicKeyPairing::Mismatched {
                private_fingerprint: private_fingerprint.clone().unwrap(),
                public_fingerprint: public_fingerprint.clone(),
            },
        },
    };

    // Fall back to the `.pub` file for anything still missing.
    //
    // This is not only for unparseable legacy keys: in the OpenSSH format the
    // comment lives *inside* the encrypted blob, so a passphrase-protected key
    // reports no comment until the sidecar file is consulted. That is exactly
    // what ssh-keygen does.
    if let Some(public) = described_public {
        key.fingerprint = key.fingerprint.or(public.fingerprint);
        key.comment = key.comment.or(public.comment);
    }

    if let Some(fingerprint) = &key.fingerprint {
        key.id = fingerprint.clone();
    }

    bytes.zeroize();
    Ok(Some(key))
}

/// Delete a private key, and optionally its `.pub` sidecar.
///
/// Refuses anything that is not actually a private key, so a tampered path
/// cannot turn this into a way to remove `config` or `known_hosts`. The caller
/// is still responsible for confirming the path is inside the SSH directory.
pub fn delete(path: &Path, include_public: bool) -> SshResult<Vec<String>> {
    if inspect(path)?.is_none() {
        return Err(SshError::invalid(
            "That file is not an SSH private key, so it will not be deleted.",
        ));
    }

    let mut removed = Vec::new();

    std::fs::remove_file(path)
        .map_err(|error| SshError::io("Could not delete the key", error))?;
    removed.push(path.to_string_lossy().to_string());

    if include_public {
        let public = public_path_for(path);
        if public.is_file() {
            // The private half is already gone; a failure here is worth
            // reporting but must not read as "nothing was deleted".
            std::fs::remove_file(&public).map_err(|error| {
                SshError::Io(format!(
                    "The private key was deleted, but {} could not be removed: {error}",
                    public.display()
                ))
            })?;
            removed.push(public.to_string_lossy().to_string());
        }
    }

    Ok(removed)
}

fn describe_openssh(key: &mut SshKey, parsed: &PrivateKey) {
    let public = parsed.public_key();

    key.encrypted = parsed.is_encrypted();
    key.fingerprint = Some(parsed.fingerprint(HashAlg::Sha256).to_string());
    key.public_key_openssh = public.to_openssh().ok();

    let comment = parsed.comment().trim();
    key.comment = (!comment.is_empty()).then(|| comment.to_string());

    key.kdf = match parsed.kdf() {
        Kdf::None => KdfInfo::None,
        Kdf::Bcrypt { rounds, .. } => KdfInfo::Bcrypt { rounds: *rounds },
        // `Kdf` is #[non_exhaustive]; a future variant is better reported as
        // unknown than mistaken for "no passphrase".
        _ => KdfInfo::Unknown,
    };

    let algorithm = parsed.algorithm();
    key.algorithm_id = algorithm_id(&algorithm).to_string();
    key.bits = bit_size(public.key_data(), &algorithm);
    key.algorithm = algorithm_label(&algorithm, key.bits);
}

/// Describe a PEM key from its header alone.
fn describe_legacy(key: &mut SshKey, head: &str) {
    key.algorithm_id = if head.contains("BEGIN RSA PRIVATE KEY") {
        "rsa"
    } else if head.contains("BEGIN DSA PRIVATE KEY") {
        "dsa"
    } else if head.contains("BEGIN EC PRIVATE KEY") {
        "ecdsa"
    } else {
        "unknown"
    }
    .to_string();

    key.algorithm = match key.algorithm_id.as_str() {
        "rsa" => "RSA (PEM)",
        "dsa" => "DSA (PEM)",
        "ecdsa" => "ECDSA (PEM)",
        _ => "Unknown (PEM)",
    }
    .to_string();

    // `Proc-Type: 4,ENCRYPTED` plus `DEK-Info` is the old OpenSSL scheme: one
    // MD5 pass over the passphrase, which a GPU chews through.
    if head.contains("Proc-Type:") && head.contains("ENCRYPTED") {
        key.encrypted = true;
        let cipher = head
            .lines()
            .find_map(|line| line.trim().strip_prefix("DEK-Info:"))
            .and_then(|value| value.trim().split(',').next())
            .unwrap_or("unknown")
            .to_string();
        key.kdf = KdfInfo::LegacyPemMd5 { cipher };
    } else if head.contains("BEGIN ENCRYPTED PRIVATE KEY") {
        key.encrypted = true;
        key.kdf = KdfInfo::Unknown;
    } else {
        key.kdf = KdfInfo::None;
    }
}

pub fn describe_public(path: &Path) -> OrphanPublicKey {
    let file_name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
    let parsed = std::fs::read_to_string(path)
        .ok()
        .and_then(|text| PublicKey::from_openssh(text.trim()).ok());

    OrphanPublicKey {
        path: path.to_string_lossy().to_string(),
        file_name,
        fingerprint: parsed
            .as_ref()
            .map(|key| key.fingerprint(HashAlg::Sha256).to_string()),
        comment: parsed.as_ref().and_then(|key| {
            let comment = key.comment().trim();
            (!comment.is_empty()).then(|| comment.to_string())
        }),
    }
}

pub fn detect_format(head: &str) -> KeyFormat {
    if head.contains("BEGIN OPENSSH PRIVATE KEY") {
        KeyFormat::OpenSsh
    } else if head.contains("BEGIN RSA PRIVATE KEY")
        || head.contains("BEGIN DSA PRIVATE KEY")
        || head.contains("BEGIN EC PRIVATE KEY")
    {
        KeyFormat::LegacyPem
    } else if head.contains("BEGIN PRIVATE KEY") || head.contains("BEGIN ENCRYPTED PRIVATE KEY") {
        KeyFormat::Pkcs8
    } else {
        KeyFormat::Unknown
    }
}

pub fn algorithm_id(algorithm: &Algorithm) -> &'static str {
    match algorithm {
        Algorithm::Ed25519 => "ed25519",
        Algorithm::Rsa { .. } => "rsa",
        Algorithm::Ecdsa { .. } => "ecdsa",
        Algorithm::Dsa => "dsa",
        Algorithm::SkEd25519 => "sk-ed25519",
        Algorithm::SkEcdsaSha2NistP256 => "sk-ecdsa",
        _ => "unknown",
    }
}

fn algorithm_label(algorithm: &Algorithm, bits: Option<u32>) -> String {
    match algorithm {
        Algorithm::Ed25519 => "Ed25519".to_string(),
        Algorithm::Rsa { .. } => match bits {
            Some(bits) => format!("RSA {bits}"),
            None => "RSA".to_string(),
        },
        Algorithm::Ecdsa { curve } => format!("ECDSA {}", curve_label(curve)),
        Algorithm::Dsa => "DSA".to_string(),
        Algorithm::SkEd25519 => "Ed25519 (FIDO)".to_string(),
        Algorithm::SkEcdsaSha2NistP256 => "ECDSA P-256 (FIDO)".to_string(),
        other => other.as_str().to_string(),
    }
}

fn curve_label(curve: &EcdsaCurve) -> &'static str {
    match curve {
        EcdsaCurve::NistP256 => "P-256",
        EcdsaCurve::NistP384 => "P-384",
        EcdsaCurve::NistP521 => "P-521",
    }
}

fn bit_size(data: &KeyData, algorithm: &Algorithm) -> Option<u32> {
    match data {
        // Derive from the modulus rather than trusting a filename or label.
        KeyData::Rsa(rsa) => rsa
            .n
            .as_positive_bytes()
            .map(|bytes| (bytes.len() as u32) * 8),
        KeyData::Ed25519(_) => Some(256),
        KeyData::Ecdsa(_) => match algorithm {
            Algorithm::Ecdsa { curve } => Some(match curve {
                EcdsaCurve::NistP256 => 256,
                EcdsaCurve::NistP384 => 384,
                EcdsaCurve::NistP521 => 521,
            }),
            _ => None,
        },
        // SSH only ever specified 1024-bit DSA.
        KeyData::Dsa(_) => Some(1024),
        _ => None,
    }
}

fn modified_millis(path: &Path) -> Option<i64> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    let duration = modified.duration_since(UNIX_EPOCH).ok()?;
    Some(duration.as_millis() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_key_formats_from_headers() {
        assert_eq!(
            detect_format("-----BEGIN OPENSSH PRIVATE KEY-----\nabc\n"),
            KeyFormat::OpenSsh
        );
        assert_eq!(
            detect_format("-----BEGIN RSA PRIVATE KEY-----\nabc\n"),
            KeyFormat::LegacyPem
        );
        assert_eq!(
            detect_format("-----BEGIN EC PRIVATE KEY-----\nabc\n"),
            KeyFormat::LegacyPem
        );
        assert_eq!(
            detect_format("-----BEGIN ENCRYPTED PRIVATE KEY-----\nabc\n"),
            KeyFormat::Pkcs8
        );
        assert_eq!(detect_format("ssh-ed25519 AAAAC3 me@host\n"), KeyFormat::Unknown);
        assert_eq!(detect_format("Host example\n  User me\n"), KeyFormat::Unknown);
    }

    #[test]
    fn reads_legacy_pem_encryption_details() {
        let head = "-----BEGIN RSA PRIVATE KEY-----\n\
                    Proc-Type: 4,ENCRYPTED\n\
                    DEK-Info: AES-128-CBC,9A1C0B21\n\n\
                    base64here\n";

        let mut key = blank_key();
        describe_legacy(&mut key, head);

        assert_eq!(key.algorithm_id, "rsa");
        assert!(key.encrypted);
        assert_eq!(
            key.kdf,
            KdfInfo::LegacyPemMd5 {
                cipher: "AES-128-CBC".to_string()
            }
        );
    }

    #[test]
    fn unencrypted_legacy_pem_reports_no_kdf() {
        let mut key = blank_key();
        describe_legacy(&mut key, "-----BEGIN DSA PRIVATE KEY-----\nbase64\n");

        assert_eq!(key.algorithm_id, "dsa");
        assert!(!key.encrypted);
        assert_eq!(key.kdf, KdfInfo::None);
    }

    #[test]
    fn recognises_stray_copies() {
        let mut key = blank_key();
        for name in ["id_rsa.bak", "id_rsa~", "id_ed25519.old", "KEY.SAVE"] {
            key.file_name = name.to_string();
            assert!(key.is_stray_copy(), "{name} should look like a stray copy");
        }

        key.file_name = "id_ed25519".to_string();
        assert!(!key.is_stray_copy());
    }

    #[test]
    fn public_path_appends_the_suffix() {
        assert_eq!(
            public_path_for(Path::new("/home/me/.ssh/id_ed25519")),
            PathBuf::from("/home/me/.ssh/id_ed25519.pub")
        );
    }

    fn blank_key() -> SshKey {
        SshKey {
            id: String::new(),
            path: String::new(),
            file_name: String::new(),
            algorithm_id: "unknown".into(),
            algorithm: "Unknown".into(),
            bits: None,
            fingerprint: None,
            comment: None,
            encrypted: false,
            kdf: KdfInfo::Unknown,
            format: KeyFormat::LegacyPem,
            public_key_path: None,
            public_key_openssh: None,
            pairing: PublicKeyPairing::Missing,
            permissions: KeyPermissions::Unknown {
                reason: "test".into(),
            },
            modified_ms: None,
            parse_error: None,
        }
    }
}
