//! The app-wide file transfer queue.
//!
//! Unlike everything else in `remote`, this is not per-host. One queue serves
//! every connection, because the thing worth rationing is the user's uplink,
//! not any single server's patience: five downloads from five hosts saturate a
//! home connection exactly as five from one would. It is the first piece of
//! process-global state besides the session registry and the secret vault.
//!
//! Scheduling is priority-then-arrival. `High` before `Normal` before `Low`,
//! and within a level the one that was asked for first, so a queue left alone
//! drains in the order it was filled. Only `max_concurrent` transfers run at
//! once; the rest sit in `Queued` with a visible position.
//!
//! Locking follows `registry.rs`: the `std::sync::Mutex` around the records is
//! never held across an `await`. The one `tokio::sync::Mutex` is `pump`, held
//! deliberately while slots are handed out so two finishing transfers cannot
//! both decide the same slot is free.
//!
//! Nothing here survives a restart. A failed or cancelled transfer stays in the
//! list as history until the user clears it, which is what makes "it stopped
//! because the host went away" visible rather than a row that silently vanishes.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

/// Slots used when the user has never chosen. Three keeps a link busy without
/// making any one transfer crawl.
pub const DEFAULT_MAX_CONCURRENT: usize = 3;
pub const MIN_MAX_CONCURRENT: usize = 1;
pub const MAX_MAX_CONCURRENT: usize = 8;

/// How many finished rows to keep before dropping the oldest. History is a
/// convenience, not a log; an unbounded one is just a leak with a nice name.
const MAX_HISTORY: usize = 200;

static NEXT_TRANSFER_ID: AtomicU64 = AtomicU64::new(1);
/// Arrival order, so ties within a priority level break by who asked first.
static NEXT_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Upload,
    Download,
}

/// Ordered worst-to-best so `Ord` puts `High` last; the scheduler takes the
/// maximum. Deriving the order rather than writing a comparator means the
/// enum's declaration *is* the policy.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    Low,
    #[default]
    Normal,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TransferState {
    Queued,
    Running,
    Done,
    Failed,
    Canceled,
}

impl TransferState {
    /// Whether this state still occupies a slot or a place in the queue.
    pub fn is_pending(self) -> bool {
        matches!(self, Self::Queued | Self::Running)
    }
}

/// One transfer, from the moment it is asked for to the moment it is cleared.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferRecord {
    pub id: u64,
    pub host_id: String,
    /// Denormalized so the Transfers page can name a host it has since
    /// disconnected from, without holding the host list.
    pub host_label: String,
    pub direction: Direction,
    pub remote_path: String,
    pub local_path: String,
    /// What the row is called — the file name, not the whole path.
    pub name: String,
    pub priority: Priority,
    pub state: TransferState,
    pub bytes_done: u64,
    /// `None` until the size is known, which for an upload is immediate and for
    /// a download takes the opening stat.
    pub bytes_total: Option<u64>,
    /// Its place among everything still waiting, 1-based. `None` once running.
    pub queue_position: Option<usize>,
    pub error: Option<String>,
    pub queued_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    #[serde(skip)]
    sequence: u64,
    /// Checked between chunks by the running task. Shared rather than signalled
    /// so a cancel takes effect inside one chunk without a channel per transfer.
    #[serde(skip)]
    cancel: Arc<AtomicBool>,
}

impl TransferRecord {
    pub fn cancel_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.cancel)
    }

    pub fn is_canceled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

/// What a caller must supply to enqueue.
pub struct TransferRequest {
    pub host_id: String,
    pub host_label: String,
    pub direction: Direction,
    pub remote_path: String,
    pub local_path: String,
    pub name: String,
    pub priority: Priority,
    pub bytes_total: Option<u64>,
}

/// A transfer the scheduler has decided should start now.
pub struct StartOrder {
    pub id: u64,
    pub host_id: String,
    pub direction: Direction,
    pub remote_path: String,
    pub local_path: String,
    pub cancel: Arc<AtomicBool>,
}

#[derive(Default)]
pub struct TransferManager {
    records: Mutex<HashMap<u64, TransferRecord>>,
    max_concurrent: AtomicUsize,
    /// Held while slots are handed out. See the module docs.
    pump: tokio::sync::Mutex<()>,
}

impl TransferManager {
    pub fn new() -> Self {
        Self {
            records: Mutex::new(HashMap::new()),
            max_concurrent: AtomicUsize::new(DEFAULT_MAX_CONCURRENT),
            pump: tokio::sync::Mutex::new(()),
        }
    }

