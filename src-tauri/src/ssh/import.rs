//! Turning `~/.ssh/config` host blocks into importable connections.
//!
//! OpenSSH resolves a directive to the *first* value obtained in file order
//! across every block whose patterns match the alias, which is why a `Host *`
//! block at the bottom acts as a fallback and one at the top does not. That
//! rule is reproduced here so an imported host matches what `ssh <alias>` does.

use serde::Serialize;

use super::config::{HostBlock, SshConfig};

/// Port used when the block names none.
const DEFAULT_PORT: u16 = 22;

/// One alias that can become a saved connection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportCandidate {
    /// The `Host` pattern itself, used as the label.
    pub alias: String,
    pub hostname: String,
    pub port: u16,
    /// Empty when the config names no `User`; the dialog supplies one.
    pub username: String,
    pub key_path: Option<String>,
    /// Alias named by `ProxyJump`, resolved to a saved host after import.
    pub proxy_jump: Option<String>,
    /// 1-based line of the `Host` directive, so a row can be traced back.
    pub line: usize,
    /// Anything about this entry the operator should see before importing.
    pub notes: Vec<String>,
}

/// Everything the import dialog needs about the file.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportListing {
    pub path: String,
    /// False means no `~/.ssh/config` at all, which is not the same as a file
    /// holding no concrete hosts.
    pub exists: bool,
    pub candidates: Vec<ImportCandidate>,
    /// File-level remarks, such as `Include` lines that were not followed.
    pub notes: Vec<String>,
}

/// Build the candidate list. Pure over an already-parsed config.
pub fn candidates(config: &SshConfig) -> (Vec<ImportCandidate>, Vec<String>) {
    let mut out = Vec::new();
    let mut notes = Vec::new();

    if config
        .all_directives()
        .any(|(_, directive)| directive.keyword_lower() == "include")
    {
        notes.push(
            "This config has Include lines. Included files are not read, so hosts defined \
             in them are missing from this list."
                .to_string(),
        );
    }

    for block in &config.blocks {
        for pattern in &block.patterns {
            if !is_concrete(pattern) {
                continue;
            }
            out.push(build(config, block, pattern));
        }
    }

    (out, notes)
}

/// Read the config at the given path and describe it.
pub fn listing(path: &std::path::Path) -> ImportListing {
    let config = SshConfig::read(path);
    let (candidates, notes) = candidates(&config);

    ImportListing {
        path: path.to_string_lossy().to_string(),
        exists: config.exists,
        candidates,
        notes,
    }
}

/// A pattern naming exactly one host. Globs and negations describe a set, and
/// a set has no address to connect to.
fn is_concrete(pattern: &str) -> bool {
    !pattern.is_empty()
        && !pattern.contains('*')
        && !pattern.contains('?')
        && !pattern.starts_with('!')
}

fn build(config: &SshConfig, own: &HostBlock, alias: &str) -> ImportCandidate {
    let mut notes = Vec::new();

    let hostname = lookup(config, alias, "hostname").unwrap_or_else(|| alias.to_string());

    let port = match lookup(config, alias, "port") {
        None => DEFAULT_PORT,
        Some(value) => match value.parse::<u16>() {
            Ok(port) if port > 0 => port,
            _ => {
                notes.push(format!("Port “{value}” is not a usable port number; using 22."));
                DEFAULT_PORT
            }
        },
    };

    let key_path = lookup(config, alias, "identityfile").map(|value| unquote(&value));

    let jump = lookup(config, alias, "proxyjump").filter(|value| !value.eq_ignore_ascii_case("none"));
    let proxy_jump = jump.as_deref().and_then(|value| first_hop(value, &mut notes));

    if lookup(config, alias, "proxycommand").is_some() {
        notes.push(
            "ProxyCommand is not supported. Set a jump host on this connection instead."
                .to_string(),
        );
    }

    ImportCandidate {
        alias: alias.to_string(),
        hostname,
        port,
        username: lookup(config, alias, "user").unwrap_or_default(),
        key_path,
        proxy_jump,
        line: own.line,
        notes,
    }
}

