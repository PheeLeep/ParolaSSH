//! Running one transfer.
//!
//! The queue in `transfers` decides *what* runs; this decides *how*. Each
//! transfer opens its own SFTP channel so a slow copy never blocks the file
//! browser, and copies in fixed chunks - a four-gigabyte download must never
//! become a four-gigabyte allocation.
//!
//! Two properties matter more than throughput:
//!
//!   * **A partial file never looks finished.** Downloads land in `{name}.part`
//!     and are renamed only after the last byte, so a cancel, a dropped link, or
//!     a crash leaves something obviously incomplete instead of a truncated file
//!     with the right name.
//!   * **Cancellation is prompt but clean.** The flag is checked at every chunk
//!     boundary rather than by dropping the task, so the `.part` is removed and
//!     the handle closed on the way out.
//!
//! Transfers are length-checked, never hashed. SSH already MACs every packet,
//! so corruption in flight breaks the session rather than reaching us; the
//! residual risk is truncation, which a byte count catches exactly. A content
//! hash would need the *server* to compute one - an exec and a second full read
//! of the file - to cover only corruption at rest.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::client::Session;
use super::registry::LiveSession;
use super::sftp;
use super::transfers::{Direction, TransferManager, TransferState};
use crate::private_file;
use crate::ssh::{SshError, SshResult};

/// Emitted as bytes move.
pub const PROGRESS_EVENT: &str = "sftp://progress";
/// Emitted when the queue's shape changes - enqueue, promote, settle, re-rank.
pub const CHANGED_EVENT: &str = "sftp://changed";

/// Chunk size. Large enough that per-request overhead disappears, small enough
/// that a cancel is noticed promptly and memory stays flat.
const CHUNK: usize = 256 * 1024;

/// Floor on how often progress reaches the webview.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(120);

/// The suffix a download wears until it is complete.
const PART_SUFFIX: &str = ".part";

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProgressEvent {
    transfer_id: u64,
    host_id: String,
    bytes_done: u64,
    bytes_total: Option<u64>,
    state: TransferState,
}

/// Tell the whole app the queue changed.
///
/// Broadcast rather than addressed to one webview, which is the opposite of
/// `shell.rs`: terminal output belongs to the pane that asked for it, but the
/// transfer list is global - the Transfers page must keep updating while the
/// user is somewhere else entirely.
pub fn emit_changed(app: &AppHandle) {
    let _ = app.emit(CHANGED_EVENT, ());
}

fn emit_progress(app: &AppHandle, id: u64, host_id: &str, done: u64, total: Option<u64>) {
    let _ = app.emit(
        PROGRESS_EVENT,
        ProgressEvent {
            transfer_id: id,
            host_id: host_id.to_string(),
            bytes_done: done,
            bytes_total: total,
            state: TransferState::Running,
        },
    );
}

/// `(bytes_done, bytes_total)`. Taken instead of an `AppHandle` so the copy
/// loops know nothing about webviews, and tests can drive them without Tauri.
pub type ProgressSink<'a> = &'a (dyn Fn(u64, Option<u64>) + Send + Sync);

/// Run one transfer to completion, recording the outcome on the manager.
///
/// Never returns an error: the outcome belongs on the record, where the UI can
/// show it, not in a log nobody reads.
#[allow(clippy::too_many_arguments)]
pub async fn run(
    app: AppHandle,
    manager: Arc<TransferManager>,
    live: Arc<LiveSession>,
    id: u64,
    host_id: String,
    direction: Direction,
    remote_path: String,
    local_path: String,
    cancel: Arc<AtomicBool>,
) {
    // Record every update, emit on a timer: per-chunk events would cost more
    // to render than the copy takes.
    let last_emit = std::sync::Mutex::new(Instant::now() - PROGRESS_INTERVAL);
    let progress = |done: u64, total: Option<u64>| {
        manager.record_progress(id, done, total);

        let due = last_emit
            .lock()
            .map(|mut at| {
                if at.elapsed() >= PROGRESS_INTERVAL {
                    *at = Instant::now();
                    true
                } else {
                    false
                }
            })
            .unwrap_or(false);

        if due {
            emit_progress(&app, id, &host_id, done, total);
        }
    };

    let session = &live.session;
    let outcome = match direction {
        Direction::Download => {
            download(session, &remote_path, &local_path, &cancel, &progress).await
        }
        Direction::Upload => upload(session, &remote_path, &local_path, &cancel, &progress).await,
    };

    // One last emit so the bar always lands on its true final value, however
    // the throttle happened to fall.
    if let Some(record) = manager.get(id) {
        emit_progress(&app, id, &host_id, record.bytes_done, record.bytes_total);
    }

    manager.finish(id, outcome.err().map(|error| error.to_string()));
    emit_changed(&app);
}

