//! Persistence for saved tasks.
//!
//! One JSON file beside `hosts.json`, written owner-only through
//! `private_file`. A task is a command someone intends to run as root on a
//! named machine, so the file is no more readable than the address book is.
//!
//! Deleting a host takes its per-host tasks with it - `forget_host` is called
//! from the same place the record is removed, so the file cannot accumulate
//! tasks pinned to machines that no longer exist.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::model::{TaskRecord, TaskScope, ValidDraft};
use crate::private_file;
use crate::remote::OsFamily;
use crate::ssh::{SshError, SshResult};

const FILE_NAME: &str = "tasks.json";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskStore {
    #[serde(default)]
    pub tasks: Vec<TaskRecord>,
}

impl TaskStore {
    /// Read the list, treating a missing or unreadable file as empty - the
    /// same bargain `HostStore` makes, for the same reason.
    pub fn read(config_dir: &Path) -> Self {
        std::fs::read_to_string(config_dir.join(FILE_NAME))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn write(&self, config_dir: &Path) -> SshResult<()> {
        let text = serde_json::to_string_pretty(self)
            .map_err(|error| SshError::invalid(format!("Could not encode tasks: {error}")))?;

        private_file::write(config_dir, FILE_NAME, &text)
    }

    pub fn get(&self, id: &str) -> Option<&TaskRecord> {
        self.tasks.iter().find(|task| task.id == id)
    }

    /// Everything offered on this host: global tasks plus its own, filtered to
    /// what runs on its OS. Order is stable - global first, then per-host -
    /// so the list does not reshuffle between visits.
    pub fn for_host(&self, host_id: &str, os: OsFamily) -> Vec<&TaskRecord> {
        let mut global: Vec<&TaskRecord> = Vec::new();
        let mut pinned: Vec<&TaskRecord> = Vec::new();

        for task in &self.tasks {
            if !task.scope.applies_to(host_id) || !task.supports(os) {
                continue;
            }
            match task.scope {
                TaskScope::Global => global.push(task),
                TaskScope::Host { .. } => pinned.push(task),
            }
        }

        global.extend(pinned);
        global
    }

    /// Insert or update, returning the stored record. An unknown id errors
    /// rather than inserting: it means another window deleted it, and
    /// resurrecting it silently would surprise.
    pub fn upsert(&mut self, draft: ValidDraft, now: String) -> SshResult<TaskRecord> {
        match draft.id.clone() {
            Some(id) => {
                let index = self
                    .tasks
                    .iter()
                    .position(|task| task.id == id)
                    .ok_or_else(|| {
                        SshError::invalid("That task no longer exists - it may have been deleted.")
                    })?;

                // Age and run history belong to the task, not to the form.
                let created = self.tasks[index].created_at.clone();
                let last_run = self.tasks[index].last_run_at.clone();
                let record = draft.apply_to(id, created, last_run);
                self.tasks[index] = record.clone();
                Ok(record)
            }
            None => {
                let record = draft.apply_to(new_id(), Some(now), None);
                self.tasks.push(record.clone());
                Ok(record)
            }
        }
    }

    pub fn remove(&mut self, id: &str) -> SshResult<TaskRecord> {
        let index = self
            .tasks
            .iter()
            .position(|task| task.id == id)
            .ok_or_else(|| SshError::invalid("That task no longer exists."))?;

        Ok(self.tasks.remove(index))
    }

    /// Stamp a run. A missing id is ignored: a task deleted while its run was
    /// in flight should not fail the run it already finished.
    pub fn touch(&mut self, id: &str, timestamp: String) {
        if let Some(task) = self.tasks.iter_mut().find(|task| task.id == id) {
            task.last_run_at = Some(timestamp);
        }
    }

    /// Drop every task pinned to a host being deleted. Returns how many went,
    /// so the caller can say so rather than deleting silently.
    pub fn forget_host(&mut self, host_id: &str) -> usize {
        let before = self.tasks.len();
        self.tasks.retain(|task| match &task.scope {
            TaskScope::Global => true,
            TaskScope::Host { host_id: owner } => owner != host_id,
        });
        before - self.tasks.len()
    }
}

/// Path of the store file, for messages that need to name it.
pub fn store_path(config_dir: &Path) -> PathBuf {
    config_dir.join(FILE_NAME)
}

/// Unique inside one JSON file, which is all that is needed.
fn new_id() -> String {
    let mut bytes = [0u8; 8];
    rand_core::RngCore::fill_bytes(&mut rand_core::OsRng, &mut bytes);
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("t-{hex}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::model::TaskDraft;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join(format!("parolassh-tasks-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn draft(name: &str, scope: TaskScope) -> TaskDraft {
        TaskDraft {
            id: None,
            name: name.into(),
            description: None,
            command: "uptime".into(),
            elevated: false,
            scope,
            os_families: Vec::new(),
        }
    }

    fn pinned(host: &str) -> TaskScope {
        TaskScope::Host {
            host_id: host.into(),
        }
    }

    fn now() -> String {
        "2026-07-28T09:00:00Z".to_string()
    }

    #[test]
    fn add_edit_remove_round_trip() {
        let dir = temp_dir("crud");
        let mut store = TaskStore::default();

        let added = store
            .upsert(draft("Uptime", TaskScope::Global).validate().unwrap(), now())
            .unwrap();
        assert!(added.id.starts_with("t-"));
        assert_eq!(added.created_at.as_deref(), Some("2026-07-28T09:00:00Z"));
        store.write(&dir).unwrap();

        let mut store = TaskStore::read(&dir);
        assert_eq!(store.tasks.len(), 1);

        let mut edit = draft("Uptime and load", TaskScope::Global);
        edit.id = Some(added.id.clone());
        edit.command = "uptime; cat /proc/loadavg".into();
        let edited = store.upsert(edit.validate().unwrap(), now()).unwrap();
        assert_eq!(edited.id, added.id, "editing must not mint a new id");
        assert_eq!(store.tasks.len(), 1, "an edit must not append");

        store.remove(&added.id).unwrap();
        store.write(&dir).unwrap();
        assert!(TaskStore::read(&dir).tasks.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn editing_preserves_creation_and_run_history() {
        let mut store = TaskStore::default();
        let added = store
            .upsert(draft("Uptime", TaskScope::Global).validate().unwrap(), now())
            .unwrap();
        store.touch(&added.id, "2026-07-28T10:00:00Z".into());

        let mut edit = draft("Renamed", TaskScope::Global);
        edit.id = Some(added.id.clone());
        let edited = store.upsert(edit.validate().unwrap(), now()).unwrap();

        assert_eq!(edited.created_at.as_deref(), Some("2026-07-28T09:00:00Z"));
        assert_eq!(edited.last_run_at.as_deref(), Some("2026-07-28T10:00:00Z"));
    }

    #[test]
    fn a_host_sees_global_tasks_and_its_own() {
        let mut store = TaskStore::default();
        store
            .upsert(draft("Everywhere", TaskScope::Global).validate().unwrap(), now())
            .unwrap();
        store
            .upsert(draft("Web only", pinned("h-web")).validate().unwrap(), now())
            .unwrap();
        store
            .upsert(draft("DB only", pinned("h-db")).validate().unwrap(), now())
            .unwrap();

        let web: Vec<&str> = store
            .for_host("h-web", OsFamily::Linux)
            .iter()
            .map(|task| task.name.as_str())
            .collect();
        assert_eq!(web, vec!["Everywhere", "Web only"], "global tasks list first");

        let db: Vec<&str> = store
            .for_host("h-db", OsFamily::Linux)
            .iter()
            .map(|task| task.name.as_str())
            .collect();
        assert_eq!(db, vec!["Everywhere", "DB only"]);

        // A host with nothing pinned to it still sees the global ones.
        assert_eq!(store.for_host("h-new", OsFamily::Linux).len(), 1);
    }

    #[test]
    fn a_task_is_hidden_from_a_host_whose_os_it_does_not_support() {
        let mut store = TaskStore::default();
        let mut linux_only = draft("systemd thing", TaskScope::Global);
        linux_only.os_families = vec![OsFamily::Linux];
        store.upsert(linux_only.validate().unwrap(), now()).unwrap();

        assert_eq!(store.for_host("h-1", OsFamily::Linux).len(), 1);
        assert!(store.for_host("h-1", OsFamily::Windows).is_empty());
    }

    #[test]
    fn deleting_a_host_takes_its_tasks_and_leaves_the_global_ones() {
        let mut store = TaskStore::default();
        store
            .upsert(draft("Everywhere", TaskScope::Global).validate().unwrap(), now())
            .unwrap();
        store
            .upsert(draft("Web A", pinned("h-web")).validate().unwrap(), now())
            .unwrap();
        store
            .upsert(draft("Web B", pinned("h-web")).validate().unwrap(), now())
            .unwrap();

        assert_eq!(store.forget_host("h-web"), 2);
        assert_eq!(store.tasks.len(), 1);
        assert_eq!(store.tasks[0].name, "Everywhere");

        // A host with nothing pinned removes nothing.
        assert_eq!(store.forget_host("h-none"), 0);
    }

    #[test]
    fn editing_or_removing_an_unknown_id_errors() {
        let mut store = TaskStore::default();

        let mut orphan = draft("Ghost", TaskScope::Global);
        orphan.id = Some("t-does-not-exist".into());
        assert!(store.upsert(orphan.validate().unwrap(), now()).is_err());
        assert!(store.remove("t-does-not-exist").is_err());
    }

    #[test]
    fn corrupt_file_reads_as_empty() {
        let dir = temp_dir("corrupt");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(store_path(&dir), "{ not json").unwrap();
        assert!(TaskStore::read(&dir).tasks.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ids_are_unique() {
        let mut store = TaskStore::default();
        for index in 0..50 {
            store
                .upsert(
                    draft(&format!("Task {index}"), TaskScope::Global)
                        .validate()
                        .unwrap(),
                    now(),
                )
                .unwrap();
        }
        let mut ids: Vec<_> = store.tasks.iter().map(|task| task.id.clone()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 50);
    }
}
