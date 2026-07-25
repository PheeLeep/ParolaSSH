//! Tauri commands for the saved-connection list.
//!
//! Each one reads the file, changes it, and writes it back. That is slower
//! than caching the list in memory and far easier to reason about: there is
//! exactly one source of truth, and a second window cannot serve stale rows.

use tauri::AppHandle;

use super::model::{HostDraft, HostRecord};
use super::store::HostStore;
use crate::app_paths::config_dir;
use crate::ssh::SshResult;

/// Every saved connection, ordered by group then label.
#[tauri::command]
pub fn list_hosts(app: AppHandle) -> SshResult<Vec<HostRecord>> {
    let mut hosts = HostStore::read(&config_dir(&app)?).hosts;
    hosts.sort_by(|a, b| {
        a.group
            .to_lowercase()
            .cmp(&b.group.to_lowercase())
            .then_with(|| a.label.to_lowercase().cmp(&b.label.to_lowercase()))
    });
    Ok(hosts)
}

/// Add a connection, or update one when the draft carries an id.
#[tauri::command]
pub fn save_host(app: AppHandle, draft: HostDraft) -> SshResult<HostRecord> {
    let config_dir = config_dir(&app)?;
    let mut store = HostStore::read(&config_dir);

    let record = store.upsert(draft.validate()?)?;
    store.write(&config_dir)?;

    Ok(record)
}

/// Remove a connection. Returns the record that was removed so the UI can
/// name it in the confirmation, and offer an undo later if we add one.
#[tauri::command]
pub fn delete_host(app: AppHandle, id: String) -> SshResult<HostRecord> {
    let config_dir = config_dir(&app)?;
    let mut store = HostStore::read(&config_dir);

    let removed = store.remove(&id)?;
    store.write(&config_dir)?;

    Ok(removed)
}

/// Existing group names, for the group picker's suggestions.
#[tauri::command]
pub fn list_host_groups(app: AppHandle) -> SshResult<Vec<String>> {
    Ok(distinct(
        HostStore::read(&config_dir(&app)?)
            .hosts
            .into_iter()
            .map(|host| host.group),
    ))
}

/// Every tag in use, for the tag input's suggestions.
#[tauri::command]
pub fn list_host_tags(app: AppHandle) -> SshResult<Vec<String>> {
    Ok(distinct(
        HostStore::read(&config_dir(&app)?)
            .hosts
            .into_iter()
            .flat_map(|host| host.tags),
    ))
}

/// De-duplicate case-insensitively, then sort for a stable dropdown order.
fn distinct(values: impl Iterator<Item = String>) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    let mut out: Vec<String> = Vec::new();

    for value in values {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            continue;
        }
        let folded = trimmed.to_lowercase();
        if !seen.contains(&folded) {
            seen.push(folded);
            out.push(trimmed.to_string());
        }
    }

    out.sort_by_key(|value| value.to_lowercase());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinct_folds_case_and_sorts() {
        let values = ["Web", "db", "web ", "", "  ", "API"]
            .into_iter()
            .map(String::from);
        assert_eq!(distinct(values), vec!["API", "db", "Web"]);
    }
}