/// Copy a remote file down.
///
/// Reads are pipelined - see `sftp::RemoteReader`. A serial reader spends most
/// of a LAN transfer waiting on round trips rather than moving bytes.
pub async fn download(
    session: &Session,
    remote_path: &str,
    local_path: &str,
    cancel: &AtomicBool,
    progress: ProgressSink<'_>,
) -> SshResult<()> {
    let final_path = PathBuf::from(local_path);
    let part_path = part_path_for(&final_path);

    // Every failure below must leave nothing behind, so the cleanup lives in
    // one place rather than on each `?`.
    let outcome = download_into(session, remote_path, &part_path, cancel, progress).await;
    if outcome.is_err() {
        let _ = std::fs::remove_file(&part_path);
        return outcome.map(|_| ());
    }

    // Only now does the file get its real name.
    std::fs::rename(&part_path, &final_path).map_err(|error| {
        let _ = std::fs::remove_file(&part_path);
        SshError::io("Could not put the downloaded file in place", error)
    })?;

    Ok(())
}

/// Fetch into the staging file. The caller owns the rename and the cleanup.
async fn download_into(
    session: &Session,
    remote_path: &str,
    part_path: &Path,
    cancel: &AtomicBool,
    progress: ProgressSink<'_>,
) -> SshResult<()> {
    // Opening also re-checks the kind on the handle we are about to read,
    // rather than trusting a listing that may be minutes old.
    let reader = sftp::RemoteReader::open(session, remote_path).await?;
    let total = reader.len;
    progress(0, Some(total));

    if let Some(parent) = part_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| SshError::io("Could not create the download folder", error))?;
    }

    // Owner-only from the moment it exists - a downloaded private key is never
    // world-readable, not even for the seconds it is arriving.
    let file = private_file::create_owner_only(part_path)?;
    let mut sink = tokio::io::BufWriter::with_capacity(CHUNK, tokio::fs::File::from_std(file));

    let done = reader
        .copy_to(&mut sink, cancel, &|done| progress(done, Some(total)))
        .await?;

    sink.flush()
        .await
        .map_err(|error| SshError::io("Could not finish writing the file", error))?;
    drop(sink);

    // Without this a short read would be renamed and look complete.
    verify_length(done, total, None)
}

/// Refuse a transfer whose byte count does not match the size promised.
/// `stray` is a partial file to remove on failure.
fn verify_length(done: u64, expected: u64, stray: Option<&Path>) -> SshResult<()> {
    if done == expected {
        return Ok(());
    }

    if let Some(path) = stray {
        let _ = std::fs::remove_file(path);
    }

    Err(SshError::Io(format!(
        "The transfer ended early: {done} of {expected} bytes arrived. \
         Nothing was kept - try again."
    )))
}

/// Copy a local file up. No `.part` staging: renaming into place would need a
/// second permission the user may not have.
pub async fn upload(
    session: &Session,
    remote_path: &str,
    local_path: &str,
    cancel: &AtomicBool,
    progress: ProgressSink<'_>,
) -> SshResult<()> {
    let sftp = sftp::connect(session).await?;

    // Refuse to write through a link that already exists at the destination.
    // `symlink_metadata` does not resolve the final component, so this sees the
    // link itself rather than whatever it points at.
    if let Ok(existing) = sftp.symlink_metadata(remote_path.to_string()).await {
        let kind = sftp::EntryKind::from(existing.file_type());
        if let Some(refusal) = sftp::refuse_unless_regular(kind, remote_path) {
            return Err(refusal);
        }
    }

    let mut source = tokio::fs::File::open(local_path)
        .await
        .map_err(|error| SshError::io("Could not read the file you chose", error))?;
    let total = source
        .metadata()
        .await
        .map(|meta| meta.len())
        .unwrap_or(0);
    progress(0, Some(total));

    let mut sink = sftp
        .create(remote_path.to_string())
        .await
        .map_err(|error| sftp::explain_error(&format!("Could not create {remote_path}"), &error.to_string()))?;

    let mut buffer = vec![0_u8; CHUNK];
    let mut done: u64 = 0;

    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(SshError::invalid("The transfer was cancelled."));
        }

        let read = source
            .read(&mut buffer)
            .await
            .map_err(|error| SshError::io("Could not read the file you chose", error))?;
        if read == 0 {
            break;
        }

        sink.write_all(&buffer[..read]).await.map_err(|error| {
            sftp::explain_error(&format!("Could not write {remote_path}"), &error.to_string())
        })?;

        done += read as u64;
        progress(done, Some(total));
    }

    sink.flush().await.map_err(|error| {
        sftp::explain_error(&format!("Could not finish writing {remote_path}"), &error.to_string())
    })?;
    sink.shutdown().await.map_err(|error| {
        sftp::explain_error(&format!("Could not close {remote_path}"), &error.to_string())
    })?;

    // Should be unreachable - we read the local file ourselves - so a short
    // count means our own loop lost bytes.
    verify_length(done, total, None)?;
    Ok(())
}

