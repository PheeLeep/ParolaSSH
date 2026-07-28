//! The shape of a saved connection.
//!
//! A record holds everything needed to *reach* a host and nothing needed to
//! *authenticate* as one: no passwords, no passphrases, no key material. The
//! file this serialises into is plain JSON in the app config directory, so
//! anything stored here should be safe to read over someone's shoulder.

use serde::{Deserialize, Serialize};

use crate::ssh::{SshError, SshResult};

/// How the client proves who it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthMethod {
    Password,
    Publickey,
    Agent,
    /// SSH's `none` method: present no credential and let the server decide.
    ///
    /// For Tailscale SSH, which authenticates the node over WireGuard before
    /// the SSH layer is reached and then offers `none` - a host that has it
    /// enabled cannot be connected to any other way. Never tried as a
    /// fallback: it is chosen per host, so "no credential was sent" is always
    /// something the operator asked for.
    None,
}

/// A saved connection.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostRecord {
    pub id: String,
    pub label: String,
    pub hostname: String,
    pub port: u16,
    pub username: String,
    pub auth_method: AuthMethod,
    /// Private key to offer when `auth_method` is `Publickey`.
    #[serde(default)]
    pub key_path: Option<String>,
    /// Folder the host is filed under. Never empty - see `HostDraft::validate`.
    pub group: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub notes: Option<String>,
    /// ISO 8601 of the last successful connection, or `None` if never.
    #[serde(default)]
    pub last_connected: Option<String>,
}

/// What the form sends. `id` is absent when adding, present when editing -
/// which is the only difference between the two operations.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostDraft {
    #[serde(default)]
    pub id: Option<String>,
    pub label: String,
    pub hostname: String,
    pub port: u16,
    pub username: String,
    pub auth_method: AuthMethod,
    #[serde(default)]
    pub key_path: Option<String>,
    pub group: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

/// Group name used when the form leaves the field blank.
pub const DEFAULT_GROUP: &str = "Ungrouped";

impl HostDraft {
    /// Trim, apply defaults, and reject anything unusable.
    ///
    /// Validation lives here rather than in the dialog because the store is
    /// the thing that has to stay consistent - a record with an empty hostname
    /// would sit in the list forever failing to connect for no visible reason.
    pub fn validate(self) -> SshResult<ValidDraft> {
        let hostname = self.hostname.trim().to_string();
        if hostname.is_empty() {
            return Err(SshError::invalid("A hostname or IP address is required."));
        }
        if hostname.split_whitespace().count() > 1 {
            return Err(SshError::invalid(
                "A hostname cannot contain spaces. Use one address per connection.",
            ));
        }

        let username = self.username.trim().to_string();
        if username.is_empty() {
            return Err(SshError::invalid("A username is required."));
        }

        if self.port == 0 {
            return Err(SshError::invalid("Port must be between 1 and 65535."));
        }

        if self.auth_method == AuthMethod::Publickey
            && self
                .key_path
                .as_ref()
                .map(|path| path.trim().is_empty())
                .unwrap_or(true)
        {
            return Err(SshError::invalid(
                "Choose a private key, or switch to password or agent authentication.",
            ));
        }

        // An unnamed connection falls back to `user@host` rather than being
        // rejected: the label is a convenience, not information we need.
        let label = match self.label.trim() {
            "" => format!("{username}@{hostname}"),
            trimmed => trimmed.to_string(),
        };

        let group = match self.group.trim() {
            "" => DEFAULT_GROUP.to_string(),
            trimmed => trimmed.to_string(),
        };

        Ok(ValidDraft {
            id: self.id,
            label,
            hostname,
            port: self.port,
            username,
            auth_method: self.auth_method,
            key_path: self
                .key_path
                .map(|path| path.trim().to_string())
                .filter(|path| !path.is_empty()),
            group,
            tags: normalise_tags(self.tags),
            notes: self
                .notes
                .map(|note| note.trim().to_string())
                .filter(|note| !note.is_empty()),
        })
    }
}

/// A draft that has passed `validate`. Only the store can build one.
pub struct ValidDraft {
    pub id: Option<String>,
    pub label: String,
    pub hostname: String,
    pub port: u16,
    pub username: String,
    pub auth_method: AuthMethod,
    pub key_path: Option<String>,
    pub group: String,
    pub tags: Vec<String>,
    pub notes: Option<String>,
}

impl ValidDraft {
    /// Fold the draft into a record, keeping fields the form does not own.
    pub fn apply_to(self, id: String, last_connected: Option<String>) -> HostRecord {
        HostRecord {
            id,
            label: self.label,
            hostname: self.hostname,
            port: self.port,
            username: self.username,
            auth_method: self.auth_method,
            key_path: self.key_path,
            group: self.group,
            tags: self.tags,
            notes: self.notes,
            last_connected,
        }
    }
}

/// Trim, drop blanks, and de-duplicate case-insensitively.
///
/// Tags are used for filtering, so `Web`, `web`, and `web ` arriving as three
/// separate chips would quietly split a filter in three. First spelling wins.
fn normalise_tags(tags: Vec<String>) -> Vec<String> {
    let mut seen = Vec::new();
    let mut out = Vec::new();

    for tag in tags {
        let trimmed = tag.trim();
        if trimmed.is_empty() {
            continue;
        }
        let folded = trimmed.to_lowercase();
        if seen.contains(&folded) {
            continue;
        }
        seen.push(folded);
        out.push(trimmed.to_string());
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft() -> HostDraft {
        HostDraft {
            id: None,
            label: "  Test VM  ".into(),
            hostname: "  192.168.56.10 ".into(),
            port: 22,
            username: " pheeleep ".into(),
            auth_method: AuthMethod::Password,
            key_path: None,
            group: "  Lab  ".into(),
            tags: vec!["web".into(), " Web ".into(), "".into(), "db".into()],
            notes: Some("   ".into()),
        }
    }

    #[test]
    fn trims_and_defaults() {
        let valid = draft().validate().unwrap();
        assert_eq!(valid.label, "Test VM");
        assert_eq!(valid.hostname, "192.168.56.10");
        assert_eq!(valid.username, "pheeleep");
        assert_eq!(valid.group, "Lab");
        // Duplicate differing only by case and padding collapses; first wins.
        assert_eq!(valid.tags, vec!["web", "db"]);
        // A whitespace-only note is the same as no note.
        assert_eq!(valid.notes, None);
    }

    #[test]
    fn blank_label_falls_back_to_user_at_host() {
        let mut input = draft();
        input.label = "   ".into();
        assert_eq!(input.validate().unwrap().label, "pheeleep@192.168.56.10");
    }

    #[test]
    fn blank_group_falls_back_to_default() {
        let mut input = draft();
        input.group = "".into();
        assert_eq!(input.validate().unwrap().group, DEFAULT_GROUP);
    }

    #[test]
    fn rejects_unusable_records() {
        let mut missing_host = draft();
        missing_host.hostname = "  ".into();
        assert!(missing_host.validate().is_err());

        let mut spaced_host = draft();
        spaced_host.hostname = "one two".into();
        assert!(spaced_host.validate().is_err());

        let mut missing_user = draft();
        missing_user.username = "".into();
        assert!(missing_user.validate().is_err());

        let mut zero_port = draft();
        zero_port.port = 0;
        assert!(zero_port.validate().is_err());

        // Public key auth without a key would fail at connect time instead.
        let mut no_key = draft();
        no_key.auth_method = AuthMethod::Publickey;
        assert!(no_key.validate().is_err());
    }
}
