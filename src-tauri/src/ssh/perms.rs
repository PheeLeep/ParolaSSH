//! File permissions, modelled per-platform rather than flattened.
//!
//! A `u32` mode is meaningless on Windows: calling such a file "0600" would
//! report a key as safe while every member of `Users` can read it. So the two
//! stay separate variants and the audit rules branch on them.

use std::path::Path;

use serde::Serialize;

use super::SshResult;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum KeyPermissions {
    /// Unix mode bits.
    #[serde(rename_all = "camelCase")]
    Posix {
        /// The permission bits only (`mode & 0o7777`).
        mode: u32,
        /// `"0600"`, for display.
        display: String,
    },

    /// Windows ACL, reduced to the principals holding an access entry.
    /// Constructed only on Windows, but compiled and tested everywhere so the
    /// rules consuming it cannot rot.
    #[cfg_attr(not(windows), allow(dead_code))]
    #[serde(rename_all = "camelCase")]
    Windows {
        principals: Vec<String>,
        /// True when the file still inherits ACEs from its parent directory.
        inherited: bool,
    },

    /// The platform is neither, or the ACL could not be read.
    #[serde(rename_all = "camelCase")]
    Unknown { reason: String },
}

impl KeyPermissions {
    /// True when someone other than the owner can reach the file. `None` when
    /// permissions could not be determined, so "not exposed" and "don't know"
    /// stay distinguishable.
    pub fn is_exposed(&self) -> Option<bool> {
        match self {
            Self::Posix { mode, .. } => Some(mode & 0o077 != 0),
            Self::Windows {
                principals,
                inherited,
            } => Some(*inherited || principals.iter().any(|p| !is_trusted_principal(p))),
            Self::Unknown { .. } => None,
        }
    }

    /// True when group or others can *write* — sshd rejects a config or
    /// `authorized_keys` file in this state.
    pub fn is_group_or_world_writable(&self) -> Option<bool> {
        match self {
            Self::Posix { mode, .. } => Some(mode & 0o022 != 0),
            Self::Windows {
                principals,
                inherited,
            } => Some(*inherited || principals.iter().any(|p| !is_trusted_principal(p))),
            Self::Unknown { .. } => None,
        }
    }

    pub fn describe(&self) -> String {
        match self {
            Self::Posix { display, .. } => display.clone(),
            Self::Windows { principals, .. } if principals.is_empty() => {
                "No access entries".to_string()
            }
            Self::Windows { principals, .. } => principals.join(", "),
            Self::Unknown { reason } => reason.clone(),
        }
    }
}

/// Principals that legitimately hold access to a user's own files on Windows.
fn is_trusted_principal(principal: &str) -> bool {
    let upper = principal.to_uppercase();
    let bare = upper.rsplit('\\').next().unwrap_or(&upper).to_string();

    if matches!(
        bare.as_str(),
        "SYSTEM" | "ADMINISTRATORS" | "TRUSTEDINSTALLER"
    ) {
        return true;
    }

    // The owner themselves.
    current_windows_user()
        .map(|user| user.to_uppercase() == upper || user.to_uppercase() == bare)
        .unwrap_or(false)
}

fn current_windows_user() -> Option<String> {
    let name = std::env::var("USERNAME").ok()?;
    match std::env::var("USERDOMAIN") {
        Ok(domain) if !domain.is_empty() => Some(format!("{domain}\\{name}")),
        _ => Some(name),
    }
}

pub fn read_permissions(path: &Path) -> KeyPermissions {
    #[cfg(unix)]
    {
        read_posix(path)
    }
    #[cfg(windows)]
    {
        read_windows(path)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
        KeyPermissions::Unknown {
            reason: "Unsupported platform".to_string(),
        }
    }
}

#[cfg(unix)]
fn read_posix(path: &Path) -> KeyPermissions {
    use std::os::unix::fs::PermissionsExt;

    match std::fs::metadata(path) {
        Ok(metadata) => {
            let mode = metadata.permissions().mode() & 0o7777;
            KeyPermissions::Posix {
                mode,
                display: format!("{mode:04o}"),
            }
        }
        Err(error) => KeyPermissions::Unknown {
            reason: format!("Could not read permissions: {error}"),
        },
    }
}

