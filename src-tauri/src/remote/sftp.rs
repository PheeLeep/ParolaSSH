//! Browsing a remote filesystem over the SFTP subsystem.
//!
//! Rides the authenticated connection like `shell` and `stream` do: a session
//! channel, `subsystem sftp`, and the channel's byte stream handed to
//! `russh_sftp`. Nothing here goes through `Session::exec`, whose thirty-second
//! cap would kill a large transfer.
//!
//! Two rules shape this module, both about not trusting the far end:
//!
//!   * **Symlinks are never followed.** They are listed, with their target, and
//!     every operation on one is refused. A link is the cheapest way for a host
//!     to make us read `/dev/zero` forever or write outside the directory the
//!     user picked, and resolving one safely means a containment check racing
//!     the server's own filesystem. Refusing costs a feature nobody has asked
//!     for and removes the whole class.
//!   * **Paths are normalized here, before the wire.** The server's idea of
//!     `..` is not ours to rely on.
//!
//! SFTP authenticates as the login user and has no sudo: there is no subsystem
//! equivalent of `power.rs`'s elevation. A permission denial is final, and
//! `explain_error` says so rather than leaving the user to wonder.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::SystemTime;

use russh_sftp::client::{RawSftpSession, SftpSession};
use russh_sftp::protocol::{FileAttributes, FileType, OpenFlags, StatusCode};
use serde::Serialize;

use super::client::Session;
use crate::ssh::{SshError, SshResult};

/// Ceiling on one directory listing. A directory with a million entries would
/// otherwise build a million-element JSON payload and lock the webview solid;
/// the pane says the listing was truncated instead.
pub const MAX_ENTRIES: usize = 5_000;

/// What one entry in a remote directory is.
///
/// `Symlink` and `Other` (FIFO, socket, device) both exist to be refused —
/// keeping them as distinct kinds lets the UI explain *why* a row is inert.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryKind {
    Dir,
    File,
    Symlink,
    Other,
}