/// Where a download lives while it is still arriving.
pub fn part_path_for(final_path: &Path) -> PathBuf {
    let mut name = final_path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "download".to_string());
    name.push_str(PART_SUFFIX);
    final_path.with_file_name(name)
}

/// `notes.txt`, then `notes (1).txt`. Downloading twice is common enough, and
/// silently replacing the first copy destructive enough, to not ask.
pub fn available_path(dir: &Path, file_name: &str) -> PathBuf {
    let candidate = dir.join(file_name);
    if !candidate.exists() && !part_path_for(&candidate).exists() {
        return candidate;
    }

    let path = Path::new(file_name);
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| file_name.to_string());
    let extension = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();

    for index in 1..1000 {
        let candidate = dir.join(format!("{stem} ({index}){extension}"));
        if !candidate.exists() && !part_path_for(&candidate).exists() {
            return candidate;
        }
    }

    // A thousand copies of one name is not a case worth more code; the caller
    // gets the plain path and the create fails loudly if it truly collides.
    dir.join(file_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_full_length_transfer_is_accepted() {
        assert!(verify_length(1024, 1024, None).is_ok());
        assert!(verify_length(0, 0, None).is_ok(), "an empty file is still a file");
    }

    #[test]
    fn a_short_transfer_is_refused_and_its_partial_file_removed() {
        let dir = tempfile::tempdir().unwrap();
        let part = dir.path().join("backup.tar.gz.part");
        std::fs::write(&part, "half of it").unwrap();

        let error = verify_length(512, 1024, Some(&part)).unwrap_err();
        let text = error.to_string();
        assert!(text.contains("512 of 1024"), "the message should show both counts: {text}");
        assert!(text.contains("Nothing was kept"));
        assert!(!part.exists(), "a truncated download must not be left behind");
    }

    #[test]
    fn a_long_transfer_is_refused_too() {
        // A file that grew under us is as untrustworthy as one that was cut
        // short - the bytes on disk no longer match anything we checked.
        assert!(verify_length(2048, 1024, None).is_err());
    }

    #[test]
    fn a_download_in_flight_is_obviously_incomplete() {
        let path = part_path_for(Path::new("/home/me/backup.tar.gz"));
        assert_eq!(path, PathBuf::from("/home/me/backup.tar.gz.part"));
    }

    #[test]
    fn available_path_leaves_an_existing_file_alone() {
        let dir = tempfile::tempdir().unwrap();
        let first = available_path(dir.path(), "notes.txt");
        assert_eq!(first, dir.path().join("notes.txt"));

        std::fs::write(&first, "one").unwrap();
        let second = available_path(dir.path(), "notes.txt");
        assert_eq!(second, dir.path().join("notes (1).txt"));

        std::fs::write(&second, "two").unwrap();
        assert_eq!(
            available_path(dir.path(), "notes.txt"),
            dir.path().join("notes (2).txt")
        );

        // The original is untouched.
        assert_eq!(std::fs::read_to_string(&first).unwrap(), "one");
    }

    #[test]
    fn a_transfer_already_in_flight_does_not_get_reused_as_a_name() {
        let dir = tempfile::tempdir().unwrap();
        // Nothing at `notes.txt`, but a download of it is already arriving.
        std::fs::write(dir.path().join("notes.txt.part"), "partial").unwrap();

        assert_eq!(
            available_path(dir.path(), "notes.txt"),
            dir.path().join("notes (1).txt"),
            "two concurrent downloads must not share one .part file"
        );
    }

    #[test]
    fn a_dotfile_keeps_its_whole_name_when_disambiguated() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".bashrc"), "x").unwrap();
        // `.bashrc` is all stem, no extension - the suffix must not eat it.
        assert_eq!(
            available_path(dir.path(), ".bashrc"),
            dir.path().join(".bashrc (1)")
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_downloaded_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("id_ed25519.part");
        let file = private_file::create_owner_only(&path).unwrap();
        drop(file);

        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "a downloaded key must not be readable by others");
    }
}