#[cfg(windows)]
fn read_windows(path: &Path) -> KeyPermissions {
    use std::process::Command;

    // Shelling out to `icacls` keeps this branch free of unsafe DACL FFI, and
    // the parsing below stays pure and testable on any platform.
    let output = Command::new("icacls").arg(path).output();

    match output {
        Ok(output) if output.status.success() => {
            let text = String::from_utf8_lossy(&output.stdout);
            let (principals, inherited) = parse_icacls(&text, &path.to_string_lossy());
            KeyPermissions::Windows {
                principals,
                inherited,
            }
        }
        Ok(output) => KeyPermissions::Unknown {
            reason: format!(
                "icacls failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        },
        Err(error) => KeyPermissions::Unknown {
            reason: format!("Could not run icacls: {error}"),
        },
    }
}

/// Extract the principals and inheritance flag from `icacls` output.
///
/// ```text
/// C:\Users\me\.ssh\id_ed25519 NT AUTHORITY\SYSTEM:(F)
///                             BUILTIN\Administrators:(F)
/// ```
///
/// The first line carries the path then the first ACE; later ACEs are indented,
/// and inherited entries are marked `(I)`. Both paths and principals may
/// contain spaces, so stripping the known `path` is the only unambiguous way to
/// find where the first principal begins.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn parse_icacls(output: &str, path: &str) -> (Vec<String>, bool) {
    let mut principals = Vec::new();
    let mut inherited = false;

    for (index, line) in output.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("Successfully processed") {
            continue;
        }

        let ace = if index == 0 {
            match strip_path_prefix(trimmed, path) {
                Some(rest) => rest,
                None => continue,
            }
        } else {
            trimmed
        };

        let Some((principal, rights)) = split_ace(ace) else {
            continue;
        };

        if rights.contains("(I)") {
            inherited = true;
        }

        let principal = principal.trim().to_string();
        if !principal.is_empty() && !principals.contains(&principal) {
            principals.push(principal);
        }
    }

    (principals, inherited)
}

/// Remove the leading file path from the first line of icacls output. Falls
/// back to the last space before the rights group when icacls echoes the path
/// in a different case or normalised form.
#[cfg_attr(not(windows), allow(dead_code))]
fn strip_path_prefix<'a>(line: &'a str, path: &str) -> Option<&'a str> {
    if !path.is_empty() && line.len() > path.len() {
        // Windows paths are case-insensitive, so compare that way.
        if line[..path.len()].eq_ignore_ascii_case(path) {
            return Some(line[path.len()..].trim_start());
        }
    }

    let colon = line.rfind(":(")?;
    line[..colon].rfind(' ').map(|space| &line[space + 1..])
}

#[cfg_attr(not(windows), allow(dead_code))]
fn split_ace(ace: &str) -> Option<(&str, &str)> {
    let colon = ace.rfind(":(")?;
    Some((&ace[..colon], &ace[colon..]))
}