impl From<FileType> for EntryKind {
    fn from(kind: FileType) -> Self {
        match kind {
            FileType::Dir => Self::Dir,
            FileType::File => Self::File,
            FileType::Symlink => Self::Symlink,
            FileType::Other => Self::Other,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteEntry {
    pub name: String,
    pub path: String,
    pub kind: EntryKind,
    pub size: u64,
    /// Unix seconds, or `None` when the server sent no mtime.
    pub modified: Option<u64>,
    /// Permission bits, or `None` on a server that reports no mode (Windows).
    pub mode: Option<u32>,
    /// Where a symlink points, for display only — we never follow it.
    pub target: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirListing {
    pub path: String,
    pub entries: Vec<RemoteEntry>,
    /// True when the directory held more than `MAX_ENTRIES`, so the pane can
    /// say so rather than quietly showing a prefix.
    pub truncated: bool,
}

/// Open an SFTP session on its own channel.
///
/// Every caller gets its own: browsing must stay responsive while a transfer
/// saturates the link, and the subsystem serialises requests per channel.
pub async fn connect(session: &Session) -> SshResult<SftpSession> {
    let channel = session.open_channel().await?;
    channel
        .request_subsystem(true, "sftp")
        .await
        .map_err(|error| {
            SshError::Io(format!(
                "This server would not start its SFTP subsystem: {error}. \
                 Check that sshd has a Subsystem sftp line."
            ))
        })?;

    SftpSession::new(channel.into_stream())
        .await
        .map_err(|error| SshError::Io(format!("Could not start an SFTP session: {error}")))
}

/// The directory to open a fresh browser in. `canonicalize(".")` is the
/// subsystem's own answer for "where does this user start", which beats
/// guessing `/home/{user}`.
pub async fn home_dir(sftp: &SftpSession) -> SshResult<String> {
    let path = sftp
        .canonicalize(".")
        .await
        .map_err(|error| explain_error("Could not find your home directory", &error.to_string()))?;
    Ok(normalize(&path))
}

/// List one directory.
///
/// The entry kinds come from the server's `readdir` attributes, which follow
/// `lstat` semantics — a symlink is reported as a symlink, not as whatever it
/// points at. `read_link` is then asked only for the links, so a directory of
/// ordinary files costs exactly one round trip.
pub async fn list_dir(sftp: &SftpSession, path: &str) -> SshResult<DirListing> {
    let path = normalize(path);
    let reader = sftp
        .read_dir(path.clone())
        .await
        .map_err(|error| explain_error(&format!("Could not open {path}"), &error.to_string()))?;

    let mut entries = Vec::new();
    let mut truncated = false;

    for item in reader {
        if entries.len() >= MAX_ENTRIES {
            truncated = true;
            break;
        }

        let metadata = item.metadata();
        let kind = EntryKind::from(item.file_type());
        let child = join(&path, &item.file_name());

        let target = if kind == EntryKind::Symlink {
            // Display only. A failed readlink is not an error worth failing the
            // whole listing over — the row still renders, just without a target.
            sftp.read_link(child.clone()).await.ok()
        } else {
            None
        };

        entries.push(RemoteEntry {
            name: item.file_name(),
            path: child,
            kind,
            size: metadata.len(),
            modified: metadata.modified().ok().and_then(to_unix_seconds),
            // The wire mode carries the file-type bits too; the pane only wants
            // the permission triplets.
            mode: metadata.permissions.map(|bits| bits & 0o7777),
            target,
        });
    }

    entries.sort_by(compare_entries);

    Ok(DirListing {
        path,
        entries,
        truncated,
    })
}

/// Confirm a path is a plain file and return its size, immediately before a
/// download opens it.
///
/// The listing already told us the kind, but that answer is as old as the last
/// refresh and the pane may have been sitting open for an hour. Re-checking
/// with `symlink_metadata` — which does not resolve the final component — costs
/// one round trip on an operation that is about to cost thousands, and closes
/// the window where a file is swapped for a link to `/dev/zero` after being
/// listed.
pub async fn stat_regular_file(sftp: &SftpSession, path: &str) -> SshResult<u64> {
    let metadata = sftp
        .symlink_metadata(path.to_string())
        .await
        .map_err(|error| explain_error(&format!("Could not read {path}"), &error.to_string()))?;

    let kind = EntryKind::from(metadata.file_type());
    if let Some(refusal) = refuse_unless_regular(kind, path) {
        return Err(refusal);
    }
    Ok(metadata.len())
}

/// The one place the symlink policy lives.
///
/// Called by descend, download, and upload alike, so the three can never drift
/// apart. `None` means the operation may proceed.
pub fn refuse_unless_regular(kind: EntryKind, path: &str) -> Option<SshError> {
    match kind {
        EntryKind::File | EntryKind::Dir => None,
        EntryKind::Symlink => Some(SshError::invalid(format!(
            "{path} is a symbolic link. ParolaSSH does not follow links — \
             open the file it points at directly."
        ))),
        EntryKind::Other => Some(SshError::invalid(format!(
            "{path} is not a regular file or directory. \
             Device files, sockets and pipes cannot be transferred."
        ))),
    }
}

/// Reduce a remote path to an absolute, `.`- and `..`-free POSIX path.
///
/// Done before anything reaches the wire: `..` is resolved against the string
/// we hold, never against the server's view, so a path cannot mean one thing to
/// the check and another to the open. A `..` that would climb above the root is
/// dropped, matching how a kernel treats `/..`.
pub fn normalize(path: &str) -> String {
    // Windows OpenSSH speaks `/C:/Users/...` on the wire. Keep the leading
    // slash — that *is* the wire form — and let the frontend present it.
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => continue,
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }

    if parts.is_empty() {
        return "/".to_string();
    }
    format!("/{}", parts.join("/"))
}

/// Append one already-known-safe child name to a directory path.
pub fn join(dir: &str, name: &str) -> String {
    normalize(&format!("{dir}/{name}"))
}

/// The parent of a path, or the path itself when it is already the root.
pub fn parent(path: &str) -> String {
    let normalized = normalize(path);
    match normalized.rsplit_once('/') {
        Some((_, _)) if normalized == "/" => "/".to_string(),
        Some(("", _)) => "/".to_string(),
        Some((head, _)) => head.to_string(),
        None => "/".to_string(),
    }
}

/// Reduce a remote file name to something safe to create locally.
///
/// The server chose this string. Anything that could steer the write out of the
/// directory the user picked — separators, `..`, a drive letter, a NUL — is
/// refused rather than rewritten, because a silently renamed download is worse
/// than a refused one.
pub fn safe_local_name(name: &str) -> SshResult<String> {
    let refuse = |reason: &str| {
        Err(SshError::invalid(format!(
            "The server offered a file name that cannot be saved safely ({reason}): {name}"
        )))
    };

    if name.is_empty() || name == "." || name == ".." {
        return refuse("empty or a directory reference");
    }
    if name.contains('/') || name.contains('\\') {
        return refuse("it contains a path separator");
    }
    if name.contains('\0') {
        return refuse("it contains a null byte");
    }
    // `C:` as a bare name is a drive-relative path on Windows.
    if name.contains(':') {
        return refuse("it contains a colon");
    }
    if name.chars().any(|c| c.is_control()) {
        return refuse("it contains control characters");
    }

    Ok(name.to_string())
}

/// Directories first, then names, case-insensitively — the ordering every file
/// browser has, so nobody has to think about it.
fn compare_entries(a: &RemoteEntry, b: &RemoteEntry) -> std::cmp::Ordering {
    let rank = |kind: EntryKind| match kind {
        EntryKind::Dir => 0,
        _ => 1,
    };
    rank(a.kind)
        .cmp(&rank(b.kind))
        .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        .then_with(|| a.name.cmp(&b.name))
}

fn to_unix_seconds(time: SystemTime) -> Option<u64> {
    time.duration_since(SystemTime::UNIX_EPOCH)
        .ok()
        .map(|since| since.as_secs())
}

/// Turn a protocol error into something a person can act on.
///
/// Permission denied is the one worth special-casing: the natural next thought
/// is "run it with sudo", and over SFTP that option does not exist. Saying so
/// here saves the user hunting for an elevation button that cannot be built.
pub fn explain_error(context: &str, error: &str) -> SshError {
    let lowered = error.to_lowercase();

    if lowered.contains("permission denied") || lowered.contains("permissiondenied") {
        return SshError::invalid(format!(
            "{context}: permission denied. SFTP runs as the user you signed in as \
             and cannot elevate — reconnect as a user with access to this path."
        ));
    }
    if lowered.contains("no such file") || lowered.contains("nosuchfile") {
        return SshError::invalid(format!("{context}: no such file or directory."));
    }
    SshError::Io(format!("{context}: {error}"))
}

/* ── Reading a file fast ───────────────────────────────────────────────── */

/// Read requests kept in flight at once.
///
/// SFTP is request/response, so a serial reader idles for a round trip per
/// chunk. `russh_sftp`'s `File` pipelines writes eight deep but reads one at a
/// time — why uploads ran 3x faster than downloads until this existed.
/// Bounds memory at `DEPTH x read_len`, about 4 MiB.
const READ_PIPELINE_DEPTH: usize = 16;

/// A remote file open for reading, several requests deep.
pub struct RemoteReader {
    raw: RawSftpSession,
    handle: String,
    /// The size the server reported when we opened it.
    pub len: u64,
    /// The largest read the server will answer in one packet.
    chunk: usize,
}

impl RemoteReader {
    /// Open `path`, refusing anything that is not a regular file. Checked here
    /// rather than trusted from a listing that may be minutes old.
    pub async fn open(session: &Session, path: &str) -> SshResult<Self> {
        let channel = session.open_channel().await?;
        channel
            .request_subsystem(true, "sftp")
            .await
            .map_err(|error| {
                SshError::Io(format!(
                    "This server would not start its SFTP subsystem: {error}. \
                     Check that sshd has a Subsystem sftp line."
                ))
            })?;

        let mut raw = RawSftpSession::new(channel.into_stream());
        raw.init()
            .await
            .map_err(|error| SshError::Io(format!("Could not start an SFTP session: {error}")))?;

        // `limits@openssh.com` where offered, so we never ask for a packet the
        // server will refuse.
        let chunk = match raw.limits().await {
            Ok(limits) => {
                let read_len = limits.max_read_len;
                raw.set_limits(limits.into());
                if read_len > 0 {
                    read_len as usize
                } else {
                    DEFAULT_READ_LEN
                }
            }
            Err(_) => DEFAULT_READ_LEN,
        };

        let attrs = raw
            .lstat(path.to_string())
            .await
            .map_err(|error| explain_error(&format!("Could not read {path}"), &error.to_string()))?;

        let kind = EntryKind::from(attrs.attrs.file_type());
        if let Some(refusal) = refuse_unless_regular(kind, path) {
            return Err(refusal);
        }
        let len = attrs.attrs.len();

        let handle = raw
            .open(path.to_string(), OpenFlags::READ, FileAttributes::empty())
            .await
            .map_err(|error| explain_error(&format!("Could not open {path}"), &error.to_string()))?
            .handle;

        Ok(Self {
            raw,
            handle,
            len,
            chunk,
        })
    }

    /// Copy the file into `out`, several reads in flight. `FuturesOrdered`
    /// yields replies in issue order, so output stays sequential even though
    /// the requests are not. Returns bytes written, for the caller's length
    /// check.
    pub async fn copy_to<W>(
        &self,
        out: &mut W,
        cancel: &AtomicBool,
        progress: &(dyn Fn(u64) + Send + Sync),
    ) -> SshResult<u64>
    where
        W: tokio::io::AsyncWrite + Unpin,
    {
        use futures_util::stream::{FuturesOrdered, StreamExt};
        use tokio::io::AsyncWriteExt;

        let mut inflight = FuturesOrdered::new();
        let mut next_offset: u64 = 0;
        let mut written: u64 = 0;

        // Tagged with its offset so a short reply can be repaired in place.
        let issue = |offset: u64| {
            let want = std::cmp::min(self.chunk as u64, self.len.saturating_sub(offset)) as u32;
            let raw = &self.raw;
            let handle = self.handle.clone();
            async move { (offset, want, raw.read(handle, offset, want).await) }
        };

        while next_offset < self.len && inflight.len() < READ_PIPELINE_DEPTH {
            inflight.push_back(issue(next_offset));
            next_offset += self.chunk as u64;
        }

        while let Some((offset, want, result)) = inflight.next().await {
            if cancel.load(Ordering::Relaxed) {
                return Err(SshError::invalid("The transfer was cancelled."));
            }

            let mut data = match result {
                Ok(data) => data.data,
                // Shorter than its stat claimed; the length check refuses it.
                Err(error) if is_eof(&error) => break,
                Err(error) => {
                    return Err(explain_error("Could not read from the server", &error.to_string()))
                }
            };

            // A server may legally return less than asked. Fill the gap now, or
            // the next chunk lands at the wrong offset and corrupts the file.
            while data.len() < want as usize {
                let filled = data.len() as u64;
                match self
                    .raw
                    .read(
                        self.handle.clone(),
                        offset + filled,
                        want - filled as u32,
                    )
                    .await
                {
                    Ok(more) if !more.data.is_empty() => data.extend_from_slice(&more.data),
                    Ok(_) => break,
                    Err(error) if is_eof(&error) => break,
                    Err(error) => {
                        return Err(explain_error(
                            "Could not read from the server",
                            &error.to_string(),
                        ))
                    }
                }
            }

            if data.is_empty() {
                break;
            }

            out.write_all(&data)
                .await
                .map_err(|error| SshError::io("Could not write the file", error))?;
            written += data.len() as u64;
            progress(written);

            // Top the pipeline back up as each reply is consumed.
            if next_offset < self.len {
                inflight.push_back(issue(next_offset));
                next_offset += self.chunk as u64;
            }
        }

        Ok(written)
    }
}

impl Drop for RemoteReader {
    fn drop(&mut self) {
        // Best effort: no async context here, and the channel closes anyway.
        let _ = self.raw.close_session();
    }
}

/// What `russh_sftp` assumes when the server offers no `limits` extension.
const DEFAULT_READ_LEN: usize = 32 * 1024;

fn is_eof(error: &russh_sftp::client::error::Error) -> bool {
    matches!(
        error,
        russh_sftp::client::error::Error::Status(status)
            if status.status_code == StatusCode::Eof
    )
}

/// Shared browsing session for one host, opened on first use.
///
/// Held behind a tokio mutex because it is used across awaits, and because the
/// subsystem is request/response per channel: two concurrent listings on one
/// channel would interleave. Transfers deliberately do not use this — they open
/// their own channel so a slow copy never blocks the pane.
#[derive(Default)]
pub struct BrowseSession {
    session: tokio::sync::Mutex<Option<Arc<SftpSession>>>,
}

impl BrowseSession {
    /// The open session, opening one if this is the first call.
    pub async fn get_or_open(&self, session: &Session) -> SshResult<Arc<SftpSession>> {
        let mut slot = self.session.lock().await;
        if let Some(existing) = slot.as_ref() {
            return Ok(Arc::clone(existing));
        }
        let opened = Arc::new(connect(session).await?);
        *slot = Some(Arc::clone(&opened));
        Ok(opened)
    }

    /// Drop the cached session, so the next browse reopens. Used when a request
    /// fails in a way that suggests the channel is gone.
    pub async fn reset(&self) {
        let mut slot = self.session.lock().await;
        *slot = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, kind: EntryKind) -> RemoteEntry {
        RemoteEntry {
            name: name.to_string(),
            path: format!("/tmp/{name}"),
            kind,
            size: 0,
            modified: None,
            mode: None,
            target: None,
        }
    }

    #[test]
    fn normalize_makes_paths_absolute_and_clean() {
        assert_eq!(normalize("/var/log/"), "/var/log");
        assert_eq!(normalize("var/log"), "/var/log");
        assert_eq!(normalize("/var/./log"), "/var/log");
        assert_eq!(normalize("/var//log"), "/var/log");
        assert_eq!(normalize(""), "/");
        assert_eq!(normalize("/"), "/");
    }

    #[test]
    fn normalize_resolves_dotdot_without_asking_the_server() {
        assert_eq!(normalize("/var/log/../lib"), "/var/lib");
        assert_eq!(normalize("/var/log/.."), "/var");
        // Climbing above the root stops at the root, as a kernel would.
        assert_eq!(normalize("/../../etc/passwd"), "/etc/passwd");
        assert_eq!(normalize("/var/../../.."), "/");
    }

    #[test]
    fn normalize_keeps_the_windows_wire_form() {
        assert_eq!(normalize("/C:/Users/pheeleep"), "/C:/Users/pheeleep");
        assert_eq!(normalize("/C:/Users/../Windows"), "/C:/Windows");
    }

    #[test]
    fn join_and_parent_agree() {
        assert_eq!(join("/var/log", "syslog"), "/var/log/syslog");
        assert_eq!(join("/", "etc"), "/etc");
        assert_eq!(parent("/var/log/syslog"), "/var/log");
        assert_eq!(parent("/etc"), "/");
        assert_eq!(parent("/"), "/");
    }

    #[test]
    fn a_traversing_child_name_cannot_escape_through_join() {
        // The name still has to clear `safe_local_name` before it reaches disk;
        // this only asserts the remote side stays inside a sane path.
        assert_eq!(join("/var/log", ".."), "/var");
        assert!(safe_local_name("../../etc/passwd").is_err());
    }

    #[test]
    fn only_regular_files_and_directories_are_allowed() {
        assert!(refuse_unless_regular(EntryKind::File, "/tmp/a").is_none());
        assert!(refuse_unless_regular(EntryKind::Dir, "/tmp").is_none());

        let link = refuse_unless_regular(EntryKind::Symlink, "/tmp/latest.log");
        assert!(link.is_some());
        assert!(link.unwrap().to_string().contains("symbolic link"));

        let other = refuse_unless_regular(EntryKind::Other, "/dev/zero");
        assert!(other.is_some());
        assert!(other.unwrap().to_string().contains("regular file"));
    }

    #[test]
    fn hostile_file_names_are_refused_not_rewritten() {
        assert!(safe_local_name("notes.txt").is_ok());
        assert!(safe_local_name(".bashrc").is_ok());

        for hostile in [
            "",
            ".",
            "..",
            "../escape",
            "sub/dir",
            "back\\slash",
            "C:evil",
            "null\0byte",
            "bell\u{7}",
        ] {
            assert!(
                safe_local_name(hostile).is_err(),
                "{hostile:?} should have been refused"
            );
        }
    }

    #[test]
    fn listings_put_directories_first_then_sort_by_name() {
        let mut entries = [
            entry("zebra.txt", EntryKind::File),
            entry("Apple", EntryKind::Dir),
            entry("alpha.txt", EntryKind::File),
            entry("beta", EntryKind::Dir),
        ];
        entries.sort_by(compare_entries);

        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["Apple", "beta", "alpha.txt", "zebra.txt"]);
    }

    #[test]
    fn permission_denied_explains_that_sftp_cannot_elevate() {
        let error = explain_error("Could not read /etc/shadow", "Permission denied");
        let text = error.to_string();
        assert!(text.contains("cannot elevate"));
        assert!(text.contains("reconnect as a user"));
    }

    #[test]
    fn a_missing_file_reads_as_a_plain_message() {
        let error = explain_error("Could not open /nope", "No such file");
        assert!(error.to_string().contains("no such file or directory"));
    }
}
