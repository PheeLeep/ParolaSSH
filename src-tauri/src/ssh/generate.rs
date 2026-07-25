//! Creating new SSH keys.
//!
//! The passphrase arrives from the frontend, is used once, and is zeroized
//! before this module returns. It is never written anywhere but into the
//! bcrypt-protected key file itself.
//!
//! New keys are written with owner-only permissions *before* any key material
//! reaches the disk, so there is no window where the file exists and is
//! world-readable.

use std::path::Path;

use serde::{Deserialize, Serialize};
use ssh_key::private::{KeypairData, RsaKeypair};
use ssh_key::rand_core::OsRng;
use ssh_key::{Algorithm, EcdsaCurve, LineEnding, PrivateKey};
use zeroize::Zeroize;

use super::keys::{self, SshKey};
use super::perms;
use super::{SshError, SshResult};

/// The strongest option that every current OpenSSH accepts.
const DEFAULT_RSA_BITS: u32 = 4096;
/// Matches `ssh-keygen`'s own floor.
const MIN_RSA_BITS: u32 = 2048;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateRequest {
    /// `ed25519`, `rsa`, `ecdsa`.
    pub algorithm: String,
    /// RSA bit size, or the ECDSA curve size (256, 384, 521).
    pub bits: Option<u32>,
    /// File name inside the SSH directory, e.g. `id_ed25519`.
    pub file_name: String,
    pub comment: Option<String>,
    /// Empty means no passphrase — allowed, but the audit will flag it.
    pub passphrase: Option<String>,
    /// Overwrite an existing file. Defaults to refusing.
    #[serde(default)]
    pub overwrite: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateOutcome {
    pub key: SshKey,
    pub private_key_path: String,
    pub public_key_path: String,
    /// The public key line, safe to display and copy.
    pub public_key_openssh: String,
}

/// Validate a requested file name.
///
/// Rejects anything that could escape the SSH directory. Path separators are
/// checked for both platforms because a name typed on Linux may still contain
/// a backslash, and `..` is refused outright.
pub fn validate_file_name(name: &str) -> SshResult<&str> {
    let trimmed = name.trim();

    if trimmed.is_empty() {
        return Err(SshError::invalid("Give the key a file name."));
    }
    if trimmed.contains('/') || trimmed.contains('\\') {
        return Err(SshError::invalid(
            "The file name cannot contain a path separator.",
        ));
    }
    if trimmed == "." || trimmed == ".." || trimmed.starts_with("..") {
        return Err(SshError::invalid("That file name is not allowed."));
    }
    if trimmed.ends_with(".pub") {
        return Err(SshError::invalid(
            "Leave off the .pub — it is added to the public key automatically.",
        ));
    }
    // Reserved device names on Windows resolve to hardware, not files.
    let stem = trimmed.split('.').next().unwrap_or(trimmed).to_uppercase();
    const RESERVED: &[&str] = &[
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "LPT1", "LPT2", "LPT3",
    ];
    if RESERVED.contains(&stem.as_str()) {
        return Err(SshError::invalid(
            "That name is reserved by Windows — pick another.",
        ));
    }

    Ok(trimmed)
}

fn resolve_algorithm(request: &GenerateRequest) -> SshResult<Algorithm> {
    match request.algorithm.to_lowercase().as_str() {
        "ed25519" => Ok(Algorithm::Ed25519),
        "rsa" => Ok(Algorithm::Rsa { hash: None }),
        "ecdsa" => {
            let curve = match request.bits.unwrap_or(256) {
                256 => EcdsaCurve::NistP256,
                384 => EcdsaCurve::NistP384,
                521 => EcdsaCurve::NistP521,
                other => {
                    return Err(SshError::invalid(format!(
                        "ECDSA supports the P-256, P-384 and P-521 curves, not {other}."
                    )))
                }
            };
            Ok(Algorithm::Ecdsa { curve })
        }
        other => Err(SshError::unsupported(format!(
            "{other} keys cannot be created here. Use Ed25519, RSA or ECDSA."
        ))),
    }
}

fn build_key(request: &GenerateRequest, algorithm: Algorithm) -> SshResult<PrivateKey> {
    // `PrivateKey::random` hardcodes 4096 for RSA, so RSA goes through
    // RsaKeypair directly to honour the requested size.
    if let Algorithm::Rsa { .. } = algorithm {
        let bits = request.bits.unwrap_or(DEFAULT_RSA_BITS);
        if bits < MIN_RSA_BITS {
            return Err(SshError::invalid(format!(
                "RSA keys must be at least {MIN_RSA_BITS} bits; {bits} would be rejected by ssh."
            )));
        }

        let keypair = RsaKeypair::random(&mut OsRng, bits as usize)
            .map_err(|error| SshError::invalid(format!("Could not generate an RSA key: {error}")))?;

        return PrivateKey::try_from(KeypairData::from(keypair))
            .map_err(|error| SshError::invalid(format!("Could not build the key: {error}")));
    }

    PrivateKey::random(&mut OsRng, algorithm)
        .map_err(|error| SshError::invalid(format!("Could not generate the key: {error}")))
}

/// Generate a key pair inside `ssh_dir` and return its metadata.
pub fn generate(ssh_dir: &Path, request: GenerateRequest) -> SshResult<GenerateOutcome> {
    let file_name = validate_file_name(&request.file_name)?;
    let algorithm = resolve_algorithm(&request)?;

    let private_path = ssh_dir.join(file_name);
    let public_path = ssh_dir.join(format!("{file_name}.pub"));

    if !request.overwrite && (private_path.exists() || public_path.exists()) {
        return Err(SshError::invalid(format!(
            "{file_name} already exists. Choose another name, or confirm replacing it."
        )));
    }

    if !ssh_dir.exists() {
        std::fs::create_dir_all(ssh_dir)
            .map_err(|error| SshError::io("Could not create the SSH directory", error))?;
        // A new directory starts locked down rather than inheriting a default.
        perms::restrict_to_owner(ssh_dir, true)?;
    }

    let mut key = build_key(&request, algorithm)?;

    let comment = request
        .comment
        .as_deref()
        .map(str::trim)
        .filter(|comment| !comment.is_empty())
        .map(str::to_string);

    // Set before encrypting so the comment is sealed inside the private blob
    // too, matching what ssh-keygen produces.
    if let Some(comment) = &comment {
        key.set_comment(comment);
    }

    // Encrypt before writing so an unencrypted version never touches the disk.
    let mut passphrase = request.passphrase.unwrap_or_default();
    let mut key = if passphrase.is_empty() {
        key
    } else {
        let encrypted = key.encrypt(&mut OsRng, passphrase.as_bytes()).map_err(|error| {
            SshError::invalid(format!("Could not encrypt the key: {error}"))
        })?;
        passphrase.zeroize();
        encrypted
    };
    passphrase.zeroize();

    // Encrypting rebuilds the public half from raw key data, which drops the
    // comment — re-apply it so the `.pub` file carries it. This matters
    // because an encrypted key's internal comment is unreadable without the
    // passphrase, so the sidecar is the only place the UI can get it from.
    if let Some(comment) = &comment {
        key.set_comment(comment);
    }

    let public_key_openssh = key
        .public_key()
        .to_openssh()
        .map_err(|error| SshError::invalid(format!("Could not encode the public key: {error}")))?;

    write_private_key(&private_path, &key)?;

    std::fs::write(&public_path, format!("{public_key_openssh}\n"))
        .map_err(|error| SshError::io("Could not write the public key", error))?;

    let described = keys::inspect(&private_path)?.ok_or_else(|| {
        SshError::invalid("The key was written but could not be read back for verification.")
    })?;

    Ok(GenerateOutcome {
        key: described,
        private_key_path: private_path.to_string_lossy().to_string(),
        public_key_path: public_path.to_string_lossy().to_string(),
        public_key_openssh,
    })
}

/// Write the private key, ensuring it is owner-only from the moment it exists.
///
/// On Unix the file is created with mode 0600 in the same syscall that creates
/// it. Elsewhere it is written and then immediately restricted — a narrower
/// window than writing first and fixing later, which is what `restrict_to_owner`
/// closes.
fn write_private_key(path: &Path, key: &PrivateKey) -> SshResult<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;

        // `Zeroizing<String>` — the encoded key is wiped from memory on drop,
        // including on the error paths below.
        let pem = key
            .to_openssh(LineEnding::LF)
            .map_err(|error| SshError::invalid(format!("Could not encode the key: {error}")))?;

        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)
            .map_err(|error| SshError::io("Could not create the key file", error))?;

        file.write_all(pem.as_bytes())
            .map_err(|error| SshError::io("Could not write the key file", error))?;
    }

    #[cfg(not(unix))]
    {
        key.write_openssh_file(path, LineEnding::LF)
            .map_err(|error| SshError::invalid(format!("Could not write the key file: {error}")))?;
        perms::restrict_to_owner(path, false)?;
    }

    Ok(())
}

