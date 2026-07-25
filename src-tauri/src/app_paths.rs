//! Where the app keeps its own files.
//!
//! Tauri resolves this per platform — `%APPDATA%` on Windows,
//! `~/Library/Application Support` on macOS, `~/.config` on Linux — so nothing
//! else in the codebase has to know the difference.

use std::path::PathBuf;

use tauri::{AppHandle, Manager};

use crate::ssh::{SshError, SshResult};

pub fn config_dir(app: &AppHandle) -> SshResult<PathBuf> {
    app.path()
        .app_config_dir()
        .map_err(|error| SshError::Io(format!("Could not locate the settings directory: {error}")))
}