    pub fn max_concurrent(&self) -> usize {
        let stored = self.max_concurrent.load(Ordering::Relaxed);
        if stored == 0 {
            DEFAULT_MAX_CONCURRENT
        } else {
            stored
        }
    }

    /// Change the cap. Raising it frees slots on the next pump; lowering it
    /// never interrupts a transfer already running — those finish, and the
    /// smaller cap takes hold as they do.
    pub fn set_max_concurrent(&self, value: usize) -> usize {
        let clamped = value.clamp(MIN_MAX_CONCURRENT, MAX_MAX_CONCURRENT);
        self.max_concurrent.store(clamped, Ordering::Relaxed);
        clamped
    }

    /// Add a transfer in `Queued`. It starts only when `take_ready` picks it.
    pub fn enqueue(&self, request: TransferRequest) -> u64 {
        let id = NEXT_TRANSFER_ID.fetch_add(1, Ordering::Relaxed);
        let record = TransferRecord {
            id,
            host_id: request.host_id,
            host_label: request.host_label,
            direction: request.direction,
            remote_path: request.remote_path,
            local_path: request.local_path,
            name: request.name,
            priority: request.priority,
            state: TransferState::Queued,
            bytes_done: 0,
            bytes_total: request.bytes_total,
            queue_position: None,
            error: None,
            queued_at: now(),
            started_at: None,
            finished_at: None,
            sequence: NEXT_SEQUENCE.fetch_add(1, Ordering::Relaxed),
            cancel: Arc::new(AtomicBool::new(false)),
        };

        if let Ok(mut records) = self.records.lock() {
            records.insert(id, record);
            prune_history(&mut records);
        }
        id
    }

    /// Hand out as many start orders as there are free slots.
    ///
    /// `connected` answers whether a host still has a session — a transfer for
    /// a host that went away is never started, it is failed by `fail_host`.
    /// Callers must hold the pump guard, which `lock_pump` provides.
    pub fn take_ready(&self, connected: &dyn Fn(&str) -> bool) -> Vec<StartOrder> {
        let mut orders = Vec::new();
        let Ok(mut records) = self.records.lock() else {
            return orders;
        };

        let cap = self.max_concurrent();
        let mut running = records
            .values()
            .filter(|record| record.state == TransferState::Running)
            .count();

        while running < cap {
            // Highest priority, then earliest arrival. A linear scan is right
            // here: the queue is tens of entries at most, and a heap would need
            // rebuilding on every priority change.
            let next = records
                .values()
                .filter(|record| record.state == TransferState::Queued)
                .filter(|record| connected(&record.host_id))
                .min_by_key(|record| (std::cmp::Reverse(record.priority), record.sequence))
                .map(|record| record.id);

            let Some(id) = next else { break };
            let Some(record) = records.get_mut(&id) else {
                break;
            };

            record.state = TransferState::Running;
            record.started_at = Some(now());
            record.queue_position = None;
            orders.push(StartOrder {
                id: record.id,
                host_id: record.host_id.clone(),
                direction: record.direction,
                remote_path: record.remote_path.clone(),
                local_path: record.local_path.clone(),
                cancel: record.cancel_flag(),
            });
            running += 1;
        }

        if !orders.is_empty() {
            renumber(&mut records);
        }
        orders
    }

    /// Serialises slot handout. Held across the spawn of every promoted
    /// transfer, so a completion racing an enqueue cannot overfill the cap.
    pub async fn lock_pump(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.pump.lock().await
    }

    pub fn record_progress(&self, id: u64, bytes_done: u64, bytes_total: Option<u64>) {
        if let Ok(mut records) = self.records.lock() {
            if let Some(record) = records.get_mut(&id) {
                record.bytes_done = bytes_done;
                if bytes_total.is_some() {
                    record.bytes_total = bytes_total;
                }
            }
        }
    }

    /// Settle a transfer. `error` of `None` means it succeeded.
    pub fn finish(&self, id: u64, error: Option<String>) {
        let Ok(mut records) = self.records.lock() else {
            return;
        };
        if let Some(record) = records.get_mut(&id) {
            record.state = match (&error, record.is_canceled()) {
                (_, true) => TransferState::Canceled,
                (Some(_), false) => TransferState::Failed,
                (None, false) => TransferState::Done,
            };
            // A cancelled transfer's error is the cancellation, which the state
            // already says; keeping the I/O error too would read as a fault.
            record.error = if record.state == TransferState::Canceled {
                None
            } else {
                error
            };
            record.finished_at = Some(now());
            record.queue_position = None;
            if record.state == TransferState::Done {
                if let Some(total) = record.bytes_total {
                    record.bytes_done = total;
                }
            }
        }
        renumber(&mut records);
    }

