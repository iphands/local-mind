//! Load balancer trait

use std::sync::Arc;

use super::node::BackendNode;

/// Trait for load balancing strategies across multiple backend nodes
pub trait LoadBalancer: Send + Sync {
    /// Select the next backend node according to the strategy
    fn select(&self) -> Arc<BackendNode>;

    /// Return the strategy name (for logging)
    fn strategy_name(&self) -> &'static str;

    /// Return all nodes (for logging/CLI display)
    fn all_nodes(&self) -> Vec<Arc<BackendNode>>;
}
