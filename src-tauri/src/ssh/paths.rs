//! Locating the user's SSH directory.
//!
//! Resolution goes through `dirs::home_dir()` rather than `$HOME` so it behaves
//! on Windows (`%USERPROFILE%\.ssh`) as well as on macOS and Linux.

use std::path::{Path, PathBuf};

use super::{SshError, SshResult};

/// The well-known files inside an SSH directory.
pub struct SshPaths {
    pub dir: PathBuf,
}

impl SshPaths {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// `~/.ssh` for the current user.
    pub fn discover() -> SshResult<Self> {
        let home = dirs::home_dir().ok_or(SshError::NoHomeDir)?;
        Ok(Self::new(home.join(".ssh")))
    }

    pub fn config(&self) -> PathBuf {
        self.dir.join("config")
    }

    pub fn known_hosts(&self) -> PathBuf {
        self.dir.join("known_hosts")
    }

    pub fn authorized_keys(&self) -> PathBuf {
        self.dir.join("authorized_keys")
    }
}

/// Whether `path` is a symlink, and what it points at.
///
/// `~/.ssh` symlinked into a dotfiles repository is common enough that the
/// audit reports it: permission findings apply to the *target*, and any fix we
/// apply may touch a git-tracked file.
pub fn symlink_target(path: &Path) -> Option<PathBuf> {
    let metadata = std::fs::symlink_metadata(path).ok()?;
    if !metadata.file_type().is_symlink() {
        return None;
    }
    std::fs::read_link(path)
        .ok()
        .map(|target| resolve_relative(path, target))
}

fn resolve_relative(link: &Path, target: PathBuf) -> PathBuf {
    if target.is_absolute() {
        target
    } else {
        link.parent()
            .map(|parent| parent.join(&target))
            .unwrap_or(target)
    }
}

/// Expand a leading `~` in a path read from `~/.ssh/config`.
pub fn expand_tilde(raw: &str) -> PathBuf {
    let trimmed = raw.trim();
    if trimmed == "~" {
        return dirs::home_dir().unwrap_or_else(|| PathBuf::from(trimmed));
    }
    // Accept both separators: a Windows config may well contain `~/.ssh/id_rsa`.
    if let Some(rest) = trimmed
        .strip_prefix("~/")
        .or_else(|| trimmed.strip_prefix("~\\"))
    {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_only_a_leading_tilde() {
        let home = dirs::home_dir().unwrap();
        assert_eq!(expand_tilde("~/.ssh/id_ed25519"), home.join(".ssh/id_ed25519"));
        assert_eq!(expand_tilde("~"), home);
        // A tilde anywhere else is a literal character in a filename.
        assert_eq!(expand_tilde("/etc/ssh~/key"), PathBuf::from("/etc/ssh~/key"));
        assert_eq!(expand_tilde("relative/key"), PathBuf::from("relative/key"));
    }

    #[test]
    fn well_known_files_hang_off_the_directory() {
        let paths = SshPaths::new("/home/someone/.ssh");
        assert_eq!(paths.config(), PathBuf::from("/home/someone/.ssh/config"));
        assert_eq!(
            paths.known_hosts(),
            PathBuf::from("/home/someone/.ssh/known_hosts")
        );
    }
}
