//! Priority-free load balancing strategy
//!
//! Always dispatches to the lowest-index backend that is not currently handling a request.
//! If all backends are busy, picks the one with the fewest active requests (lowest index wins ties).

use std::sync::atomic::Ordering;
use std::sync::Arc;

use super::balancer::{BackendGuard, LoadBalancer};
use super::node::BackendNode;

/// Priority-free load balancer — always uses the lowest-index free node.
pub struct PriorityFreeBalancer {
    nodes: Vec<Arc<BackendNode>>,
}

impl PriorityFreeBalancer {
    pub fn new(nodes: Vec<Arc<BackendNode>>) -> Result<Self, Box<dyn std::error::Error>> {
        if nodes.is_empty() {
            return Err("PriorityFreeBalancer requires at least one node".into());
        }
        Ok(Self { nodes })
    }
}

impl LoadBalancer for PriorityFreeBalancer {
    fn select(&self) -> BackendGuard {
        // Pick the first node with zero active requests
        for node in &self.nodes {
            if node.active_requests.load(Ordering::Relaxed) == 0 {
                return BackendGuard::new(node.clone());
            }
        }

        // All busy: pick the node with the fewest active requests (lowest index wins ties)
        let node = self
            .nodes
            .iter()
            .min_by_key(|n| n.active_requests.load(Ordering::Relaxed))
            .unwrap(); // safe: nodes is non-empty
        BackendGuard::new(node.clone())
    }

    fn strategy_name(&self) -> &'static str {
        "priority_free"
    }

    fn all_nodes(&self) -> Vec<Arc<BackendNode>> {
        self.nodes.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    fn make_test_node(url: &str) -> Arc<BackendNode> {
        Arc::new(BackendNode {
            url: url.to_string(),
            model: None,
            api_key: None,
            timeout_seconds: 300,
            http_client: reqwest::Client::new(),
            active_requests: AtomicUsize::new(0),
        })
    }

    #[test]
    fn test_single_node_always_selected() {
        let nodes = vec![make_test_node("http://localhost:8080")];
        let balancer = PriorityFreeBalancer::new(nodes).unwrap();
        assert_eq!(balancer.select().node.base_url(), "http://localhost:8080");
        assert_eq!(balancer.select().node.base_url(), "http://localhost:8080");
    }

    #[test]
    fn test_first_free_wins() {
        let nodes = vec![
            make_test_node("http://localhost:8080"),
            make_test_node("http://localhost:8081"),
            make_test_node("http://localhost:8082"),
        ];
        let balancer = PriorityFreeBalancer::new(nodes).unwrap();

        // Node 0 is free — always gets selected
        assert_eq!(balancer.select().node.base_url(), "http://localhost:8080");
        assert_eq!(balancer.select().node.base_url(), "http://localhost:8080");
    }

    #[test]
    fn test_skips_busy_nodes() {
        let nodes = vec![
            make_test_node("http://localhost:8080"),
            make_test_node("http://localhost:8081"),
            make_test_node("http://localhost:8082"),
        ];

        // Mark node 0 and 1 as busy
        nodes[0].active_requests.store(1, Ordering::Relaxed);
        nodes[1].active_requests.store(1, Ordering::Relaxed);

        let balancer = PriorityFreeBalancer::new(nodes).unwrap();
        assert_eq!(balancer.select().node.base_url(), "http://localhost:8082");
    }

    #[test]
    fn test_all_busy_picks_least_loaded() {
        let nodes = vec![
            make_test_node("http://localhost:8080"),
            make_test_node("http://localhost:8081"),
            make_test_node("http://localhost:8082"),
        ];

        nodes[0].active_requests.store(3, Ordering::Relaxed);
        nodes[1].active_requests.store(1, Ordering::Relaxed); // least loaded
        nodes[2].active_requests.store(2, Ordering::Relaxed);

        let balancer = PriorityFreeBalancer::new(nodes).unwrap();
        assert_eq!(balancer.select().node.base_url(), "http://localhost:8081");
    }

    #[test]
    fn test_all_busy_tie_picks_lowest_index() {
        let nodes = vec![
            make_test_node("http://localhost:8080"),
            make_test_node("http://localhost:8081"),
            make_test_node("http://localhost:8082"),
        ];

        // All have same load — lowest index (node 0) wins
        nodes[0].active_requests.store(2, Ordering::Relaxed);
        nodes[1].active_requests.store(2, Ordering::Relaxed);
        nodes[2].active_requests.store(2, Ordering::Relaxed);

        let balancer = PriorityFreeBalancer::new(nodes).unwrap();
        assert_eq!(balancer.select().node.base_url(), "http://localhost:8080");
    }

    #[test]
    fn test_guard_decrements_on_drop() {
        let node = make_test_node("http://localhost:8080");
        let nodes = vec![node.clone()];
        let balancer = PriorityFreeBalancer::new(nodes).unwrap();

        assert_eq!(node.active_requests.load(Ordering::Relaxed), 0);
        {
            let _guard = balancer.select();
            assert_eq!(node.active_requests.load(Ordering::Relaxed), 1);
        }
        // Guard dropped — counter should be back to 0
        assert_eq!(node.active_requests.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_empty_nodes_error() {
        let result = PriorityFreeBalancer::new(vec![]);
        assert!(result.is_err());
    }

    #[test]
    fn test_strategy_name() {
        let nodes = vec![make_test_node("http://localhost:8080")];
        let balancer = PriorityFreeBalancer::new(nodes).unwrap();
        assert_eq!(balancer.strategy_name(), "priority_free");
    }

    #[test]
    fn test_five_nodes_example_from_spec() {
        // 0: busy, 1: busy, 2: free, 3: busy, 4: free → should pick node 2
        let nodes = vec![
            make_test_node("http://localhost:8080"),
            make_test_node("http://localhost:8081"),
            make_test_node("http://localhost:8082"),
            make_test_node("http://localhost:8083"),
            make_test_node("http://localhost:8084"),
        ];

        nodes[0].active_requests.store(1, Ordering::Relaxed);
        nodes[1].active_requests.store(1, Ordering::Relaxed);
        // nodes[2] stays at 0
        nodes[3].active_requests.store(1, Ordering::Relaxed);
        // nodes[4] stays at 0

        let balancer = PriorityFreeBalancer::new(nodes).unwrap();
        assert_eq!(balancer.select().node.base_url(), "http://localhost:8082");
    }
}
