//! Shared mtime-checked prompt file cache.
//!
//! One implementation of the reload-on-mtime-change pattern that the reprompt
//! engine pioneered, now consumed by the reprompt engine, the augment backend,
//! and any later prompt-file reader. Reads go through `tokio::fs` so async
//! callers never block the runtime on prompt loading.
//!
//! Cache key = path + mtime. Two consequences are part of the contract:
//! - content rewritten in place WITHOUT moving mtime (same clock tick, a
//!   restored/rolled-back mtime, `cp --preserve` over a file) is served stale
//!   by design - the cache never re-reads while mtime matches;
//! - on filesystems where `metadata().modified()` fails, every refresh re-reads
//!   the file (change detection is impossible, so freshness wins over cost).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::SystemTime;
use tokio::sync::RwLock;

/// Outcome of one [`PromptFileCache::refresh`] round.
#[derive(Debug)]
pub enum Refresh {
    /// mtime differed (or was unknown) and the file re-read with non-blank
    /// content; the cache absorbed the new text and mtime.
    Reloaded { text: String, mtime: Option<SystemTime> },
    /// mtime matches the cached one - cached content is current.
    Unchanged,
    /// reload read the file but it is blank; the cache KEPT its previous
    /// content (callers decide whether blank input is acceptable).
    Empty,
    /// stat or read failed; the cache KEPT its previous content.
    Failed(std::io::Error),
}

/// Prompt text plus the mtime of the file it was last read from.
#[derive(Debug, Default)]
pub struct PromptFileCache {
    text: RwLock<String>,
    mtime: RwLock<Option<SystemTime>>,
}

impl PromptFileCache {
    pub fn new(initial_text: impl Into<String>, initial_mtime: Option<SystemTime>) -> Self {
        Self {
            text: RwLock::new(initial_text.into()),
            mtime: RwLock::new(initial_mtime),
        }
    }

    /// Content from the last successful reload.
    pub async fn text(&self) -> String {
        self.text.read().await.clone()
    }

    /// Async stat `path`; reload only when its mtime differs from the cached
    /// one (or either side is unknown). State advances on `Reloaded` only.
    pub async fn refresh(&self, path: &Path) -> Refresh {
        let metadata = match tokio::fs::metadata(path).await {
            Ok(metadata) => metadata,
            Err(e) => return Refresh::Failed(e),
        };
        match metadata.modified().ok() {
            Some(current) => {
                if *self.mtime.read().await == Some(current) {
                    return Refresh::Unchanged;
                }
                self.read_and_store(path, Some(current)).await
            }
            None => self.read_and_store(path, None).await,
        }
    }

    async fn read_and_store(&self, path: &Path, mtime: Option<SystemTime>) -> Refresh {
        match tokio::fs::read_to_string(path).await {
            Ok(text) if text.trim().is_empty() => Refresh::Empty,
            Ok(text) => {
                *self.text.write().await = text.clone();
                *self.mtime.write().await = mtime;
                Refresh::Reloaded { text, mtime }
            }
            Err(e) => Refresh::Failed(e),
        }
    }
}

static SHARED: OnceLock<RwLock<HashMap<PathBuf, Arc<PromptFileCache>>>> = OnceLock::new();

/// Process-wide cache entry for `path` (keyed by the path as given - the same
/// file reached via a different spelling gets its own entry). Lets clients
/// that cannot hold per-instance state share one mtime-checked cache.
pub async fn shared(path: &Path) -> Arc<PromptFileCache> {
    let store = SHARED.get_or_init(|| RwLock::new(HashMap::new()));
    if let Some(hit) = store.read().await.get(path) {
        return Arc::clone(hit);
    }
    let mut store = store.write().await;
    Arc::clone(
        store
            .entry(path.to_path_buf())
            .or_insert_with(|| Arc::new(PromptFileCache::default())),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mtime(seconds: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(seconds)
    }

    fn write_with_mtime(path: &Path, content: &str, modified: SystemTime) {
        std::fs::write(path, content).unwrap();
        let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
        file.set_times(std::fs::FileTimes::new().set_modified(modified)).unwrap();
    }

    fn temp(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("prompt_cache_test_{name}"))
    }

    #[tokio::test]
    async fn first_refresh_reloads_second_is_unchanged() {
        let path = temp("a1");
        write_with_mtime(&path, "V1", mtime(1_000));
        let cache = PromptFileCache::default();

        let first = cache.refresh(&path).await;
        assert!(
            matches!(&first, Refresh::Reloaded { text, mtime: Some(m) } if text == "V1" && *m == mtime(1_000)),
            "first refresh must load: {first:?}"
        );
        assert_eq!(cache.text().await, "V1");
        assert!(matches!(cache.refresh(&path).await, Refresh::Unchanged));

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn mtime_bump_is_reloaded_without_restarting_the_process() {
        let path = temp("a2");
        write_with_mtime(&path, "V1", mtime(1_000));
        let cache = PromptFileCache::default();
        cache.refresh(&path).await;

        write_with_mtime(&path, "V2", mtime(2_000));
        let second = cache.refresh(&path).await;
        assert!(
            matches!(&second, Refresh::Reloaded { text, mtime: Some(m) } if text == "V2" && *m == mtime(2_000)),
            "bumped mtime must reload: {second:?}"
        );
        assert_eq!(cache.text().await, "V2");

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn blank_reload_reports_empty_and_keeps_previous_state() {
        let path = temp("a3");
        write_with_mtime(&path, "Good prompt.", mtime(1_000));
        let cache = PromptFileCache::default();
        cache.refresh(&path).await;

        write_with_mtime(&path, "   \n ", mtime(2_000));
        assert!(matches!(cache.refresh(&path).await, Refresh::Empty));
        assert_eq!(cache.text().await, "Good prompt.", "blank file must not wipe the cache");

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn absent_file_reports_failed_with_not_found_and_keeps_previous() {
        let path = temp("a4");
        write_with_mtime(&path, "Cached.", mtime(1_000));
        let cache = PromptFileCache::default();
        cache.refresh(&path).await;
        std::fs::remove_file(&path).unwrap();

        let missing = match cache.refresh(&path).await {
            Refresh::Failed(e) => e,
            other => panic!("absent file must report Failed, got {other:?}"),
        };
        assert_eq!(missing.kind(), std::io::ErrorKind::NotFound);
        assert_eq!(cache.text().await, "Cached.");
    }

    #[tokio::test]
    async fn restored_mtime_over_new_content_is_the_documented_stale_read() {
        let path = temp("a5");
        write_with_mtime(&path, "V1", mtime(1_000));
        let cache = PromptFileCache::default();
        cache.refresh(&path).await;

        std::fs::write(&path, "V2").unwrap();
        let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        file.set_times(std::fs::FileTimes::new().set_modified(mtime(1_000))).unwrap();

        assert!(
            matches!(cache.refresh(&path).await, Refresh::Unchanged),
            "documented limitation: path+mtime unchanged = cached content wins"
        );
        assert_eq!(cache.text().await, "V1");

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn shared_hands_one_cache_per_path() {
        let path = temp("a6");
        write_with_mtime(&path, "Shared.", mtime(1_000));

        let a = shared(&path).await;
        a.refresh(&path).await;
        let b = shared(&path).await;
        assert!(Arc::ptr_eq(&a, &b), "same path must share one cache");
        assert_eq!(b.text().await, "Shared.", "the second handle sees the first's reload");

        let other = shared(&temp("a7")).await;
        assert!(!Arc::ptr_eq(&a, &other), "different paths are different caches");

        let _ = std::fs::remove_file(&path);
    }
}
