//! Load balancer trait and BackendGuard

use std::sync::atomic::Ordering;
use std::sync::Arc;

use super::node::BackendNode;

/// RAII guard that tracks an active request on a backend node.
/// Increments `active_requests` on creation, decrements on drop.
pub struct BackendGuard {
    pub node: Arc<BackendNode>,
}

impl BackendGuard {
    pub fn new(node: Arc<BackendNode>) -> Self {
        node.active_requests.fetch_add(1, Ordering::Relaxed);
        Self { node }
    }
}

impl Drop for BackendGuard {
    fn drop(&mut self) {
        self.node.active_requests.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Trait for load balancing strategies across multiple backend nodes
pub trait LoadBalancer: Send + Sync {
    /// Select the next backend node according to the strategy.
    /// If model is provided, only consider backends with matching mapping.
    /// If model is None or no mapping matches, consider all backends.
    /// Returns a BackendGuard that releases the node on drop.
    fn select(&self, model: Option<&str>) -> BackendGuard;

    /// Return the strategy name (for logging)
    fn strategy_name(&self) -> &'static str;

    /// Return all nodes (for logging/CLI display)
    fn all_nodes(&self) -> Vec<Arc<BackendNode>>;
}
