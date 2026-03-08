//! Multi-backend load balancing

mod balancer;
mod node;
mod priority_free;
mod round_robin;

pub use balancer::{BackendGuard, LoadBalancer};
pub use node::BackendNode;
pub use priority_free::PriorityFreeBalancer;
pub use round_robin::RoundRobinBalancer;

use std::sync::atomic::AtomicUsize;
use std::sync::Arc;

/// Build a load balancer from a list of nodes and a strategy name
pub fn build_balancer(
    nodes: Vec<BackendNode>,
    strategy: &str,
) -> Result<Arc<dyn LoadBalancer>, Box<dyn std::error::Error>> {
    let arc_nodes: Vec<Arc<BackendNode>> = nodes.into_iter().map(Arc::new).collect();
    match strategy {
        "round_robin" => Ok(Arc::new(RoundRobinBalancer::new(arc_nodes)?)),
        "priority_free" => Ok(Arc::new(PriorityFreeBalancer::new(arc_nodes)?)),
        other => Err(format!(
            "Unknown load balancer strategy: '{}'. Supported: round_robin, priority_free",
            other
        )
        .into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_node(url: &str) -> BackendNode {
        BackendNode {
            url: url.to_string(),
            model: None,
            api_key: None,
            timeout_seconds: 300,
            http_client: reqwest::Client::new(),
            active_requests: AtomicUsize::new(0),
        }
    }

    #[test]
    fn test_build_balancer_round_robin() {
        let nodes = vec![make_test_node("http://localhost:8080")];
        let balancer = build_balancer(nodes, "round_robin").unwrap();
        assert_eq!(balancer.strategy_name(), "round_robin");
    }

    #[test]
    fn test_build_balancer_priority_free() {
        let nodes = vec![make_test_node("http://localhost:8080")];
        let balancer = build_balancer(nodes, "priority_free").unwrap();
        assert_eq!(balancer.strategy_name(), "priority_free");
    }

    #[test]
    fn test_build_balancer_unknown_strategy() {
        let nodes = vec![make_test_node("http://localhost:8080")];
        let result = build_balancer(nodes, "bogus");
        assert!(result.is_err());
        assert!(result.err().unwrap().to_string().contains("bogus"));
    }

    #[test]
    fn test_build_balancer_selects_node() {
        let nodes = vec![make_test_node("http://localhost:8080")];
        let balancer = build_balancer(nodes, "priority_free").unwrap();
        assert_eq!(balancer.select().node.base_url(), "http://localhost:8080");
    }
}
