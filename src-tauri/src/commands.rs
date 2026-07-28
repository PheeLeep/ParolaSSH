//! The Tauri command surface for SSH keys.
//!
//! The only way the webview reaches the filesystem. There is deliberately no
//! filesystem plugin scoped to `~/.ssh`: with purpose-built commands only, a
//! compromised frontend can ask for key metadata but never key material.

use std::path::{Path, PathBuf};

use tauri::AppHandle;

use crate::app_paths::config_dir;
use crate::ssh::audit::AuditReport;
use crate::ssh::generate::{GenerateOutcome, GenerateRequest};
use crate::ssh::keys::KeyScan;
use crate::ssh::paths::SshPaths;
use crate::ssh::perms::KeyPermissions;
use crate::ssh::store::Suppressions;
use crate::ssh::{audit, config, generate, keys, perms, SshError, SshResult};

fn ssh_dir() -> SshResult<PathBuf> {
    SshPaths::discover().map(|paths| paths.dir)
}

/// Confirm a path lies inside the SSH directory before acting on it. Paths come
/// from a scan, but the frontend is not a trust boundary, so fixes stay
/// constrained to the directory the audit covers.
fn ensure_within_ssh_dir(path: &Path, ssh_dir: &Path) -> SshResult<()> {
    let canonical_target = path
        .canonicalize()
        .map_err(|error| SshError::io("Could not resolve that path", error))?;
    let canonical_root = ssh_dir
        .canonicalize()
        .map_err(|error| SshError::io("Could not resolve the SSH directory", error))?;

    if canonical_target == canonical_root || canonical_target.starts_with(&canonical_root) {
        return Ok(());
    }

    Err(SshError::invalid(
        "That file is outside your SSH directory, so it will not be modified here.",
    ))
}

/// Where the app is looking, and whether anything is there yet.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SshLocation {
    pub path: String,
    pub exists: bool,
}

#[tauri::command]
pub fn ssh_location() -> SshResult<SshLocation> {
    let dir = ssh_dir()?;
    Ok(SshLocation {
        path: dir.to_string_lossy().to_string(),
        exists: dir.is_dir(),
    })
}

/// List the private keys in `~/.ssh`, plus any referenced from the config.
#[tauri::command]
pub fn list_ssh_keys() -> SshResult<KeyScan> {
    let dir = ssh_dir()?;
    let config = config::SshConfig::read(&SshPaths::new(&dir).config());
    keys::scan(&dir, &config.identity_files())
}

/// Run the security audit over `~/.ssh`.
#[tauri::command]
pub fn audit_ssh_dir(app: AppHandle) -> SshResult<AuditReport> {
    let dir = ssh_dir()?;
    let suppressed = Suppressions::read(&config_dir(&app)?).as_set();
    audit::run(&dir, &suppressed)
}

/// Dismiss or restore a finding. Returns the refreshed report.
#[tauri::command]
pub fn set_finding_suppressed(
    app: AppHandle,
    finding_id: String,
    suppressed: bool,
) -> SshResult<AuditReport> {
    let config_dir = config_dir(&app)?;
    let mut suppressions = Suppressions::read(&config_dir);

    if suppressed {
        suppressions.insert(finding_id);
    } else {
        suppressions.remove(&finding_id);
    }
    suppressions.write(&config_dir)?;

    let dir = ssh_dir()?;
    audit::run(&dir, &suppressions.as_set())
}

/// Tighten a file or directory to owner-only access. The only automatic fix,
/// and only one explicitly requested path at a time - a bulk chmod that guesses
/// wrong on a symlinked dotfiles repo is worse than the finding.
#[tauri::command]
pub fn restrict_permissions(path: String, is_dir: bool) -> SshResult<KeyPermissions> {
    let dir = ssh_dir()?;
    let target = PathBuf::from(&path);

    ensure_within_ssh_dir(&target, &dir)?;
    perms::restrict_to_owner(&target, is_dir)
}

/// Create a new key pair in `~/.ssh`.
#[tauri::command]
pub fn generate_ssh_key(request: GenerateRequest) -> SshResult<GenerateOutcome> {
    let dir = ssh_dir()?;
    generate::generate(&dir, request)
}

/// Permanently delete a private key, and optionally its `.pub` sidecar. The
/// path must resolve inside the SSH directory and the file must parse as a
/// private key; confirming intent is the UI's job.
#[tauri::command]
pub fn delete_ssh_key(path: String, include_public: bool) -> SshResult<Vec<String>> {
    let dir = ssh_dir()?;
    let target = PathBuf::from(&path);

    ensure_within_ssh_dir(&target, &dir)?;
    keys::delete(&target, include_public)
}

/// Check a passphrase without storing it.
#[tauri::command]
pub fn verify_key_passphrase(path: String, passphrase: String) -> SshResult<bool> {
    let dir = ssh_dir()?;
    let target = PathBuf::from(&path);

    ensure_within_ssh_dir(&target, &dir)?;
    generate::verify_passphrase(&target, &passphrase)
}

/// Read a `.pub` file so the UI can show and copy it. Restricted to public keys
/// by extension, so it never becomes a way to read `id_ed25519` itself.
#[tauri::command]
pub fn read_public_key(path: String) -> SshResult<String> {
    let dir = ssh_dir()?;
    let target = PathBuf::from(&path);

    ensure_within_ssh_dir(&target, &dir)?;

    if target.extension().and_then(|ext| ext.to_str()) != Some("pub") {
        return Err(SshError::invalid("Only public keys can be read."));
    }

    std::fs::read_to_string(&target)
        .map(|text| text.trim().to_string())
        .map_err(|error| SshError::io("Could not read the public key", error))
}

/* ── The app's own log ──────────────────────────────────────────────────── */

/// Where the log file is, so Settings can show the path and reveal it.
#[tauri::command]
pub fn app_log_location() -> Option<crate::logging::LogLocation> {
    crate::logging::location()
}

/// The tail of the log, newest last. Capped so a long-running session cannot
/// hand the webview a multi-megabyte array.
#[tauri::command]
pub fn read_app_log(max_lines: Option<usize>) -> Vec<crate::logging::LogEntry> {
    crate::logging::read_entries(max_lines.unwrap_or(1_000).min(5_000))
}

/// Truncate the log. Logging continues into a fresh file afterwards.
#[tauri::command]
pub fn clear_app_log() {
    crate::logging::clear();
    crate::logging::info("app", "Log cleared");
}
