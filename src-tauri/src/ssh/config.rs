//! A tolerant reader for `~/.ssh/config`.
//!
//! This is not a full OpenSSH config implementation — it exists to answer the
//! audit's questions ("does anything turn off host key checking?", "which
//! identity files are referenced?"), so it keeps line numbers and leaves
//! semantics to the rules.

use std::path::{Path, PathBuf};

use serde::Serialize;

use super::paths::expand_tilde;

/// Directives that only mean anything on macOS.
///
/// Flagging `UseKeychain` as unknown on a Mac would be wrong; not mentioning
/// that it is inert on Windows and Linux would also be wrong.
pub const MACOS_ONLY_KEYWORDS: &[&str] = &["usekeychain", "applepersistentkeychain"];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Directive {
    pub keyword: String,
    pub value: String,
    /// 1-based, so it matches what an editor shows.
    pub line: usize,
}

impl Directive {
    pub fn keyword_lower(&self) -> String {
        self.keyword.to_lowercase()
    }

    /// True for `yes`, `true`, `1` — the spellings ssh accepts as affirmative.
    pub fn is_yes(&self) -> bool {
        matches!(self.value.trim().to_lowercase().as_str(), "yes" | "true" | "1")
    }

    pub fn is_no(&self) -> bool {
        matches!(
            self.value.trim().to_lowercase().as_str(),
            "no" | "false" | "0"
        )
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostBlock {
    pub patterns: Vec<String>,
    pub line: usize,
    pub directives: Vec<Directive>,
}

impl HostBlock {
    /// True when this block applies to every host — `Host *`.
    ///
    /// A secret set here reaches every server you connect to, which is what
    /// makes `ForwardAgent yes` under a wildcard so much worse than under a
    /// single named host.
    pub fn is_wildcard(&self) -> bool {
        self.patterns.iter().any(|pattern| pattern == "*")
    }

    pub fn label(&self) -> String {
        if self.patterns.is_empty() {
            "(top level)".to_string()
        } else {
            self.patterns.join(" ")
        }
    }

    /// Look up a directive by its lowercase keyword.
    #[allow(dead_code)]
    pub fn find(&self, keyword: &str) -> Option<&Directive> {
        self.directives
            .iter()
            .find(|directive| directive.keyword_lower() == keyword)
    }
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SshConfig {
    pub exists: bool,
    pub blocks: Vec<HostBlock>,
}

impl SshConfig {
    pub fn read(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(text) => Self {
                exists: true,
                blocks: parse(&text),
            },
            Err(_) => Self::default(),
        }
    }

    /// Every `IdentityFile` path in the file, tilde-expanded and de-duplicated.
    pub fn identity_files(&self) -> Vec<PathBuf> {
        let mut found: Vec<PathBuf> = Vec::new();
        for block in &self.blocks {
            for directive in &block.directives {
                if directive.keyword_lower() != "identityfile" {
                    continue;
                }
                let path = expand_tilde(&unquote(&directive.value));
                if !found.contains(&path) {
                    found.push(path);
                }
            }
        }
        found
    }

    pub fn all_directives(&self) -> impl Iterator<Item = (&HostBlock, &Directive)> {
        self.blocks
            .iter()
            .flat_map(|block| block.directives.iter().map(move |d| (block, d)))
    }
}

/// Split a config into host blocks.
///
/// Directives before the first `Host` line land in a leading block with no
/// patterns; OpenSSH treats those as global, so the audit still needs them.
pub fn parse(text: &str) -> Vec<HostBlock> {
    let mut blocks: Vec<HostBlock> = Vec::new();
    let mut current = HostBlock {
        patterns: Vec::new(),
        line: 0,
        directives: Vec::new(),
    };

    for (index, raw) in text.lines().enumerate() {
        let line = index + 1;
        // `.lines()` leaves a trailing `\r` on CRLF files; `trim` removes it.
        let Some((keyword, value)) = split_directive(raw) else {
            continue;
        };

        if keyword.eq_ignore_ascii_case("host") {
            if !current.patterns.is_empty() || !current.directives.is_empty() {
                blocks.push(current);
            }
            current = HostBlock {
                patterns: value.split_whitespace().map(str::to_string).collect(),
                line,
                directives: Vec::new(),
            };
            continue;
        }

        current.directives.push(Directive {
            keyword: keyword.to_string(),
            value: value.to_string(),
            line,
        });
    }

    if !current.patterns.is_empty() || !current.directives.is_empty() {
        blocks.push(current);
    }

    blocks
}

/// Split `Keyword value`, `Keyword=value` or `Keyword = value`.
fn split_directive(raw: &str) -> Option<(&str, &str)> {
    let line = raw.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }

    // An `=` before any whitespace means the equals form.
    let separator = line
        .find(|c: char| c.is_whitespace() || c == '=')
        .unwrap_or(line.len());

    let keyword = &line[..separator];
    if keyword.is_empty() {
        return None;
    }

    let value = line[separator..]
        .trim_start_matches(|c: char| c.is_whitespace() || c == '=')
        .trim();

    Some((keyword, value))
}

fn unquote(value: &str) -> String {
    let trimmed = value.trim();
    trimmed
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or(trimmed)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
# leading comment
ForwardAgent no

Host prod-*
    User deploy
    IdentityFile ~/.ssh/id_ed25519
    StrictHostKeyChecking no

Host *
  ForwardAgent yes
  UserKnownHostsFile=/dev/null
";

    #[test]
    fn splits_into_blocks_and_keeps_globals() {
        let blocks = parse(SAMPLE);
        assert_eq!(blocks.len(), 3);

        // Directives before any `Host` line are global.
        assert!(blocks[0].patterns.is_empty());
        assert_eq!(blocks[0].directives[0].keyword, "ForwardAgent");
        assert!(blocks[0].directives[0].is_no());

        assert_eq!(blocks[1].patterns, vec!["prod-*"]);
        assert!(!blocks[1].is_wildcard());

        assert_eq!(blocks[2].patterns, vec!["*"]);
        assert!(blocks[2].is_wildcard());
    }

    #[test]
    fn reports_one_based_line_numbers() {
        let blocks = parse(SAMPLE);
        let strict = blocks[1].find("stricthostkeychecking").unwrap();
        assert_eq!(strict.line, 7);
        assert!(strict.is_no());
    }

    #[test]
    fn accepts_the_equals_form() {
        let blocks = parse(SAMPLE);
        let known_hosts = blocks[2].find("userknownhostsfile").unwrap();
        assert_eq!(known_hosts.value, "/dev/null");
    }

    #[test]
    fn tolerates_crlf_line_endings() {
        let blocks = parse("Host win\r\n    ForwardAgent yes\r\n");
        assert_eq!(blocks[0].patterns, vec!["win"]);

        let forward = blocks[0].find("forwardagent").unwrap();
        // Without trimming the `\r` this would be "yes\r" and fail to match.
        assert_eq!(forward.value, "yes");
        assert!(forward.is_yes());
    }

    #[test]
    fn collects_and_expands_identity_files() {
        let home = dirs::home_dir().unwrap();
        let config = SshConfig {
            exists: true,
            blocks: parse(SAMPLE),
        };

        assert_eq!(
            config.identity_files(),
            vec![home.join(".ssh/id_ed25519")]
        );
    }

    #[test]
    fn ignores_comments_and_blank_lines() {
        let blocks = parse("# nothing\n\n   \n# more\n");
        assert!(blocks.is_empty());
    }

    #[test]
    fn strips_quotes_from_identity_paths() {
        let config = SshConfig {
            exists: true,
            blocks: parse("Host a\n  IdentityFile \"/keys/my key\"\n"),
        };
        assert_eq!(config.identity_files(), vec![PathBuf::from("/keys/my key")]);
    }
}
