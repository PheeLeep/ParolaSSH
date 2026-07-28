//! Reading `~/.ssh/known_hosts`.
//!
//! The audit cares about two things here: whether the host list is hashed
//! (an unhashed file is a readable map of everything you connect to), and
//! whether any entry pins a host key algorithm that is no longer trustworthy.

use std::path::Path;

use serde::Serialize;

/// Host key algorithms that should no longer appear in a `known_hosts` file.
///
/// `ssh-dss` is disabled by default in modern OpenSSH; `ssh-rsa` is the
/// SHA-1 signature variant, distinct from the `rsa-sha2-*` names.
pub const DEPRECATED_KEY_TYPES: &[&str] = &["ssh-dss", "ssh-rsa"];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnownHostsEntry {
    /// 1-based line number.
    pub line: usize,
    /// `@revoked` or `@cert-authority`, when present.
    pub marker: Option<String>,
    /// The host field as written - `|1|…` when hashed.
    pub hosts: String,
    pub hashed: bool,
    pub key_type: String,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct KnownHosts {
    pub exists: bool,
    pub entries: Vec<KnownHostsEntry>,
}

impl KnownHosts {
    pub fn read(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(text) => Self {
                exists: true,
                entries: parse(&text),
            },
            Err(_) => Self::default(),
        }
    }

    pub fn unhashed(&self) -> Vec<&KnownHostsEntry> {
        self.entries.iter().filter(|entry| !entry.hashed).collect()
    }

    pub fn deprecated(&self) -> Vec<&KnownHostsEntry> {
        self.entries
            .iter()
            .filter(|entry| DEPRECATED_KEY_TYPES.contains(&entry.key_type.as_str()))
            .collect()
    }
}

pub fn parse(text: &str) -> Vec<KnownHostsEntry> {
    let mut entries = Vec::new();

    for (index, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let mut fields = line.split_whitespace();
        let Some(first) = fields.next() else {
            continue;
        };

        // A leading `@marker` shifts every other field along by one.
        let (marker, hosts) = if first.starts_with('@') {
            match fields.next() {
                Some(hosts) => (Some(first.to_string()), hosts),
                None => continue,
            }
        } else {
            (None, first)
        };

        let Some(key_type) = fields.next() else {
            continue;
        };

        entries.push(KnownHostsEntry {
            line: index + 1,
            marker,
            hosts: hosts.to_string(),
            hashed: hosts.starts_with("|1|"),
            key_type: key_type.to_string(),
        });
    }

    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
# a comment
github.com ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl
|1|F1E2D3=|A9B8C7= ssh-rsa AAAAB3NzaC1yc2E
@revoked old.example.com ssh-dss AAAAB3NzaC1kc3M
";

    #[test]
    fn parses_plain_hashed_and_marked_entries() {
        let entries = parse(SAMPLE);
        assert_eq!(entries.len(), 3);

        assert_eq!(entries[0].hosts, "github.com");
        assert!(!entries[0].hashed);
        assert_eq!(entries[0].key_type, "ssh-ed25519");
        assert_eq!(entries[0].line, 2);

        assert!(entries[1].hashed);
        assert_eq!(entries[1].key_type, "ssh-rsa");

        // The marker must not be mistaken for the host field.
        assert_eq!(entries[2].marker.as_deref(), Some("@revoked"));
        assert_eq!(entries[2].hosts, "old.example.com");
        assert_eq!(entries[2].key_type, "ssh-dss");
    }

    #[test]
    fn separates_unhashed_and_deprecated() {
        let known = KnownHosts {
            exists: true,
            entries: parse(SAMPLE),
        };

        assert_eq!(known.unhashed().len(), 2);

        let deprecated: Vec<_> = known
            .deprecated()
            .iter()
            .map(|entry| entry.key_type.clone())
            .collect();
        assert_eq!(deprecated, vec!["ssh-rsa", "ssh-dss"]);
    }

    #[test]
    fn tolerates_crlf_and_truncated_lines() {
        let entries = parse("host.example ssh-ed25519 AAAA\r\nbroken\r\n");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].key_type, "ssh-ed25519");
    }
}
