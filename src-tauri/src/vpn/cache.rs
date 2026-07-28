//! One CLI round per TTL, however many callers ask.
//!
//! Detection spawns up to six local processes, and three things want the
//! answer: the navbar poll every 30 s, the host-row glyphs, and every failed
//! probe's explanation. Without this, a page of unreachable hosts fans out into
//! dozens of `tailscale status` invocations that all say the same thing.
//!
//! Two TTLs, because the two answers age differently. A client going up or down
//! is what the pill exists to show, so statuses stay short-lived; the Twingate
//! resource list is administrator-defined and near-static, so it may sit longer.

use std::future::Future;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

/// How long a detection pass is reused. Under the 30 s poll, so a scheduled
/// poll always does real work; above the burst a single render produces.
pub const STATUS_TTL: Duration = Duration::from_secs(10);

/// The resource list changes when an administrator edits it, not when the
/// network moves, so it earns a much longer life than a status.
pub const RESOURCE_TTL: Duration = Duration::from_secs(300);

/// Whether the caller will accept a cached answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freshness {
    /// A poll or a glyph: reuse whatever is still within the TTL.
    Cached,
    /// The user pressed refresh. Anything loaded before they asked is stale by
    /// definition - but a load that *started* after they asked still counts, so
    /// three impatient clicks cost one CLI round, not three.
    Forced,
}

struct Entry<T> {
    value: T,
    loaded_at: Instant,
}

/// A single value, reloaded at most once per TTL.
pub struct TtlCache<T> {
    /// Held across the load, which is what gives single-flight: a second caller
    /// waits for the first rather than starting its own, then finds the entry
    /// fresh. The wait is bounded by `CLI_TIMEOUT`, so a wedged client delays
    /// callers by three seconds at worst instead of hanging them.
    slot: Mutex<Option<Entry<T>>>,
    ttl: Duration,
}

impl<T: Clone> TtlCache<T> {
    pub const fn new(ttl: Duration) -> Self {
        Self {
            slot: Mutex::const_new(None),
            ttl,
        }
    }

    /// The cached value, loading it first if it is missing or too old.
    pub async fn get<F, Fut>(&self, freshness: Freshness, load: F) -> T
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = T>,
    {
        let asked_at = Instant::now();
        let mut slot = self.slot.lock().await;

        if let Some(entry) = slot.as_ref() {
            let usable = match freshness {
                Freshness::Cached => entry.loaded_at.elapsed() < self.ttl,
                Freshness::Forced => entry.loaded_at >= asked_at,
            };
            if usable {
                return entry.value.clone();
            }
        }

        let value = load().await;
        *slot = Some(Entry {
            value: value.clone(),
            loaded_at: Instant::now(),
        });
        value
    }

    /// Drop the cached value. For the tests, and for anything that knows the
    /// answer has changed underneath us.
    #[cfg(test)]
    pub async fn clear(&self) {
        *self.slot.lock().await = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// A loader that counts its calls, so "did the CLI run?" is assertable
    /// without a VPN client anywhere near the test.
    fn counting(count: &Arc<AtomicUsize>) -> impl Fn() -> std::future::Ready<usize> + '_ {
        move || {
            let n = count.fetch_add(1, Ordering::SeqCst) + 1;
            std::future::ready(n)
        }
    }

    #[tokio::test]
    async fn a_second_caller_inside_the_ttl_runs_no_cli() {
        let cache: TtlCache<usize> = TtlCache::new(Duration::from_secs(60));
        let count = Arc::new(AtomicUsize::new(0));
        let load = counting(&count);

        assert_eq!(cache.get(Freshness::Cached, &load).await, 1);
        assert_eq!(cache.get(Freshness::Cached, &load).await, 1);
        assert_eq!(cache.get(Freshness::Cached, &load).await, 1);
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn an_expired_entry_reloads() {
        let cache: TtlCache<usize> = TtlCache::new(Duration::from_millis(10));
        let count = Arc::new(AtomicUsize::new(0));
        let load = counting(&count);

        assert_eq!(cache.get(Freshness::Cached, &load).await, 1);
        tokio::time::sleep(Duration::from_millis(25)).await;
        assert_eq!(cache.get(Freshness::Cached, &load).await, 2);
    }

    #[tokio::test]
    async fn forcing_ignores_a_fresh_entry() {
        let cache: TtlCache<usize> = TtlCache::new(Duration::from_secs(60));
        let count = Arc::new(AtomicUsize::new(0));
        let load = counting(&count);

        assert_eq!(cache.get(Freshness::Cached, &load).await, 1);
        assert_eq!(cache.get(Freshness::Forced, &load).await, 2);
        // …and the forced result is what the next cached caller sees.
        assert_eq!(cache.get(Freshness::Cached, &load).await, 2);
    }

    #[tokio::test]
    async fn concurrent_callers_share_one_load() {
        let cache: Arc<TtlCache<usize>> = Arc::new(TtlCache::new(Duration::from_secs(60)));
        let count = Arc::new(AtomicUsize::new(0));

        // Every caller arrives before the first load finishes, so single-flight
        // is the only thing that can keep the count at one.
        let mut tasks = Vec::new();
        for _ in 0..8 {
            let cache = Arc::clone(&cache);
            let count = Arc::clone(&count);
            tasks.push(tokio::spawn(async move {
                cache
                    .get(Freshness::Cached, || async {
                        tokio::time::sleep(Duration::from_millis(30)).await;
                        count.fetch_add(1, Ordering::SeqCst) + 1
                    })
                    .await
            }));
        }
        for task in tasks {
            assert_eq!(task.await.unwrap(), 1);
        }
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn impatient_refreshes_collapse_into_one_load() {
        let cache: Arc<TtlCache<usize>> = Arc::new(TtlCache::new(Duration::from_secs(60)));
        let count = Arc::new(AtomicUsize::new(0));

        let mut tasks = Vec::new();
        for _ in 0..4 {
            let cache = Arc::clone(&cache);
            let count = Arc::clone(&count);
            tasks.push(tokio::spawn(async move {
                cache
                    .get(Freshness::Forced, || async {
                        tokio::time::sleep(Duration::from_millis(30)).await;
                        count.fetch_add(1, Ordering::SeqCst) + 1
                    })
                    .await
            }));
        }
        for task in tasks {
            task.await.unwrap();
        }
        // A forced load that began after the click satisfies the click.
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn clearing_forces_the_next_load() {
        let cache: TtlCache<usize> = TtlCache::new(Duration::from_secs(60));
        let count = Arc::new(AtomicUsize::new(0));
        let load = counting(&count);

        assert_eq!(cache.get(Freshness::Cached, &load).await, 1);
        cache.clear().await;
        assert_eq!(cache.get(Freshness::Cached, &load).await, 2);
    }
}
