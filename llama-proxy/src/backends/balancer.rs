//! Load balancer trait and BackendGuard

use std::sync::atomic::Ordering;
use std::sync::Arc;

use super::node::BackendNode;
use crate::config::NoMatchingBackend;

/// RAII guard that tracks an active request on a backend node.
/// Increments `active_requests` on creation, decrements on drop.
#[derive(Debug)]
pub struct BackendGuard {
    pub node: Arc<BackendNode>,
    /// Backend group name (only set in multi-backend mode)
    pub group_name: Option<String>,
}

impl BackendGuard {
    pub fn new(node: Arc<BackendNode>) -> Self {
        node.active_requests.fetch_add(1, Ordering::Relaxed);
        Self { node, group_name: None }
    }

    /// Create a guard with an associated group name
    pub fn with_group(node: Arc<BackendNode>, group_name: String) -> Self {
        node.active_requests.fetch_add(1, Ordering::Relaxed);
        Self { node, group_name: Some(group_name) }
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
    /// Returns Err(NoMatchingBackend) if no backend is configured for the model.
    /// Returns a BackendGuard that releases the node on drop.
    fn select(&self, model: Option<&str>) -> Result<BackendGuard, NoMatchingBackend>;

    /// Return the strategy name (for logging)
    fn strategy_name(&self) -> &'static str;

    /// Return all nodes (for logging/CLI display)
    fn all_nodes(&self) -> Vec<Arc<BackendNode>>;
}
