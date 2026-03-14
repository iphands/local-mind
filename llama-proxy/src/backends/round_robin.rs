//! Round-robin load balancing strategy

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use super::balancer::{BackendGuard, LoadBalancer};
use super::node::BackendNode;

/// Round-robin load balancer — cycles through nodes in order
pub struct RoundRobinBalancer {
    nodes: Vec<Arc<BackendNode>>,
    counter: AtomicUsize,
}

impl RoundRobinBalancer {
    pub fn new(nodes: Vec<Arc<BackendNode>>) -> Result<Self, Box<dyn std::error::Error>> {
        if nodes.is_empty() {
            return Err("RoundRobinBalancer requires at least one node".into());
        }
        Ok(Self {
            nodes,
            counter: AtomicUsize::new(0),
        })
    }
}

impl LoadBalancer for RoundRobinBalancer {
    fn select(&self, model: Option<&str>) -> BackendGuard {
        match model {
            None => {
                // No model specified, use all nodes
                let idx = self.counter.fetch_add(1, Ordering::Relaxed) % self.nodes.len();
                BackendGuard::new(self.nodes[idx].clone())
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
                    let idx = self.counter.fetch_add(1, Ordering::Relaxed) % self.nodes.len();
                    BackendGuard::new(self.nodes[idx].clone())
                } else {
                    // Use only specific matches
                    let idx = self.counter.fetch_add(1, Ordering::Relaxed) % specific_nodes.len();
                    BackendGuard::new(specific_nodes[idx].clone())
                }
            }
        }
    }

    fn strategy_name(&self) -> &'static str {
        "round_robin"
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
    fn test_round_robin_cycling() {
        let nodes = vec![
            make_test_node("http://localhost:8080"),
            make_test_node("http://localhost:8081"),
            make_test_node("http://localhost:8082"),
        ];
        let balancer = RoundRobinBalancer::new(nodes).unwrap();

        assert_eq!(balancer.select(None).node.base_url(), "http://localhost:8080");
        assert_eq!(balancer.select(None).node.base_url(), "http://localhost:8081");
        assert_eq!(balancer.select(None).node.base_url(), "http://localhost:8082");
        assert_eq!(balancer.select(None).node.base_url(), "http://localhost:8080");
    }

    #[test]
    fn test_round_robin_single_node() {
        let nodes = vec![make_test_node("http://localhost:8080")];
        let balancer = RoundRobinBalancer::new(nodes).unwrap();

        assert_eq!(balancer.select(None).node.base_url(), "http://localhost:8080");
        assert_eq!(balancer.select(None).node.base_url(), "http://localhost:8080");
    }

    #[test]
    fn test_round_robin_empty_nodes_error() {
        let result = RoundRobinBalancer::new(vec![]);
        assert!(result.is_err());
    }

    #[test]
    fn test_strategy_name() {
        let nodes = vec![make_test_node("http://localhost:8080")];
        let balancer = RoundRobinBalancer::new(nodes).unwrap();
        assert_eq!(balancer.strategy_name(), "round_robin");
    }

    #[test]
    fn test_all_nodes() {
        let nodes = vec![
            make_test_node("http://localhost:8080"),
            make_test_node("http://localhost:8081"),
        ];
        let balancer = RoundRobinBalancer::new(nodes).unwrap();
        assert_eq!(balancer.all_nodes().len(), 2);
    }

    #[test]
    fn test_guard_increments_and_decrements() {
        let node = make_test_node("http://localhost:8080");
        let nodes = vec![node.clone()];
        let balancer = RoundRobinBalancer::new(nodes).unwrap();

        assert_eq!(node.active_requests.load(Ordering::Relaxed), 0);
        {
            let _guard = balancer.select(None);
            assert_eq!(node.active_requests.load(Ordering::Relaxed), 1);
        }
        assert_eq!(node.active_requests.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_guard_multiple_concurrent() {
        let node = make_test_node("http://localhost:8080");
        let nodes = vec![node.clone()];
        let balancer = RoundRobinBalancer::new(nodes).unwrap();

        let g1 = balancer.select(None);
        let g2 = balancer.select(None);
        assert_eq!(node.active_requests.load(Ordering::Relaxed), 2);
        drop(g1);
        assert_eq!(node.active_requests.load(Ordering::Relaxed), 1);
        drop(g2);
        assert_eq!(node.active_requests.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_model_routing_filter() {
        // Node 0: catch-all (no mapping)
        // Node 1: haiku only
        // Node 2: sonnet only
        let nodes = vec![
            make_test_node("http://localhost:8080"),
            make_test_node_with_mapping("http://localhost:8081", vec!["haiku"]),
            make_test_node_with_mapping("http://localhost:8082", vec!["sonnet"]),
        ];
        let balancer = RoundRobinBalancer::new(nodes).unwrap();

        // Request for haiku should only use node 1
        assert_eq!(balancer.select(Some("haiku")).node.base_url(), "http://localhost:8081");
        assert_eq!(balancer.select(Some("haiku")).node.base_url(), "http://localhost:8081");

        // Request for sonnet should only use node 2
        assert_eq!(balancer.select(Some("sonnet")).node.base_url(), "http://localhost:8082");

        // Request for unknown model should use all nodes (fallback)
        // The counter keeps incrementing, so next is node 0
        assert_eq!(balancer.select(Some("unknown")).node.base_url(), "http://localhost:8080");
    }

    #[test]
    fn test_model_routing_catch_all() {
        // Node 0: catch-all
        // Node 1: haiku only
        let nodes = vec![
            make_test_node("http://localhost:8080"),
            make_test_node_with_mapping("http://localhost:8081", vec!["haiku"]),
        ];
        let balancer = RoundRobinBalancer::new(nodes).unwrap();

        // No model specified should use all nodes
        assert_eq!(balancer.select(None).node.base_url(), "http://localhost:8080");
        assert_eq!(balancer.select(None).node.base_url(), "http://localhost:8081");
    }

    #[test]
    fn test_model_routing_multiple_mappings() {
        // Node handles multiple models
        let nodes = vec![
            make_test_node_with_mapping("http://localhost:8080", vec!["haiku", "claude-haiku-4-5-20251001"]),
            make_test_node("http://localhost:8081"),
        ];
        let balancer = RoundRobinBalancer::new(nodes).unwrap();

        // Both haiku variants should route to node 0
        assert_eq!(balancer.select(Some("haiku")).node.base_url(), "http://localhost:8080");
        assert_eq!(balancer.select(Some("claude-haiku-4-5-20251001")).node.base_url(), "http://localhost:8080");
    }
}
