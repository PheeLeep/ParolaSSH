//! The app's own log.
//!
//! A desktop SSH client is used to explain yesterday's failure, so the log is
//! a file, not a session buffer. It lives in the per-OS app log directory that
//! Tauri resolves, and is written owner-only for the same reason `hosts.json`
//! is: it names every machine you administer.
//!
//! Two rules hold everywhere in this module and at every call site:
//!
//! * **No secrets.** Passwords, passphrases and key material never reach a
//!   call. Credentials are `Zeroizing` at the IPC boundary precisely so they
//!   are not copied around, and a log file is a copy that outlives the process.
//! * **No remote output.** A journal, a directory listing or a command's stdout
//!   contains whatever the remote machine puts there. Log *what was attempted
//!   and how it ended*, never what came back.
//!
//! Failing to log is never an error the user sees: every write is best-effort.

use std::fmt::Write as _;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::hosts::store::now_iso8601;

pub const LOG_FILE_NAME: &str = "parolassh.log";

/// Rotated at this size, keeping one previous generation. Big enough to hold a
/// long session, small enough to paste the tail of into a bug report.
const MAX_LOG_BYTES: u64 = 1_048_576;

/// How much of the file the UI will ever parse, taken from the end.
const MAX_READ_BYTES: u64 = 2 * MAX_LOG_BYTES;

const FIELD_SEPARATOR: char = '\t';

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Debug,
    Info,
    Warn,
    Error,
}

impl Level {
    fn as_str(self) -> &'static str {
        match self {
            Level::Debug => "DEBUG",
            Level::Info => "INFO",
            Level::Warn => "WARN",
            Level::Error => "ERROR",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        match text {
            "DEBUG" => Some(Level::Debug),
            "INFO" => Some(Level::Info),
            "WARN" => Some(Level::Warn),
            "ERROR" => Some(Level::Error),
            _ => None,
        }
    }
}

/// One parsed line, as the Settings pane shows it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub time: String,
    pub level: Level,
    /// Which part of the app spoke: `ssh`, `transfers`, `app`…
    pub target: String,
    pub message: String,
}

/// Where the log is and what state it is in, for the Settings header.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogLocation {
    pub path: String,
    pub exists: bool,
    pub bytes: u64,
}

struct Logger {
    path: PathBuf,
    file: Option<File>,
}

static LOGGER: OnceLock<Mutex<Logger>> = OnceLock::new();

/// Resolve the log path and open the file. Called once, from `setup`.
pub fn init(app: &AppHandle) {
    let Ok(dir) = app.path().app_log_dir() else {
        return;
    };
    let path = dir.join(LOG_FILE_NAME);
    let _ = crate::private_file::create_dir(&dir);

    let logger = Mutex::new(Logger {
        file: open_append(&path),
        path,
    });
    // A second call would mean two handles to one file; keep the first.
    let _ = LOGGER.set(logger);
}

/// The log's path, whether or not anything has been written to it yet.
pub fn location() -> Option<LogLocation> {
    let logger = LOGGER.get()?.lock().ok()?;
    let metadata = std::fs::metadata(&logger.path).ok();

    Some(LogLocation {
        path: logger.path.display().to_string(),
        exists: metadata.is_some(),
        bytes: metadata.map(|m| m.len()).unwrap_or(0),
    })
}

pub fn debug(target: &str, message: impl AsRef<str>) {
    write_line(Level::Debug, target, message.as_ref());
}

pub fn info(target: &str, message: impl AsRef<str>) {
    write_line(Level::Info, target, message.as_ref());
}

pub fn warn(target: &str, message: impl AsRef<str>) {
    write_line(Level::Warn, target, message.as_ref());
}

pub fn error(target: &str, message: impl AsRef<str>) {
    write_line(Level::Error, target, message.as_ref());
}

/// Tab-separated so the file stays readable in a text editor and still parses
/// back without a grammar. Tabs and newlines inside a field would break that,
/// so they are folded to spaces on the way in.
fn write_line(level: Level, target: &str, message: &str) {
    let Some(logger) = LOGGER.get() else {
        return; // before `init`, or the log directory was unavailable
    };
    let Ok(mut logger) = logger.lock() else {
        return; // a poisoned lock must not take the app down with it
    };

    let mut line = String::with_capacity(message.len() + 48);
    let _ = write!(
        line,
        "{}\t{}\t{}\t{}\n",
        now_iso8601(),
        level.as_str(),
        sanitize(target),
        sanitize(message),
    );

    rotate_if_needed(&mut logger);

    if logger.file.is_none() {
        logger.file = open_append(&logger.path);
    }
    if let Some(file) = logger.file.as_mut() {
        // A failed write means a full or read-only disk. Drop the handle so the
        // next call retries from scratch rather than writing into a dead file.
        if file.write_all(line.as_bytes()).is_err() {
            logger.file = None;
        }
    }
}

