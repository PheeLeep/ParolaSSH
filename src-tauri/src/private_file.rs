//! Writing the app's own config files so only the owner can read them.
//!
//! `hosts.json` is a list of every machine you administer — hostnames, accounts,
//! ports, key paths. The audit already scores an unhashed `known_hosts` as "a
//! readable map of everything you connect to"; this app writes the same map, so
//! it gets the same protection rather than whatever `umask` happened to be.
//!
//! Permissions are set when the file is created, before any content reaches the
//! disk, so there is no window where the map exists and is world-readable — the
//! rule `generate.rs` already follows for private keys.
//!
//! Platforms differ in what "owner-only" costs:
//!
//! | | Mechanism |
//! |---|---|
//! | Unix (Linux, macOS, BSD) | Explicit `0600` on files, `0700` on the directory — `umask` is typically `022`, so the default would be world-readable |
//! | Windows | Inherited from `%APPDATA%`, which is already per-user: another non-administrator account cannot read it |
//!
//! Windows is not given a hand-rolled ACL. Getting DACLs subtly wrong is worse
//! than inheriting a correct one, and the inherited one is correct here.

use std::path::Path;

use crate::ssh::{SshError, SshResult};

/// Create the settings directory, owner-only on Unix.
pub fn create_dir(dir: &Path) -> SshResult<()> {
    std::fs::create_dir_all(dir)
        .map_err(|error| SshError::io("Could not create the settings directory", error))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Best-effort: a directory we cannot chmod is still usable, and failing
        // the save over it would lose the user's edit.
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    }

    Ok(())
}

/// Write `text` to `dir/file_name`, owner-only, replacing any existing file
/// atomically.
///
/// The content goes to a temporary file that is renamed over the target, so a
/// crash mid-write leaves the previous version intact rather than a truncated
/// one. The rename also repairs the permissions of a file written by an earlier
/// version of the app: the new inode carries the new mode.
pub fn write(dir: &Path, file_name: &str, text: &str) -> SshResult<()> {
    create_dir(dir)?;

    let final_path = dir.join(file_name);
    let temp_path = dir.join(format!("{file_name}.tmp"));

    write_owner_only(&temp_path, text)?;

    // Atomic on the same filesystem on every platform we target; on Windows
    // this call is the one that replaces an existing file.
    std::fs::rename(&temp_path, &final_path).map_err(|error| {
        let _ = std::fs::remove_file(&temp_path);
        SshError::io("Could not save settings", error)
    })
}

#[cfg(unix)]
fn write_owner_only(path: &Path, text: &str) -> SshResult<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;

    // `.mode()` applies at creation, so the file is never briefly readable.
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .map_err(|error| SshError::io("Could not save settings", error))?;

    file.write_all(text.as_bytes())
        .map_err(|error| SshError::io("Could not save settings", error))
}

#[cfg(not(unix))]
fn write_owner_only(path: &Path, text: &str) -> SshResult<()> {
    // See the module note: `%APPDATA%` is already per-user.
    std::fs::write(path, text).map_err(|error| SshError::io("Could not save settings", error))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "parolassh-private-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn writes_and_replaces_content() {
        let dir = scratch("replace");

        write(&dir, "thing.json", "{\"a\":1}").unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("thing.json")).unwrap(),
            "{\"a\":1}"
        );

        write(&dir, "thing.json", "{\"a\":2}").unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("thing.json")).unwrap(),
            "{\"a\":2}"
        );

        // The temporary file must not be left behind.
        assert!(!dir.join("thing.json.tmp").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn file_and_directory_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let dir = scratch("modes");
        write(&dir, "hosts.json", "{}").unwrap();

        let file_mode = std::fs::metadata(dir.join("hosts.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(file_mode, 0o600, "the host list must not be group/world readable");

        let dir_mode = std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(dir_mode, 0o700, "the settings directory must be owner-only");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn rewriting_repairs_a_world_readable_file() {
        use std::os::unix::fs::PermissionsExt;

        let dir = scratch("repair");
        std::fs::create_dir_all(&dir).unwrap();

        // What an older version of the app left behind.
        let path = dir.join("hosts.json");
        std::fs::write(&path, "{}").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

        write(&dir, "hosts.json", "{\"hosts\":[]}").unwrap();

        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "an existing loose file must be tightened, not kept");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
