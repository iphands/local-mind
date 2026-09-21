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

/// One node-visit order for every strategy: walk the node list starting at
/// `start`, wrapping at the end — `start % len`, then `(start + step) % len`.
///
/// priority_free walks with `start = 0` (lowest-index priority); round_robin
/// walks with its rotating counter. Sharing the walk is what makes the
/// strategies iterate IDENTICALLY: cooldown filtering and claiming stay
/// per-strategy, the traversal cannot drift between them.
///
/// An empty slice yields an empty iterator (the closure never runs, so the
/// modulo never divides by zero). Both summands are `< len`, so the sum
/// cannot overflow.
pub(crate) fn candidate_order<'a>(
    nodes: &'a [Arc<BackendNode>],
    start: usize,
) -> impl Iterator<Item = (usize, &'a Arc<BackendNode>)> + 'a {
    let len = nodes.len();
    (0..len).map(move |step| {
        let idx = (start % len + step) % len;
        (idx, &nodes[idx])
    })
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
    fn candidate_order_walks_one_rotation_for_every_strategy() {
        let nodes: Vec<Arc<BackendNode>> = vec![
            make_test_node("http://a"),
            make_test_node("http://b"),
            make_test_node("http://c"),
        ];
        // start = 0 (priority_free's entry order): identity order.
        let walked: Vec<usize> = candidate_order(&nodes, 0).map(|(idx, _)| idx).collect();
        assert_eq!(walked, vec![0, 1, 2], "start=0 must visit lowest index first");
        // start = 2 (round_robin mid-rotation): wraps at the end.
        let walked: Vec<usize> = candidate_order(&nodes, 2).map(|(idx, _)| idx).collect();
        assert_eq!(walked, vec![2, 0, 1], "the walk must wrap around the end");
        // start beyond len is the same walk as start % len (the counter is unbounded).
        let walked: Vec<usize> = candidate_order(&nodes, 5).map(|(idx, _)| idx).collect();
        assert_eq!(walked, vec![2, 0, 1], "start=5 must reduce to start=2");
    }

    #[test]
    fn candidate_order_yields_nodes_matching_their_indices() {
        let nodes: Vec<Arc<BackendNode>> = vec![make_test_node("http://a"), make_test_node("http://b")];
        let seen: Vec<(usize, String)> = candidate_order(&nodes, 1)
            .map(|(idx, node)| (idx, node.base_url().to_string()))
            .collect();
        assert_eq!(
            seen,
            vec![(1, "http://b".to_string()), (0, "http://a".to_string())],
            "the yielded node must be the node at the yielded index"
        );
    }

    #[test]
    fn candidate_order_on_empty_nodes_is_empty_not_a_panic() {
        let nodes: Vec<Arc<BackendNode>> = vec![];
        assert_eq!(
            candidate_order(&nodes, 7).count(),
            0,
            "empty list must yield no candidates, not a modulo panic"
        );
    }
}