/// Restrict a file so only its owner can read it. POSIX sets `0600` (`0700` for
/// directories); Windows drops inheritance and grants the current user full
/// control.
pub fn restrict_to_owner(path: &Path, is_dir: bool) -> SshResult<KeyPermissions> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mode = if is_dir { 0o700 } else { 0o600 };
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
            .map_err(|error| super::SshError::io("Could not change permissions", error))?;
    }

    #[cfg(windows)]
    {
        use std::process::Command;

        let _ = is_dir;
        let user = current_windows_user()
            .ok_or_else(|| super::SshError::invalid("Could not determine the current user."))?;

        let output = Command::new("icacls")
            .arg(path)
            .arg("/inheritance:r")
            .arg("/grant:r")
            .arg(format!("{user}:F"))
            .output()
            .map_err(|error| super::SshError::io("Could not run icacls", error))?;

        if !output.status.success() {
            return Err(super::SshError::invalid(format!(
                "icacls could not update permissions: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = is_dir;
        return Err(super::SshError::unsupported(
            "Changing permissions is not supported on this platform.",
        ));
    }

    Ok(read_permissions(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn posix_exposure_looks_at_group_and_other_bits() {
        let private = KeyPermissions::Posix {
            mode: 0o600,
            display: "0600".into(),
        };
        assert_eq!(private.is_exposed(), Some(false));

        let leaky = KeyPermissions::Posix {
            mode: 0o644,
            display: "0644".into(),
        };
        assert_eq!(leaky.is_exposed(), Some(true));

        let group_only = KeyPermissions::Posix {
            mode: 0o640,
            display: "0640".into(),
        };
        assert_eq!(group_only.is_exposed(), Some(true));
    }

    #[test]
    fn posix_writability_ignores_read_bits() {
        let readable = KeyPermissions::Posix {
            mode: 0o644,
            display: "0644".into(),
        };
        assert_eq!(readable.is_group_or_world_writable(), Some(false));

        let writable = KeyPermissions::Posix {
            mode: 0o664,
            display: "0664".into(),
        };
        assert_eq!(writable.is_group_or_world_writable(), Some(true));
    }

    #[test]
    fn unknown_permissions_do_not_claim_safety() {
        let unknown = KeyPermissions::Unknown {
            reason: "nope".into(),
        };
        assert_eq!(unknown.is_exposed(), None);
        assert_eq!(unknown.is_group_or_world_writable(), None);
    }

    #[test]
    fn parses_icacls_output() {
        let output = "C:\\Users\\me\\.ssh\\id_ed25519 NT AUTHORITY\\SYSTEM:(F)\r\n\
                      \x20                           BUILTIN\\Administrators:(F)\r\n\
                      \x20                           DESKTOP-1\\me:(F)\r\n\
                      \r\n\
                      Successfully processed 1 files; Failed processing 0 files\r\n";

        let (principals, inherited) = parse_icacls(output, "C:\\Users\\me\\.ssh\\id_ed25519");
        assert_eq!(
            principals,
            vec![
                "NT AUTHORITY\\SYSTEM",
                "BUILTIN\\Administrators",
                "DESKTOP-1\\me"
            ]
        );
        assert!(!inherited);
    }

    #[test]
    fn detects_inherited_entries() {
        let output = "C:\\Users\\me\\.ssh\\id_rsa BUILTIN\\Users:(I)(F)\r\n\
                      \r\n\
                      Successfully processed 1 files; Failed processing 0 files\r\n";

        let (principals, inherited) = parse_icacls(output, "C:\\Users\\me\\.ssh\\id_rsa");
        assert_eq!(principals, vec!["BUILTIN\\Users"]);
        assert!(inherited);
    }

    #[test]
    fn handles_spaces_in_both_the_path_and_the_principal() {
        // The hard case: a space-containing path followed by a
        // space-containing principal cannot be split without knowing the path.
        let output = "C:\\My Keys\\id_rsa NT AUTHORITY\\SYSTEM:(F)\r\n";
        let (principals, _) = parse_icacls(output, "C:\\My Keys\\id_rsa");
        assert_eq!(principals, vec!["NT AUTHORITY\\SYSTEM"]);
    }

    #[test]
    fn falls_back_when_the_path_does_not_match_verbatim() {
        // icacls echoed a different form of the path; the heuristic still
        // recovers a single-token principal.
        let output = "D:\\keys\\id_rsa BUILTIN\\Users:(F)\r\n";
        let (principals, _) = parse_icacls(output, "\\\\?\\D:\\keys\\id_rsa");
        assert_eq!(principals, vec!["BUILTIN\\Users"]);
    }

    #[test]
    fn matches_the_path_case_insensitively() {
        let output = "C:\\Users\\Me\\.ssh\\id_rsa NT AUTHORITY\\SYSTEM:(F)\r\n";
        let (principals, _) = parse_icacls(output, "c:\\users\\me\\.ssh\\id_rsa");
        assert_eq!(principals, vec!["NT AUTHORITY\\SYSTEM"]);
    }

    #[test]
    fn windows_permissions_flag_untrusted_principals() {
        let exposed = KeyPermissions::Windows {
            principals: vec!["BUILTIN\\Users".into()],
            inherited: false,
        };
        assert_eq!(exposed.is_exposed(), Some(true));

        let system_only = KeyPermissions::Windows {
            principals: vec!["NT AUTHORITY\\SYSTEM".into(), "BUILTIN\\Administrators".into()],
            inherited: false,
        };
        assert_eq!(system_only.is_exposed(), Some(false));
    }

    #[test]
    fn inheritance_alone_counts_as_exposure() {
        // Inherited ACEs come from the parent directory and typically widen
        // access, so an inherited-but-otherwise-clean file is still a finding.
        let inherited = KeyPermissions::Windows {
            principals: vec!["NT AUTHORITY\\SYSTEM".into()],
            inherited: true,
        };
        assert_eq!(inherited.is_exposed(), Some(true));
    }
}