/// Confirm a passphrase unlocks a key, without keeping either.
pub fn verify_passphrase(path: &Path, passphrase: &str) -> SshResult<bool> {
    let key = PrivateKey::read_openssh_file(path)
        .map_err(|error| SshError::invalid(format!("Could not read the key: {error}")))?;

    if !key.is_encrypted() {
        return Ok(true);
    }

    Ok(key.decrypt(passphrase.as_bytes()).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_names_that_escape_the_directory() {
        for name in ["../id_rsa", "keys/id_rsa", "keys\\id_rsa", "..", "."] {
            assert!(
                validate_file_name(name).is_err(),
                "{name} should be rejected"
            );
        }
    }

    #[test]
    fn rejects_empty_and_pub_suffixed_names() {
        assert!(validate_file_name("   ").is_err());
        assert!(validate_file_name("id_ed25519.pub").is_err());
    }

    #[test]
    fn rejects_windows_device_names() {
        // These resolve to devices rather than files on Windows, so the check
        // runs everywhere to keep a key created on Linux portable.
        assert!(validate_file_name("CON").is_err());
        assert!(validate_file_name("nul.key").is_err());
        assert!(validate_file_name("com1").is_err());
    }

    #[test]
    fn accepts_ordinary_names_and_trims() {
        assert_eq!(validate_file_name("  id_ed25519  ").unwrap(), "id_ed25519");
        assert_eq!(validate_file_name("work-key").unwrap(), "work-key");
    }

    #[test]
    fn resolves_supported_algorithms() {
        let request = |algorithm: &str, bits: Option<u32>| GenerateRequest {
            algorithm: algorithm.to_string(),
            bits,
            file_name: "k".into(),
            comment: None,
            passphrase: None,
            overwrite: false,
        };

        assert_eq!(
            resolve_algorithm(&request("ed25519", None)).unwrap(),
            Algorithm::Ed25519
        );
        assert_eq!(
            resolve_algorithm(&request("ECDSA", Some(384))).unwrap(),
            Algorithm::Ecdsa {
                curve: EcdsaCurve::NistP384
            }
        );
        assert!(resolve_algorithm(&request("ecdsa", Some(512))).is_err());
        assert!(resolve_algorithm(&request("dsa", None)).is_err());
    }
}
