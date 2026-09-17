//! The last Twingate resource list the client actually gave us.
//!
//! `twingate resources` only answers while the service runs, so the list is
//! empty exactly when a failed connection most needs it: a stopped Twingate is
//! the usual reason a resource address stops answering. The last good answer is
//! kept in memory and on disk (owner-only, beside `hosts.json`), so it also
//! survives starting the app with Twingate already off.
//!
//! An empty list only replaces it when the client is online and answered - that
//! is the administrator removing access, not the service being down.

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

use super::twingate::TwingateResource;
use crate::private_file;

const FILE_NAME: &str = "twingate-resources.json";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LastKnown {
    /// ISO 8601 UTC of the answer this came from.
    pub seen_at: String,
    pub resources: Vec<TwingateResource>,
}

/// The list to use, and whether it is live or remembered.
#[derive(Debug, Clone, PartialEq)]
pub struct KnownResources {
    pub resources: Vec<TwingateResource>,
    /// When the list was last seen live; `None` when it is live now.
    pub remembered_from: Option<String>,
}

/// What the stored copy should become.
#[derive(Debug, PartialEq)]
enum Update {
    Keep,
    Save(LastKnown),
    Forget,
}

/// Decide between the live answer and the remembered one.
///
/// `live` is `None` when the client could not list at all. `online` is the
/// client's own status, which is what makes an empty answer believable.
fn reconcile(
    live: Option<Vec<TwingateResource>>,
    online: bool,
    remembered: Option<&LastKnown>,
    now: &str,
) -> (KnownResources, Update) {
    match live {
        Some(list) if !list.is_empty() => {
            let update = match remembered {
                Some(known) if known.resources == list => Update::Keep,
                _ => Update::Save(LastKnown { seen_at: now.to_string(), resources: list.clone() }),
            };
            (KnownResources { resources: list, remembered_from: None }, update)
        }
        Some(_) if online => {
            let update = if remembered.is_some() { Update::Forget } else { Update::Keep };
            (KnownResources { resources: Vec::new(), remembered_from: None }, update)
        }
        _ => {
            let known = match remembered {
                Some(known) => KnownResources {
                    resources: known.resources.clone(),
                    remembered_from: Some(known.seen_at.clone()),
                },
                None => KnownResources { resources: Vec::new(), remembered_from: None },
            };
            (known, Update::Keep)
        }
    }
}

static CONFIG_DIR: OnceLock<PathBuf> = OnceLock::new();

/// `None` until first read; then the stored copy, loaded from disk once.
static MEMORY: Mutex<Option<Option<LastKnown>>> = Mutex::new(None);

/// Where the copy is kept. Until this is called it lives in memory only.
pub fn init(config_dir: PathBuf) {
    let _ = CONFIG_DIR.set(config_dir);
}

/// Fold a fresh answer into the remembered list and return what to use.
pub fn resolve(live: Option<Vec<TwingateResource>>, online: bool) -> KnownResources {
    let Ok(mut memory) = MEMORY.lock() else {
        return KnownResources { resources: live.unwrap_or_default(), remembered_from: None };
    };
    let stored = memory.get_or_insert_with(load);

    let now = crate::hosts::store::now_iso8601();
    let (known, update) = reconcile(live, online, stored.as_ref(), &now);

    match update {
        Update::Keep => {}
        Update::Save(last) => {
            save(Some(&last));
            *stored = Some(last);
        }
        Update::Forget => {
            save(None);
            *stored = None;
        }
    }
    known
}

fn load() -> Option<LastKnown> {
    let dir = CONFIG_DIR.get()?;
    let text = std::fs::read_to_string(dir.join(FILE_NAME)).ok()?;
    serde_json::from_str(&text).ok()
}

/// Best-effort: failing to persist only costs the list across a restart.
fn save(last: Option<&LastKnown>) {
    let Some(dir) = CONFIG_DIR.get() else { return };
    match last {
        Some(last) => {
            if let Ok(text) = serde_json::to_string_pretty(last) {
                let _ = private_file::write(dir, FILE_NAME, &text);
            }
        }
        None => {
            let _ = std::fs::remove_file(dir.join(FILE_NAME));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: &str = "2026-09-17T12:00:00Z";
    const EARLIER: &str = "2026-09-10T08:00:00Z";

    fn resource(name: &str, address: &str) -> TwingateResource {
        TwingateResource {
            name: name.to_string(),
            address: address.to_string(),
            alias: None,
            auth_status: "Auth expires in 4 days".to_string(),
        }
    }

    fn remembered(resources: Vec<TwingateResource>) -> LastKnown {
        LastKnown { seen_at: EARLIER.to_string(), resources }
    }

    #[test]
    fn a_live_list_is_used_and_remembered() {
        let list = vec![resource("acme", "192.168.9.0/24")];
        let (known, update) = reconcile(Some(list.clone()), true, None, NOW);

        assert_eq!(known, KnownResources { resources: list.clone(), remembered_from: None });
        assert_eq!(update, Update::Save(LastKnown { seen_at: NOW.to_string(), resources: list }));
    }

    #[test]
    fn an_unchanged_list_is_not_rewritten() {
        let list = vec![resource("acme", "192.168.9.0/24")];
        let stored = remembered(list.clone());
        let (_, update) = reconcile(Some(list), true, Some(&stored), NOW);
        assert_eq!(update, Update::Keep);
    }

    #[test]
    fn a_stopped_service_falls_back_to_the_remembered_list() {
        let stored = remembered(vec![resource("acme", "192.168.9.0/24")]);
        let (known, update) = reconcile(None, false, Some(&stored), NOW);

        assert_eq!(known.resources, stored.resources);
        assert_eq!(known.remembered_from.as_deref(), Some(EARLIER));
        assert_eq!(update, Update::Keep);
    }

    #[test]
    fn an_empty_answer_while_offline_is_not_believed() {
        let stored = remembered(vec![resource("acme", "192.168.9.0/24")]);
        let (known, update) = reconcile(Some(Vec::new()), false, Some(&stored), NOW);

        assert_eq!(known.resources.len(), 1);
        assert_eq!(update, Update::Keep);
    }

    #[test]
    fn an_empty_answer_while_online_clears_the_memory() {
        let stored = remembered(vec![resource("acme", "192.168.9.0/24")]);
        let (known, update) = reconcile(Some(Vec::new()), true, Some(&stored), NOW);

        assert!(known.resources.is_empty());
        assert_eq!(known.remembered_from, None);
        assert_eq!(update, Update::Forget);
    }

    #[test]
    fn a_timed_out_listing_while_online_keeps_the_memory() {
        let stored = remembered(vec![resource("acme", "192.168.9.0/24")]);
        let (known, update) = reconcile(None, true, Some(&stored), NOW);

        assert_eq!(known.resources.len(), 1);
        assert_eq!(update, Update::Keep);
    }

    #[test]
    fn nothing_live_and_nothing_remembered_is_empty() {
        let (known, update) = reconcile(None, false, None, NOW);
        assert!(known.resources.is_empty());
        assert_eq!(known.remembered_from, None);
        assert_eq!(update, Update::Keep);
    }

    #[test]
    fn the_stored_copy_round_trips_through_json() {
        let stored = remembered(vec![TwingateResource {
            alias: Some("db.acme.internal".to_string()),
            ..resource("acme", "10.0.0.0/16")
        }]);
        let text = serde_json::to_string(&stored).unwrap();
        assert_eq!(serde_json::from_str::<LastKnown>(&text).unwrap(), stored);
    }
}
