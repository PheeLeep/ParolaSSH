//! Browsing this machine's filesystem, for the local side of the Files tab.
//!
//! Entries reuse the remote shapes so one table renders both sides. Links are
//! followed here - it is the user's own disk - except by `walk`, which skips
//! linked folders so a cycle cannot recurse forever.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::Serialize;

use crate::remote::sftp::{EntryKind, RemoteEntry, TreeFile, TreeListing, MAX_ENTRIES, MAX_TREE_FILES};
use crate::ssh::{SshError, SshResult};

/// How deep a folder upload will walk.
const MAX_TREE_DEPTH: usize = 40;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalListing {
    /// Native form: `/home/me` or `C:\Users\me`. Empty is the drive list on Windows.
    pub path: String,
    /// `None` at the top.
    pub parent: Option<String>,
    pub entries: Vec<RemoteEntry>,
    pub truncated: bool,
}

/// Where the local pane opens.
#[tauri::command]
pub fn local_home_dir() -> SshResult<String> {
    dirs::home_dir()
        .map(|home| home.to_string_lossy().to_string())
        .ok_or_else(|| SshError::invalid("Could not find your home folder."))
}

/// List one local folder. On Windows an empty path lists the drives.
#[tauri::command]
pub fn list_local_dir(path: String) -> SshResult<LocalListing> {
    if path.is_empty() {
        return Ok(top_level());
    }

    let dir = PathBuf::from(&path);
    let reader = std::fs::read_dir(&dir)
        .map_err(|error| SshError::io(&format!("Could not open {path}"), error))?;

    let mut entries = Vec::new();
    let mut truncated = false;
    for item in reader.flatten() {
        if entries.len() >= MAX_ENTRIES {
            truncated = true;
            break;
        }
        entries.push(describe(&item.path(), item.file_name().to_string_lossy().to_string()));
    }

    entries.sort_by(|a, b| {
        let rank = |kind: EntryKind| (kind != EntryKind::Dir) as u8;
        rank(a.kind)
            .cmp(&rank(b.kind))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    Ok(LocalListing {
        path: dir.to_string_lossy().to_string(),
        parent: parent_of(&dir),
        entries,
        truncated,
    })
}

/// Every regular file under a local folder, for a folder upload.
#[tauri::command]
pub fn list_local_tree(path: String) -> SshResult<TreeListing> {
    let root = PathBuf::from(&path);
    let mut listing = TreeListing::default();
    let mut queue = vec![(root.clone(), 0_usize)];

    while let Some((dir, depth)) = queue.pop() {
        if depth > MAX_TREE_DEPTH {
            listing.skipped.push(format!("{} (too deeply nested)", dir.display()));
            continue;
        }
        let Ok(reader) = std::fs::read_dir(&dir) else {
            listing.skipped.push(format!("{} (unreadable)", dir.display()));
            continue;
        };

        for item in reader.flatten() {
            let child = item.path();
            let Ok(link) = std::fs::symlink_metadata(&child) else { continue };
            if link.is_dir() {
                queue.push((child, depth + 1));
                continue;
            }
            // A linked file is uploaded as its contents; a linked folder is not
            // walked, which is what keeps a cycle out.
            match std::fs::metadata(&child) {
                Ok(meta) if meta.is_file() => {
                    if listing.files.len() >= MAX_TREE_FILES {
                        listing.truncated = true;
                        return Ok(listing);
                    }
                    listing.files.push(TreeFile {
                        path: child.to_string_lossy().to_string(),
                        relative: relative_slashed(&root, &child),
                        size: meta.len(),
                    });
                }
                _ => listing.skipped.push(child.to_string_lossy().to_string()),
            }
        }
    }

    listing.files.sort_by(|a, b| a.relative.cmp(&b.relative));
    Ok(listing)
}

fn describe(path: &Path, name: String) -> RemoteEntry {
    let link = std::fs::symlink_metadata(path).ok();
    let is_link = link.as_ref().is_some_and(|meta| meta.file_type().is_symlink());
    // Follow links for the kind; a dangling one is `Other`.
    let meta = std::fs::metadata(path).ok().or(if is_link { None } else { link });

    let kind = match &meta {
        Some(meta) if meta.is_dir() => EntryKind::Dir,
        Some(meta) if meta.is_file() => EntryKind::File,
        _ => EntryKind::Other,
    };

    RemoteEntry {
        name,
        path: path.to_string_lossy().to_string(),
        kind,
        size: meta.as_ref().map(|meta| meta.len()).unwrap_or(0),
        modified: meta
            .as_ref()
            .and_then(|meta| meta.modified().ok())
            .and_then(|time| time.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|since| since.as_secs()),
        mode: meta.as_ref().and_then(mode_bits),
        target: if is_link {
            std::fs::read_link(path).ok().map(|target| target.to_string_lossy().to_string())
        } else {
            None
        },
    }
}

#[cfg(unix)]
fn mode_bits(meta: &std::fs::Metadata) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;
    Some(meta.permissions().mode() & 0o7777)
}

#[cfg(not(unix))]
fn mode_bits(_: &std::fs::Metadata) -> Option<u32> {
    None
}

/// The parent folder, or the drive list (`""`) above a Windows drive root.
fn parent_of(dir: &Path) -> Option<String> {
    match dir.parent() {
        Some(parent) => Some(parent.to_string_lossy().to_string()),
        None if cfg!(windows) => Some(String::new()),
        None => None,
    }
}

/// The root on Unix; every mounted drive letter on Windows.
fn top_level() -> LocalListing {
    if cfg!(windows) {
        let entries = (b'A'..=b'Z')
            .map(|letter| format!("{}:\\", letter as char))
            .filter(|root| Path::new(root).exists())
            .map(|root| RemoteEntry {
                name: root.trim_end_matches('\\').to_string(),
                path: root,
                kind: EntryKind::Dir,
                size: 0,
                modified: None,
                mode: None,
                target: None,
            })
            .collect();
        return LocalListing {
            path: String::new(),
            parent: None,
            entries,
            truncated: false,
        };
    }
    list_local_dir("/".to_string()).unwrap_or(LocalListing {
        path: "/".to_string(),
        parent: None,
        entries: Vec::new(),
        truncated: false,
    })
}

/// `child` under `root`, `/`-separated whatever the OS, so it can be joined
/// onto a remote folder.
fn relative_slashed(root: &Path, child: &Path) -> String {
    child
        .strip_prefix(root)
        .unwrap_or(child)
        .components()
        .map(|part| part.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_lists_directories_first() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("b.txt"), "x").unwrap();
        std::fs::create_dir(dir.path().join("z-folder")).unwrap();

        let listing = list_local_dir(dir.path().to_string_lossy().to_string()).unwrap();
        let names: Vec<&str> = listing.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["z-folder", "b.txt"]);
        assert_eq!(listing.entries[1].size, 1);
        assert!(listing.parent.is_some());
    }

    #[test]
    fn a_tree_keeps_relative_paths_with_forward_slashes() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("app/conf")).unwrap();
        std::fs::write(dir.path().join("app/conf/nginx.conf"), "x").unwrap();
        std::fs::write(dir.path().join("app/README"), "x").unwrap();

        let tree = list_local_tree(dir.path().join("app").to_string_lossy().to_string()).unwrap();
        let relatives: Vec<&str> = tree.files.iter().map(|f| f.relative.as_str()).collect();
        assert_eq!(relatives, ["README", "conf/nginx.conf"]);
    }

    #[cfg(unix)]
    #[test]
    fn a_linked_folder_is_not_walked() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("app")).unwrap();
        std::fs::write(dir.path().join("app/a.txt"), "x").unwrap();
        // A link back to its own parent would recurse forever if followed.
        std::os::unix::fs::symlink(dir.path().join("app"), dir.path().join("app/loop")).unwrap();

        let tree = list_local_tree(dir.path().join("app").to_string_lossy().to_string()).unwrap();
        assert_eq!(tree.files.len(), 1);
        assert_eq!(tree.skipped.len(), 1);
    }
}
