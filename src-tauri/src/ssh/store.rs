//! Persistence for dismissed audit findings.
//!
//! Some findings are deliberate choices — an unencrypted key used by a CI
//! agent, say. A rule that cannot be dismissed trains people to ignore the
//! whole report, so dismissals are stored and survive restarts.
//!
//! This holds finding ids only. No key material, no paths beyond the ones the
//! user already sees in the report.

use std::collections::HashSet;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{SshError, SshResult};

const FILE_NAME: &str = "audit-suppressions.json";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Suppressions {
    #[serde(default)]
    pub ids: Vec<String>,
}

impl Suppressions {
    pub fn read(config_dir: &Path) -> Self {
        std::fs::read_to_string(config_dir.join(FILE_NAME))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn write(&self, config_dir: &Path) -> SshResult<()> {
        std::fs::create_dir_all(config_dir)
            .map_err(|error| SshError::io("Could not create the settings directory", error))?;

        let text = serde_json::to_string_pretty(self)
            .map_err(|error| SshError::invalid(format!("Could not encode settings: {error}")))?;

        std::fs::write(config_dir.join(FILE_NAME), text)
            .map_err(|error| SshError::io("Could not save settings", error))
    }

    pub fn as_set(&self) -> HashSet<String> {
        self.ids.iter().cloned().collect()
    }

    pub fn insert(&mut self, id: String) {
        if !self.ids.contains(&id) {
            self.ids.push(id);
        }
    }

    pub fn remove(&mut self, id: &str) {
        self.ids.retain(|existing| existing != id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_disk() {
        let dir = std::env::temp_dir().join(format!("parolassh-store-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let mut suppressions = Suppressions::default();
        suppressions.insert("key.unencrypted|SHA256:abc".to_string());
        // Inserting twice must not duplicate.
        suppressions.insert("key.unencrypted|SHA256:abc".to_string());
        suppressions.write(&dir).unwrap();

        let loaded = Suppressions::read(&dir);
        assert_eq!(loaded.ids, vec!["key.unencrypted|SHA256:abc"]);
        assert!(loaded.as_set().contains("key.unencrypted|SHA256:abc"));

        let mut loaded = loaded;
        loaded.remove("key.unencrypted|SHA256:abc");
        loaded.write(&dir).unwrap();
        assert!(Suppressions::read(&dir).ids.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_or_corrupt_file_reads_as_empty() {
        let missing = Path::new("/nonexistent-parolassh-dir");
        assert!(Suppressions::read(missing).ids.is_empty());
    }
}