    /// Ask a transfer to stop. A queued one settles immediately; a running one
    /// notices at its next chunk boundary and settles itself through `finish`.
    pub fn cancel(&self, id: u64) -> bool {
        let Ok(mut records) = self.records.lock() else {
            return false;
        };
        let Some(record) = records.get_mut(&id) else {
            return false;
        };
        if !record.state.is_pending() {
            return false;
        }

        record.cancel.store(true, Ordering::Relaxed);
        if record.state == TransferState::Queued {
            record.state = TransferState::Canceled;
            record.finished_at = Some(now());
            record.queue_position = None;
        }
        renumber(&mut records);
        true
    }

    /// Fail everything belonging to a host that has gone away.
    ///
    /// Queued entries are failed rather than left parked: a queue that silently
    /// resumes on some later reconnect is a surprise, and a row that disappears
    /// is worse. Both end up visible, with the reason attached.
    pub fn fail_host(&self, host_id: &str, reason: &str) -> usize {
        let Ok(mut records) = self.records.lock() else {
            return 0;
        };
        let mut affected = 0;

        for record in records.values_mut() {
            if record.host_id != host_id || !record.state.is_pending() {
                continue;
            }
            record.cancel.store(true, Ordering::Relaxed);
            record.state = TransferState::Failed;
            record.error = Some(reason.to_string());
            record.finished_at = Some(now());
            record.queue_position = None;
            affected += 1;
        }

        renumber(&mut records);
        affected
    }

    /// Re-rank a waiting transfer. Running ones are left alone — their slot is
    /// already spent, so the level would only affect a re-queue that never
    /// happens.
    pub fn set_priority(&self, id: u64, priority: Priority) -> bool {
        let Ok(mut records) = self.records.lock() else {
            return false;
        };
        let Some(record) = records.get_mut(&id) else {
            return false;
        };
        if record.state != TransferState::Queued {
            return false;
        }
        record.priority = priority;
        renumber(&mut records);
        true
    }

    /// Every transfer, newest first — what the Transfers page lists.
    pub fn snapshot(&self) -> Vec<TransferRecord> {
        let Ok(records) = self.records.lock() else {
            return Vec::new();
        };
        let mut all: Vec<TransferRecord> = records.values().cloned().collect();
        all.sort_by_key(|record| std::cmp::Reverse(record.sequence));
        all
    }

    pub fn get(&self, id: u64) -> Option<TransferRecord> {
        self.records
            .lock()
            .ok()
            .and_then(|records| records.get(&id).cloned())
    }

    /// How many are running and how many are waiting, for the sidebar badge.
    pub fn pending_counts(&self) -> (usize, usize) {
        let Ok(records) = self.records.lock() else {
            return (0, 0);
        };
        let running = records
            .values()
            .filter(|r| r.state == TransferState::Running)
            .count();
        let queued = records
            .values()
            .filter(|r| r.state == TransferState::Queued)
            .count();
        (running, queued)
    }

    /// Drop settled rows. Anything still pending stays.
    pub fn clear_finished(&self) -> usize {
        let Ok(mut records) = self.records.lock() else {
            return 0;
        };
        let before = records.len();
        records.retain(|_, record| record.state.is_pending());
        before - records.len()
    }
}

/// Recompute the 1-based position of everything still queued, in the order the
/// scheduler would take them. Called after any change that could reorder.
fn renumber(records: &mut HashMap<u64, TransferRecord>) {
    let mut queued: Vec<(u64, Priority, u64)> = records
        .values()
        .filter(|record| record.state == TransferState::Queued)
        .map(|record| (record.id, record.priority, record.sequence))
        .collect();

    queued.sort_by_key(|(_, priority, sequence)| (std::cmp::Reverse(*priority), *sequence));

    for (position, (id, _, _)) in queued.into_iter().enumerate() {
        if let Some(record) = records.get_mut(&id) {
            record.queue_position = Some(position + 1);
        }
    }
}

/// Keep history bounded by dropping the oldest settled rows.
fn prune_history(records: &mut HashMap<u64, TransferRecord>) {
    let mut finished: Vec<(u64, u64)> = records
        .values()
        .filter(|record| !record.state.is_pending())
        .map(|record| (record.sequence, record.id))
        .collect();

    if finished.len() <= MAX_HISTORY {
        return;
    }

    finished.sort_unstable();
    let excess = finished.len() - MAX_HISTORY;
    for (_, id) in finished.into_iter().take(excess) {
        records.remove(&id);
    }
}

