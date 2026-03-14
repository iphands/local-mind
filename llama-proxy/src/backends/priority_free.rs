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
    fn select(&self, model: Option<&str>) -> BackendGuard {
        match model {
            None => {
                // No model specified, use all nodes
                self.select_from_nodes(&self.nodes)
            }
            Some(model_str) => {
                // Find nodes that specifically handle this model (non-empty mapping containing model)
                let specific_nodes: Vec<Arc<BackendNode>> = self.nodes.iter()
                    .filter(|n| !n.mapping.is_empty() && n.mapping.contains(&model_str.to_string()))
                    .cloned()
                    .collect();

                if specific_nodes.is_empty() {
                    // No specific match - fall back to all nodes
                    tracing::debug!(
                        requested_model = ?model,
                        "No backends specifically handle model, falling back to all nodes"
                    );
                    self.select_from_nodes(&self.nodes)
                } else {
                    // Use only specific matches
                    self.select_from_nodes(&specific_nodes)
                }
            }
        }
    }

    fn strategy_name(&self) -> &'static str {
        "priority_free"
    }

    fn all_nodes(&self) -> Vec<Arc<BackendNode>> {
        self.nodes.clone()
    }
}

impl PriorityFreeBalancer {
    /// Select from a slice of nodes (used internally)
    fn select_from_nodes(&self, nodes: &[Arc<BackendNode>]) -> BackendGuard {
        // Pick the first node with zero active requests
        for node in nodes {
            if node.active_requests.load(Ordering::Relaxed) == 0 {
                return BackendGuard::new(node.clone());
            }
        }

        // All busy: pick the node with the fewest active requests (lowest index wins ties)
        let node = nodes
            .iter()
            .min_by_key(|n| n.active_requests.load(Ordering::Relaxed))
            .unwrap(); // safe: nodes is non-empty
        BackendGuard::new(node.clone())
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
            mapping: vec![],
        })
    }

    fn make_test_node_with_mapping(url: &str, mapping: Vec<&str>) -> Arc<BackendNode> {
        Arc::new(BackendNode {
            url: url.to_string(),
            model: None,
            api_key: None,
            timeout_seconds: 300,
            http_client: reqwest::Client::new(),
            active_requests: AtomicUsize::new(0),
            mapping: mapping.iter().map(|s| s.to_string()).collect(),
        })
    }

    #[test]
    fn test_single_node_always_selected() {
        let nodes = vec![make_test_node("http://localhost:8080")];
        let balancer = PriorityFreeBalancer::new(nodes).unwrap();
        assert_eq!(balancer.select(None).node.base_url(), "http://localhost:8080");
        assert_eq!(balancer.select(None).node.base_url(), "http://localhost:8080");
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
        assert_eq!(balancer.select(None).node.base_url(), "http://localhost:8080");
        assert_eq!(balancer.select(None).node.base_url(), "http://localhost:8080");
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
        assert_eq!(balancer.select(None).node.base_url(), "http://localhost:8082");
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
        assert_eq!(balancer.select(None).node.base_url(), "http://localhost:8081");
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
        assert_eq!(balancer.select(None).node.base_url(), "http://localhost:8080");
    }

    #[test]
    fn test_guard_decrements_on_drop() {
        let node = make_test_node("http://localhost:8080");
        let nodes = vec![node.clone()];
        let balancer = PriorityFreeBalancer::new(nodes).unwrap();

        assert_eq!(node.active_requests.load(Ordering::Relaxed), 0);
        {
            let _guard = balancer.select(None);
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
        // 0: busy, 1: busy, 2: free, 3: busy, 4: free -> should pick node 2
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
        assert_eq!(balancer.select(None).node.base_url(), "http://localhost:8082");
    }

    #[test]
    fn test_model_routing_filter() {
        // Node 0: catch-all (no mapping)
        // Node 1: haiku only (busy)
        // Node 2: sonnet only (free)
        let nodes = vec![
            make_test_node("http://localhost:8080"),
            make_test_node_with_mapping("http://localhost:8081", vec!["haiku"]),
            make_test_node_with_mapping("http://localhost:8082", vec!["sonnet"]),
        ];

        // Mark haiku node as busy
        nodes[1].active_requests.store(1, Ordering::Relaxed);

        let balancer = PriorityFreeBalancer::new(nodes).unwrap();

        // Request for haiku should only consider node 1 (even though busy)
        assert_eq!(balancer.select(Some("haiku")).node.base_url(), "http://localhost:8081");

        // Request for sonnet should only consider node 2
        assert_eq!(balancer.select(Some("sonnet")).node.base_url(), "http://localhost:8082");

        // Request for unknown model should use all nodes (fallback to node 0 - first free)
        assert_eq!(balancer.select(Some("unknown")).node.base_url(), "http://localhost:8080");
    }

    #[test]
    fn test_model_routing_priority_free() {
        // Node 0: haiku only (busy)
        // Node 1: haiku only (free)
        let nodes = vec![
            make_test_node_with_mapping("http://localhost:8080", vec!["haiku"]),
            make_test_node_with_mapping("http://localhost:8081", vec!["haiku"]),
        ];

        // Mark node 0 as busy
        nodes[0].active_requests.store(1, Ordering::Relaxed);

        let balancer = PriorityFreeBalancer::new(nodes).unwrap();

        // Should pick the free haiku node
        assert_eq!(balancer.select(Some("haiku")).node.base_url(), "http://localhost:8081");
    }

    #[test]
    fn test_model_routing_no_match_fallback() {
        // Only one node with haiku mapping
        let nodes = vec![
            make_test_node_with_mapping("http://localhost:8080", vec!["haiku"]),
        ];
        let balancer = PriorityFreeBalancer::new(nodes).unwrap();

        // Request for unknown model should fall back to all nodes
        assert_eq!(balancer.select(Some("sonnet")).node.base_url(), "http://localhost:8080");
    }
}