fn sanitize(text: &str) -> String {
    text.replace(['\t', '\n', '\r'], " ")
}

fn rotate_if_needed(logger: &mut Logger) {
    let too_big = std::fs::metadata(&logger.path)
        .map(|metadata| metadata.len() >= MAX_LOG_BYTES)
        .unwrap_or(false);
    if !too_big {
        return;
    }

    // Drop the handle first: on Windows the rename fails while the file is
    // open, and on Unix the old handle would keep writing to the rotated inode.
    logger.file = None;
    let previous = logger.path.with_extension("log.1");
    let _ = std::fs::remove_file(&previous);
    let _ = std::fs::rename(&logger.path, &previous);
    logger.file = open_append(&logger.path);
}

/// Owner-only at creation on Unix, for the same reason as `hosts.json`. Windows
/// inherits `%APPDATA%`, which is already per-user.
fn open_append(path: &Path) -> Option<File> {
    let mut options = std::fs::OpenOptions::new();
    options.append(true).create(true);

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    options.open(path).ok()
}

/// The last `max_lines` entries, newest last. Unparseable lines are dropped
/// rather than shown raw — a half-written final line is normal while the app
/// is running.
pub fn read_entries(max_lines: usize) -> Vec<LogEntry> {
    let Some(logger) = LOGGER.get() else {
        return Vec::new();
    };
    let Ok(logger) = logger.lock() else {
        return Vec::new();
    };

    let Ok(text) = read_tail(&logger.path, MAX_READ_BYTES) else {
        return Vec::new();
    };

    let mut entries: Vec<LogEntry> = text.lines().filter_map(parse_line).collect();
    if entries.len() > max_lines {
        entries.drain(..entries.len() - max_lines);
    }
    entries
}

/// Truncate the log. The handle is reopened, so logging continues afterwards.
pub fn clear() {
    let Some(logger) = LOGGER.get() else {
        return;
    };
    let Ok(mut logger) = logger.lock() else {
        return;
    };

    logger.file = None;
    let _ = std::fs::remove_file(&logger.path);
    let _ = std::fs::remove_file(logger.path.with_extension("log.1"));
    logger.file = open_append(&logger.path);
}

/// Read at most `limit` bytes from the end of the file. The first line of the
/// result may be cut mid-way, so it is dropped when the file was truncated.
fn read_tail(path: &Path, limit: u64) -> std::io::Result<String> {
    use std::io::{Read, Seek, SeekFrom};

    let mut file = File::open(path)?;
    let length = file.metadata()?.len();
    let truncated = length > limit;
    if truncated {
        file.seek(SeekFrom::Start(length - limit))?;
    }

    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    // The file is ASCII timestamps plus whatever we wrote; be forgiving.
    let mut text = String::from_utf8_lossy(&bytes).into_owned();

    if truncated {
        if let Some(newline) = text.find('\n') {
            text = text.split_off(newline + 1);
        }
    }
    Ok(text)
}

fn parse_line(line: &str) -> Option<LogEntry> {
    let mut fields = line.splitn(4, FIELD_SEPARATOR);
    let time = fields.next()?;
    let level = Level::parse(fields.next()?)?;
    let target = fields.next()?;
    let message = fields.next()?;

    Some(LogEntry {
        time: time.to_string(),
        level,
        target: target.to_string(),
        message: message.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_written_line() {
        let entry = parse_line("2026-07-27T10:00:00Z\tWARN\tssh\tHost key changed").unwrap();
        assert_eq!(entry.level, Level::Warn);
        assert_eq!(entry.target, "ssh");
        assert_eq!(entry.message, "Host key changed");
    }

    #[test]
    fn a_message_keeps_its_own_tabs_as_one_field() {
        // Only the first three separators are structural; `splitn(4)` means a
        // message that somehow contains one is not silently truncated.
        let entry = parse_line("t\tINFO\tapp\ta\tb").unwrap();
        assert_eq!(entry.message, "a\tb");
    }

    #[test]
    fn rejects_a_line_with_an_unknown_level() {
        assert!(parse_line("t\tTRACE\tapp\thello").is_none());
        assert!(parse_line("not a log line").is_none());
    }

    #[test]
    fn sanitize_folds_the_separators() {
        assert_eq!(sanitize("a\tb\nc\rd"), "a b c d");
    }
}
