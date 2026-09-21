//! Load balancer trait and BackendGuard

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use super::node::BackendNode;
use crate::config::NoMatchingBackend;

/// Shared busy-count claim for exactly one backend slot.
///
/// One handle exists per claim; `Arc` clones are shared by every clone of the
/// owning `BackendGuard`, so the decrement runs exactly once — when the last
/// handle drops. For streamed responses that clone is parked in the response
/// body, so release happens at body end (completion or client disconnect),
/// not at handler return.
#[derive(Debug)]
struct BusyHandle(Arc<AtomicUsize>);

impl Drop for BusyHandle {
    fn drop(&mut self) {
        let prev = self.0.fetch_sub(1, Ordering::AcqRel);
        debug_assert!(prev > 0, "BusyHandle dropped more times than claims were made");
        if prev == 0 {
            // Unreachable by construction (every handle is born from a +1). In release
            // saturate back to 0 instead of wrapping to usize::MAX, which would pin the
            // node as permanently busy.
            self.0.fetch_add(1, Ordering::AcqRel);
        }
    }
}

/// RAII guard that tracks an active request on a backend node.
/// Increments `active_requests` on creation (or wraps a claim the caller made
/// itself); the slot is released when the LAST clone of the guard drops. Cloning
/// shares the one claim, so the streaming path can hand a clone to the lazy
/// response body without double-counting.
#[derive(Debug, Clone)]
pub struct BackendGuard {
    _busy: Arc<BusyHandle>,
    pub node: Arc<BackendNode>,
    /// Backend group name (only set in multi-backend mode)
    pub group_name: Option<String>,
}

impl BackendGuard {
    pub fn new(node: Arc<BackendNode>) -> Self {
        node.active_requests.fetch_add(1, Ordering::Release);
        Self::from_claimed(node)
    }

    /// Create a guard with an associated group name
    pub fn with_group(node: Arc<BackendNode>, group_name: String) -> Self {
        let mut guard = Self::new(node);
        guard.group_name = Some(group_name);
        guard
    }

    /// Wrap a node whose counter the caller already incremented by exactly one.
    /// The priority_free free-claim CAS and all-busy reservation record the +1
    /// themselves; going through `new` there would double-count.
    pub(crate) fn from_claimed(node: Arc<BackendNode>) -> Self {
        Self {
            _busy: Arc::new(BusyHandle(node.active_requests.clone())),
            node,
            group_name: None,
        }
    }
}

/// Trait for load balancing strategies across multiple backend nodes
pub trait LoadBalancer: Send + Sync {
    /// Select the next backend node according to the strategy.
    /// Returns Err(NoMatchingBackend) if no backend is configured for the model.
    /// Returns a BackendGuard that releases the node on drop.
    fn select(&self, model: Option<&str>) -> Result<BackendGuard, NoMatchingBackend>;

    /// Return the strategy name (for logging)
    fn strategy_name(&self) -> &'static str;

    /// Return all nodes (for logging/CLI display)
    fn all_nodes(&self) -> Vec<Arc<BackendNode>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_node(url: &str) -> Arc<BackendNode> {
        Arc::new(BackendNode {
            url: url.to_string(),
            model: None,
            api_key: None,
            timeout_seconds: 300,
            http_client: reqwest::Client::new(),
            active_requests: Arc::new(AtomicUsize::new(0)),
            strip_path_prefix: None,
            temperature: None,
            healthy: std::sync::atomic::AtomicBool::new(true),
            cooldown_until: std::sync::Mutex::new(std::time::Instant::now()),
        })
    }

    #[test]
    fn clones_share_one_claim_released_by_last_drop() {
        let node = make_test_node("http://localhost:8080");
        let guard = BackendGuard::new(node.clone());
        assert_eq!(node.active_requests.load(Ordering::Relaxed), 1);

        let clone = guard.clone();
        assert_eq!(
            node.active_requests.load(Ordering::Relaxed),
            1,
            "cloning a guard must not add a second claim"
        );

        drop(guard);
        assert_eq!(
            node.active_requests.load(Ordering::Relaxed),
            1,
            "the claim is held while any clone is alive (the body clone)"
        );

        drop(clone);
        assert_eq!(
            node.active_requests.load(Ordering::Relaxed),
            0,
            "the last drop must release exactly once"
        );
    }

    #[test]
    fn from_claimed_wraps_an_existing_increment_without_adding_one() {
        let node = make_test_node("http://localhost:8080");
        // Simulate the priority_free claim: the caller records the +1 itself.
        node.active_requests.fetch_add(1, Ordering::AcqRel);

        let guard = BackendGuard::from_claimed(node.clone());
        assert_eq!(
            node.active_requests.load(Ordering::Relaxed),
            1,
            "from_claimed must not double-count a caller-made claim"
        );

        let body_clone = guard.clone();
        drop(guard);
        assert_eq!(node.active_requests.load(Ordering::Relaxed), 1);
        drop(body_clone);
        assert_eq!(node.active_requests.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn with_group_keeps_single_claim_and_group_name() {
        let node = make_test_node("http://localhost:8080");
        let guard = BackendGuard::with_group(node.clone(), "local".to_string());
        assert_eq!(node.active_requests.load(Ordering::Relaxed), 1);
        assert_eq!(guard.group_name.as_deref(), Some("local"));
        drop(guard);
        assert_eq!(node.active_requests.load(Ordering::Relaxed), 0);
    }
}