/// First value for `keyword` across every block matching `alias`, in file
/// order - OpenSSH's "first obtained value wins".
fn lookup(config: &SshConfig, alias: &str, keyword: &str) -> Option<String> {
    config
        .blocks
        .iter()
        .filter(|block| block_matches(block, alias))
        .flat_map(|block| block.directives.iter())
        .find(|directive| directive.keyword_lower() == keyword)
        .map(|directive| directive.value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// A block with no patterns is the leading global one and applies to every
/// alias. Otherwise a negated pattern vetoes the block outright.
fn block_matches(block: &HostBlock, alias: &str) -> bool {
    if block.patterns.is_empty() {
        return true;
    }

    let mut matched = false;
    for pattern in &block.patterns {
        match pattern.strip_prefix('!') {
            Some(negated) if glob_matches(negated, alias) => return false,
            Some(_) => {}
            None if glob_matches(pattern, alias) => matched = true,
            None => {}
        }
    }
    matched
}

/// `*` and `?` only - the whole of ssh_config's pattern syntax.
fn glob_matches(pattern: &str, value: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let value: Vec<char> = value.chars().collect();
    let (mut p, mut v) = (0usize, 0usize);
    // Position to resume from when a `*` has to swallow one more character.
    let (mut star, mut resume) = (None, 0usize);

    while v < value.len() {
        if p < pattern.len() && (pattern[p] == '?' || eq_fold(pattern[p], value[v])) {
            p += 1;
            v += 1;
        } else if p < pattern.len() && pattern[p] == '*' {
            star = Some(p);
            resume = v;
            p += 1;
        } else if let Some(index) = star {
            p = index + 1;
            resume += 1;
            v = resume;
        } else {
            return false;
        }
    }

    pattern[p..].iter().all(|c| *c == '*')
}

/// Hostnames are case-insensitive, and so is ssh's pattern matching.
fn eq_fold(a: char, b: char) -> bool {
    a.eq_ignore_ascii_case(&b)
}

/// The alias of the first hop in a `ProxyJump` value, stripped of any
/// `user@` and `:port`. Later hops are reported, not imported.
fn first_hop(value: &str, notes: &mut Vec<String>) -> Option<String> {
    let mut hops = value.split(',').map(str::trim).filter(|hop| !hop.is_empty());
    let first = hops.next()?;

    if hops.next().is_some() {
        notes.push(format!(
            "ProxyJump lists several hops. Only the first ({first}) is imported."
        ));
    }

    let without_user = first.rsplit('@').next().unwrap_or(first);
    // An IPv6 literal is bracketed, so only strip a port from an unbracketed
    // value with exactly one colon.
    let host = match without_user.split_once(':') {
        Some((host, port)) if !without_user.starts_with('[') && !port.contains(':') => host,
        _ => without_user,
    };

    let host = host.trim();
    (!host.is_empty()).then(|| host.to_string())
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
    use crate::ssh::config::parse;

    fn config(text: &str) -> SshConfig {
        SshConfig {
            exists: true,
            blocks: parse(text),
        }
    }

    fn candidate(text: &str, alias: &str) -> ImportCandidate {
        let (list, _) = candidates(&config(text));
        list.into_iter()
            .find(|entry| entry.alias == alias)
            .unwrap_or_else(|| panic!("no candidate for {alias}"))
    }

    const SAMPLE: &str = "\
Host web01
    HostName web01.example.com
    User deploy
    Port 2222
    IdentityFile ~/.ssh/id_ed25519

Host db01 db02
    HostName 192.168.9.20
    ProxyJump bastion

Host bastion
    HostName bastion.example.com
    User jump

Host *
    User fallback
    Port 2200
";

    #[test]
    fn reads_a_concrete_block() {
        let web = candidate(SAMPLE, "web01");
        assert_eq!(web.hostname, "web01.example.com");
        assert_eq!(web.username, "deploy");
        assert_eq!(web.port, 2222);
        assert_eq!(web.key_path.as_deref(), Some("~/.ssh/id_ed25519"));
        assert!(web.proxy_jump.is_none());
    }

    #[test]
    fn wildcard_block_supplies_defaults_only() {
        // db01 names neither, so the trailing `Host *` fills both in.
        let db = candidate(SAMPLE, "db01");
        assert_eq!(db.username, "fallback");
        assert_eq!(db.port, 2200);
        // web01 names its own, and the earlier value wins.
        assert_eq!(candidate(SAMPLE, "web01").port, 2222);
    }

    #[test]
    fn a_wildcard_is_never_itself_a_candidate() {
        let (list, _) = candidates(&config(SAMPLE));
        assert!(list.iter().all(|entry| entry.alias != "*"));
        // Every concrete alias is, including both on a shared Host line.
        let aliases: Vec<_> = list.iter().map(|entry| entry.alias.as_str()).collect();
        assert_eq!(aliases, vec!["web01", "db01", "db02", "bastion"]);
    }

    #[test]
    fn hostname_defaults_to_the_alias() {
        assert_eq!(candidate("Host lonely\n  User me\n", "lonely").hostname, "lonely");
    }

    #[test]
    fn proxy_jump_records_the_alias() {
        assert_eq!(candidate(SAMPLE, "db01").proxy_jump.as_deref(), Some("bastion"));
    }

    #[test]
    fn proxy_jump_strips_user_and_port() {
        let text = "Host a\n  ProxyJump admin@gate.example.com:2222\n";
        assert_eq!(candidate(text, "a").proxy_jump.as_deref(), Some("gate.example.com"));
    }

    #[test]
    fn only_the_first_jump_hop_is_imported() {
        let entry = candidate("Host a\n  ProxyJump one,two\n", "a");
        assert_eq!(entry.proxy_jump.as_deref(), Some("one"));
        assert!(entry.notes.iter().any(|note| note.contains("several hops")));
    }

    #[test]
    fn proxy_jump_none_is_not_a_jump_host() {
        assert!(candidate("Host a\n  ProxyJump none\n", "a").proxy_jump.is_none());
    }

    #[test]
    fn proxy_command_is_reported_not_silently_dropped() {
        let entry = candidate("Host a\n  ProxyCommand nc %h %p\n", "a");
        assert!(entry.notes.iter().any(|note| note.contains("ProxyCommand")));
    }

    #[test]
    fn an_unusable_port_falls_back_and_says_so() {
        let entry = candidate("Host a\n  Port www\n", "a");
        assert_eq!(entry.port, DEFAULT_PORT);
        assert!(entry.notes.iter().any(|note| note.contains("not a usable port")));
    }

    #[test]
    fn negated_patterns_veto_their_block() {
        let text = "\
Host !secret.example.com *.example.com
    User shared

Host secret.example.com
    HostName secret.example.com

Host public.example.com
    HostName public.example.com
";
        // The wildcard block matches both names but negates the first.
        assert_eq!(candidate(text, "public.example.com").username, "shared");
        assert_eq!(candidate(text, "secret.example.com").username, "");
    }

    #[test]
    fn include_lines_are_reported() {
        let (_, notes) = candidates(&config("Include conf.d/*\nHost a\n  User me\n"));
        assert!(notes.iter().any(|note| note.contains("Include")));
    }

    #[test]
    fn tolerates_crlf_and_the_equals_form() {
        let entry = candidate("Host win\r\n  HostName=10.0.0.5\r\n  Port=2200\r\n", "win");
        assert_eq!(entry.hostname, "10.0.0.5");
        assert_eq!(entry.port, 2200);
    }

    #[test]
    fn quotes_are_stripped_from_identity_paths() {
        let entry = candidate("Host a\n  IdentityFile \"/keys/my key\"\n", "a");
        assert_eq!(entry.key_path.as_deref(), Some("/keys/my key"));
    }

    #[test]
    fn glob_matching_handles_stars_questions_and_case() {
        assert!(glob_matches("*.example.com", "web.example.com"));
        assert!(glob_matches("web0?", "WEB01"));
        assert!(glob_matches("*", "anything"));
        assert!(glob_matches("a*b*c", "axxbyyc"));
        assert!(!glob_matches("*.example.com", "example.com"));
        assert!(!glob_matches("web0?", "web001"));
        assert!(!glob_matches("a*b", "acd"));
    }

    #[test]
    fn a_missing_file_is_not_an_empty_one() {
        let listing = listing(std::path::Path::new("/nonexistent/parolassh/config"));
        assert!(!listing.exists);
        assert!(listing.candidates.is_empty());
    }
}
