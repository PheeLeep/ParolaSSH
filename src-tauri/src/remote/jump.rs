//! Resolving a host's jump chain.
//!
//! `proxy_jump` holds the id of another saved connection, not an ssh_config
//! string, so a jump host is reached with the credentials and host key policy
//! it already has. Nothing here connects; it only works out the order.

use crate::hosts::model::HostRecord;
use crate::ssh::{SshError, SshResult};

/// How many jump hosts may stand between us and the target.
///
/// Each hop is a full handshake through the previous one, and a chain this
/// long is far more likely to be a mistake than a topology.
pub const MAX_HOPS: usize = 4;

/// The jump hosts to connect through, outermost first. Empty for a direct
/// connection. The target itself is not included.
pub fn resolve(hosts: &[HostRecord], target_id: &str) -> SshResult<Vec<HostRecord>> {
    let mut chain: Vec<HostRecord> = Vec::new();
    let mut visited = vec![target_id.to_string()];
    let mut current = target_id.to_string();

    loop {
        let host = hosts
            .iter()
            .find(|host| host.id == current)
            .ok_or_else(|| SshError::invalid("That connection no longer exists."))?;

        let Some(next) = host.proxy_jump.clone().filter(|id| !id.is_empty()) else {
            break;
        };

        if visited.contains(&next) {
            return Err(SshError::invalid(format!(
                "The jump hosts for “{}” loop back on themselves. Edit one of them and clear its jump host.",
                host.label
            )));
        }

        if !hosts.iter().any(|candidate| candidate.id == next) {
            return Err(SshError::invalid(format!(
                "“{}” jumps through a connection that has been deleted. Edit it and choose another.",
                host.label
            )));
        }

        if chain.len() == MAX_HOPS {
            return Err(SshError::invalid(format!(
                "That is more than {MAX_HOPS} jump hosts deep. Shorten the chain."
            )));
        }

        visited.push(next.clone());
        current = next;

        let hop = hosts
            .iter()
            .find(|host| host.id == current)
            .expect("presence checked above")
            .clone();
        chain.push(hop);
    }

    // Built target-outwards; connecting goes the other way.
    chain.reverse();
    Ok(chain)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hosts::model::AuthMethod;

    fn host(id: &str, jump: Option<&str>) -> HostRecord {
        HostRecord {
            id: id.into(),
            label: id.into(),
            hostname: format!("{id}.example.com"),
            port: 22,
            username: "operator".into(),
            auth_method: AuthMethod::Agent,
            key_path: None,
            group: "Lab".into(),
            tags: Vec::new(),
            notes: None,
            proxy_jump: jump.map(String::from),
            last_connected: None,
        }
    }

    fn ids(chain: &[HostRecord]) -> Vec<&str> {
        chain.iter().map(|host| host.id.as_str()).collect()
    }

    #[test]
    fn no_jump_host_is_a_direct_connection() {
        let hosts = vec![host("target", None)];
        assert!(resolve(&hosts, "target").unwrap().is_empty());
    }

    #[test]
    fn chain_is_ordered_outermost_first() {
        // target -> inner -> outer, so we dial outer first.
        let hosts = vec![
            host("target", Some("inner")),
            host("inner", Some("outer")),
            host("outer", None),
        ];
        assert_eq!(ids(&resolve(&hosts, "target").unwrap()), vec!["outer", "inner"]);
    }

    #[test]
    fn a_loop_is_refused_rather_than_dialled_forever() {
        let hosts = vec![host("a", Some("b")), host("b", Some("a"))];
        assert!(resolve(&hosts, "a").is_err());
    }

    #[test]
    fn a_host_jumping_through_itself_is_a_loop() {
        let hosts = vec![host("a", Some("a"))];
        assert!(resolve(&hosts, "a").is_err());
    }

    #[test]
    fn a_deleted_jump_host_is_named_not_ignored() {
        let hosts = vec![host("a", Some("gone"))];
        let error = resolve(&hosts, "a").unwrap_err().to_string();
        assert!(error.contains("deleted"), "{error}");
    }

    #[test]
    fn too_many_hops_is_refused() {
        let mut hosts = vec![host("h0", Some("h1"))];
        for index in 1..=MAX_HOPS {
            hosts.push(host(&format!("h{index}"), Some(&format!("h{}", index + 1))));
        }
        hosts.push(host(&format!("h{}", MAX_HOPS + 1), None));
        assert!(resolve(&hosts, "h0").is_err());
    }

    #[test]
    fn exactly_the_maximum_is_allowed() {
        let mut hosts = vec![host("h0", Some("h1"))];
        for index in 1..MAX_HOPS {
            hosts.push(host(&format!("h{index}"), Some(&format!("h{}", index + 1))));
        }
        hosts.push(host(&format!("h{MAX_HOPS}"), None));
        assert_eq!(resolve(&hosts, "h0").unwrap().len(), MAX_HOPS);
    }

    #[test]
    fn an_empty_jump_id_is_no_jump_host() {
        let hosts = vec![host("a", Some(""))];
        assert!(resolve(&hosts, "a").unwrap().is_empty());
    }
}