/// ISO-8601-ish UTC, matching how `connected_at` is stamped elsewhere.
fn now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or(0);
    format!("{seconds}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn always_connected(_: &str) -> bool {
        true
    }

    fn request(host: &str, name: &str, priority: Priority) -> TransferRequest {
        TransferRequest {
            host_id: host.to_string(),
            host_label: host.to_string(),
            direction: Direction::Download,
            remote_path: format!("/tmp/{name}"),
            local_path: format!("/home/me/{name}"),
            name: name.to_string(),
            priority,
            bytes_total: Some(1024),
        }
    }

    fn started_names(orders: &[StartOrder]) -> Vec<String> {
        orders
            .iter()
            .map(|order| order.remote_path.replace("/tmp/", ""))
            .collect()
    }

    #[test]
    fn the_cap_is_never_exceeded() {
        let manager = TransferManager::new();
        for index in 0..10 {
            manager.enqueue(request("h1", &format!("f{index}"), Priority::Normal));
        }

        let orders = manager.take_ready(&always_connected);
        assert_eq!(orders.len(), DEFAULT_MAX_CONCURRENT);
        assert_eq!(manager.pending_counts(), (3, 7));

        // A second pump with nothing finished hands out nothing more.
        assert!(manager.take_ready(&always_connected).is_empty());
    }

    #[test]
    fn high_priority_jumps_the_queue_and_ties_break_by_arrival() {
        let manager = TransferManager::new();
        manager.set_max_concurrent(1);

        manager.enqueue(request("h1", "first", Priority::Normal));
        manager.enqueue(request("h1", "second", Priority::Normal));
        let urgent = manager.enqueue(request("h1", "urgent", Priority::High));
        manager.enqueue(request("h1", "later", Priority::Low));

        let first = manager.take_ready(&always_connected);
        assert_eq!(started_names(&first), ["urgent"]);
        assert_eq!(urgent, manager.get(urgent).unwrap().id);

        manager.finish(urgent, None);
        assert_eq!(started_names(&manager.take_ready(&always_connected)), ["first"]);
    }

    #[test]
    fn bumping_a_queued_transfer_moves_it_to_the_front() {
        let manager = TransferManager::new();
        manager.set_max_concurrent(1);

        let head = manager.enqueue(request("h1", "head", Priority::Normal));
        manager.enqueue(request("h1", "middle", Priority::Normal));
        let tail = manager.enqueue(request("h1", "tail", Priority::Normal));

        manager.take_ready(&always_connected); // head starts

        assert_eq!(manager.get(tail).unwrap().queue_position, Some(2));
        assert!(manager.set_priority(tail, Priority::High));
        assert_eq!(manager.get(tail).unwrap().queue_position, Some(1));

        manager.finish(head, None);
        assert_eq!(started_names(&manager.take_ready(&always_connected)), ["tail"]);
    }

    #[test]
    fn a_running_transfer_cannot_be_reprioritised() {
        let manager = TransferManager::new();
        let id = manager.enqueue(request("h1", "busy", Priority::Normal));
        manager.take_ready(&always_connected);

        assert!(!manager.set_priority(id, Priority::High));
        assert_eq!(manager.get(id).unwrap().priority, Priority::Normal);
    }

    #[test]
    fn queue_positions_are_contiguous_and_only_for_waiting_rows() {
        let manager = TransferManager::new();
        manager.set_max_concurrent(1);
        for index in 0..4 {
            manager.enqueue(request("h1", &format!("f{index}"), Priority::Normal));
        }
        manager.take_ready(&always_connected);

        let mut positions: Vec<usize> = manager
            .snapshot()
            .iter()
            .filter_map(|record| record.queue_position)
            .collect();
        positions.sort_unstable();
        assert_eq!(positions, [1, 2, 3]);

        let running = manager
            .snapshot()
            .into_iter()
            .find(|record| record.state == TransferState::Running)
            .unwrap();
        assert_eq!(running.queue_position, None);
    }

    #[test]
    fn a_disconnected_host_never_starts_and_its_queue_is_failed() {
        let manager = TransferManager::new();
        manager.enqueue(request("gone", "a", Priority::High));
        let mine = manager.enqueue(request("here", "b", Priority::Normal));

        let orders = manager.take_ready(&|host| host == "here");
        assert_eq!(orders.len(), 1);
        assert_eq!(orders[0].id, mine);

        assert_eq!(manager.fail_host("gone", "The host disconnected."), 1);
        let failed = manager
            .snapshot()
            .into_iter()
            .find(|record| record.host_id == "gone")
            .unwrap();
        assert_eq!(failed.state, TransferState::Failed);
        assert_eq!(failed.error.as_deref(), Some("The host disconnected."));
    }

    #[test]
    fn failing_a_host_stops_its_running_transfers_too() {
        let manager = TransferManager::new();
        let id = manager.enqueue(request("h1", "big", Priority::Normal));
        let orders = manager.take_ready(&always_connected);
        let cancel = orders[0].cancel.clone();

        manager.fail_host("h1", "The host disconnected.");

        assert!(cancel.load(Ordering::Relaxed), "the task must be told to stop");
        assert_eq!(manager.get(id).unwrap().state, TransferState::Failed);
        assert_eq!(manager.pending_counts(), (0, 0));
    }

    #[test]
    fn cancelling_a_queued_transfer_settles_it_immediately() {
        let manager = TransferManager::new();
        manager.set_max_concurrent(1);
        manager.enqueue(request("h1", "running", Priority::Normal));
        let waiting = manager.enqueue(request("h1", "waiting", Priority::Normal));
        manager.take_ready(&always_connected);

        assert!(manager.cancel(waiting));
        assert_eq!(manager.get(waiting).unwrap().state, TransferState::Canceled);
        // Already settled, so a second cancel is a no-op rather than an error.
        assert!(!manager.cancel(waiting));
    }

    #[test]
    fn cancelling_a_running_transfer_flags_it_and_finish_honours_the_flag() {
        let manager = TransferManager::new();
        let id = manager.enqueue(request("h1", "big", Priority::Normal));
        manager.take_ready(&always_connected);

        assert!(manager.cancel(id));
        // Still running until the task notices.
        assert_eq!(manager.get(id).unwrap().state, TransferState::Running);

        // The task reports the I/O error the cancel caused; it reads as a
        // cancellation, not a failure.
        manager.finish(id, Some("stream closed".into()));
        let record = manager.get(id).unwrap();
        assert_eq!(record.state, TransferState::Canceled);
        assert_eq!(record.error, None);
    }

    #[test]
    fn raising_the_cap_starts_more_and_lowering_it_spares_the_running() {
        let manager = TransferManager::new();
        manager.set_max_concurrent(1);
        for index in 0..4 {
            manager.enqueue(request("h1", &format!("f{index}"), Priority::Normal));
        }
        assert_eq!(manager.take_ready(&always_connected).len(), 1);

        manager.set_max_concurrent(3);
        assert_eq!(manager.take_ready(&always_connected).len(), 2);
        assert_eq!(manager.pending_counts(), (3, 1));

        manager.set_max_concurrent(1);
        assert_eq!(manager.pending_counts().0, 3, "running transfers are not killed");
        assert!(manager.take_ready(&always_connected).is_empty());
    }

    #[test]
    fn the_cap_is_clamped_to_a_sane_range() {
        let manager = TransferManager::new();
        assert_eq!(manager.set_max_concurrent(0), MIN_MAX_CONCURRENT);
        assert_eq!(manager.set_max_concurrent(999), MAX_MAX_CONCURRENT);
        assert_eq!(manager.set_max_concurrent(4), 4);
        assert_eq!(manager.max_concurrent(), 4);
    }

    #[test]
    fn a_finished_transfer_reads_as_complete() {
        let manager = TransferManager::new();
        let id = manager.enqueue(request("h1", "f", Priority::Normal));
        manager.take_ready(&always_connected);
        manager.record_progress(id, 512, Some(1024));
        manager.finish(id, None);

        let record = manager.get(id).unwrap();
        assert_eq!(record.state, TransferState::Done);
        assert_eq!(record.bytes_done, 1024, "a done transfer shows as full");
        assert!(record.finished_at.is_some());
    }

    #[test]
    fn clearing_finished_keeps_whatever_is_still_pending() {
        let manager = TransferManager::new();
        let done = manager.enqueue(request("h1", "done", Priority::Normal));
        let waiting = manager.enqueue(request("h1", "waiting", Priority::Normal));
        manager.set_max_concurrent(1);
        manager.take_ready(&always_connected);
        manager.finish(done, None);

        assert_eq!(manager.clear_finished(), 1);
        let remaining = manager.snapshot();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].id, waiting);
    }
}
