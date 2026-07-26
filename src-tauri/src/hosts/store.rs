//! Persistence for saved connections.
//!
//! One JSON file in the app config directory, written owner-only through
//! `private_file` — it is a list of every machine you administer, so it is not
//! left at whatever `umask` allows.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::model::{HostRecord, ValidDraft};
use crate::private_file;
use crate::ssh::{SshError, SshResult};

const FILE_NAME: &str = "hosts.json";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostStore {
    #[serde(default)]
    pub hosts: Vec<HostRecord>,
}

impl HostStore {
    /// Read the list, treating a missing or unreadable file as empty, so a
    /// corrupt file leaves the app usable and the next save rewrites it.
    pub fn read(config_dir: &Path) -> Self {
        std::fs::read_to_string(config_dir.join(FILE_NAME))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn write(&self, config_dir: &Path) -> SshResult<()> {
        let text = serde_json::to_string_pretty(self)
            .map_err(|error| SshError::invalid(format!("Could not encode connections: {error}")))?;

        private_file::write(config_dir, FILE_NAME, &text)
    }

    pub fn get(&self, id: &str) -> Option<&HostRecord> {
        self.hosts.iter().find(|host| host.id == id)
    }

    /// Insert or update, returning the stored record. An unknown id errors
    /// rather than inserting: it means another window deleted the record, and
    /// resurrecting it silently would surprise.
    pub fn upsert(&mut self, draft: ValidDraft) -> SshResult<HostRecord> {
        match draft.id.clone() {
            Some(id) => {
                let index = self
                    .hosts
                    .iter()
                    .position(|host| host.id == id)
                    .ok_or_else(|| {
                        SshError::invalid("That connection no longer exists — it may have been deleted.")
                    })?;

                // `lastConnected` is owned by the connect path, not the form.
                let record = draft.apply_to(id, self.hosts[index].last_connected.clone());
                self.hosts[index] = record.clone();
                Ok(record)
            }
            None => {
                let record = draft.apply_to(new_id(), None);
                self.hosts.push(record.clone());
                Ok(record)
            }
        }
    }

    /// Remove by id. Returns the record that was removed.
    pub fn remove(&mut self, id: &str) -> SshResult<HostRecord> {
        let index = self
            .hosts
            .iter()
            .position(|host| host.id == id)
            .ok_or_else(|| SshError::invalid("That connection no longer exists."))?;

        Ok(self.hosts.remove(index))
    }

    /// Stamp a successful connection. Missing ids are ignored, so a host
    /// deleted mid-session does not fail a working connection.
    pub fn touch(&mut self, id: &str, timestamp: String) {
        if let Some(host) = self.hosts.iter_mut().find(|host| host.id == id) {
            host.last_connected = Some(timestamp);
        }
    }
}

/// Path of the store file, for messages that need to name it.
pub fn store_path(config_dir: &Path) -> PathBuf {
    config_dir.join(FILE_NAME)
}

/// Random enough to never collide within one list. Not a UUID: uniqueness is
/// only needed inside a single JSON file.
fn new_id() -> String {
    let mut bytes = [0u8; 8];
    rand_core::RngCore::fill_bytes(&mut rand_core::OsRng, &mut bytes);
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("h-{hex}")
}

/// ISO 8601 UTC, to the second. Hand-rolled rather than pulling in `chrono` for
/// one format string; the trailing `Z` is what `Date.parse` needs.
pub fn now_iso8601() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let days = secs.div_euclid(86_400);
    let time_of_day = secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);

    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        time_of_day / 3600,
        (time_of_day % 3600) / 60,
        time_of_day % 60,
    )
}

/// Howard Hinnant's `civil_from_days`: days since the Unix epoch to Y-M-D.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hosts::model::{AuthMethod, HostDraft};

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "parolassh-hosts-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn draft(label: &str) -> HostDraft {
        HostDraft {
            id: None,
            label: label.into(),
            hostname: "192.168.56.10".into(),
            port: 22,
            username: "pheeleep".into(),
            auth_method: AuthMethod::Password,
            key_path: None,
            group: "Lab".into(),
            tags: vec!["vm".into()],
            notes: None,
        }
    }

    #[test]
    fn add_edit_remove_round_trip() {
        let dir = temp_dir("crud");
        let mut store = HostStore::default();

        let added = store.upsert(draft("Test VM").validate().unwrap()).unwrap();
        assert_eq!(store.hosts.len(), 1);
        assert!(added.id.starts_with("h-"));
        store.write(&dir).unwrap();

        // Reload, then edit through the same id.
        let mut store = HostStore::read(&dir);
        assert_eq!(store.hosts.len(), 1);

        let mut edit = draft("Renamed VM");
        edit.id = Some(added.id.clone());
        edit.port = 2222;
        let edited = store.upsert(edit.validate().unwrap()).unwrap();
        assert_eq!(edited.id, added.id, "editing must not mint a new id");
        assert_eq!(edited.label, "Renamed VM");
        assert_eq!(edited.port, 2222);
        assert_eq!(store.hosts.len(), 1, "an edit must not append");

        store.remove(&added.id).unwrap();
        assert!(store.hosts.is_empty());
        store.write(&dir).unwrap();
        assert!(HostStore::read(&dir).hosts.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn editing_preserves_last_connected() {
        let mut store = HostStore::default();
        let added = store.upsert(draft("VM").validate().unwrap()).unwrap();
        store.touch(&added.id, "2026-07-25T10:00:00Z".into());

        let mut edit = draft("VM renamed");
        edit.id = Some(added.id.clone());
        let edited = store.upsert(edit.validate().unwrap()).unwrap();

        assert_eq!(edited.last_connected.as_deref(), Some("2026-07-25T10:00:00Z"));
    }

    #[test]
    fn editing_or_removing_an_unknown_id_errors() {
        let mut store = HostStore::default();

        let mut orphan = draft("Ghost");
        orphan.id = Some("h-does-not-exist".into());
        assert!(store.upsert(orphan.validate().unwrap()).is_err());
        assert!(store.remove("h-does-not-exist").is_err());
    }

    #[test]
    fn ids_are_unique() {
        let mut store = HostStore::default();
        for index in 0..50 {
            store
                .upsert(draft(&format!("VM {index}")).validate().unwrap())
                .unwrap();
        }
        let mut ids: Vec<_> = store.hosts.iter().map(|host| host.id.clone()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 50);
    }

    #[test]
    fn corrupt_file_reads_as_empty() {
        let dir = temp_dir("corrupt");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(store_path(&dir), "{ not json").unwrap();
        assert!(HostStore::read(&dir).hosts.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn timestamp_looks_like_iso8601() {
        let stamp = now_iso8601();
        assert_eq!(stamp.len(), 20, "{stamp}");
        assert!(stamp.ends_with('Z'), "{stamp}");
        // Known epoch conversions, to catch a broken date algorithm.
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_000), (2022, 1, 8));
    }
}
