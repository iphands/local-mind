//! Round-robin load balancing strategy

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::balancer::LoadBalancer;
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
    fn select(&self) -> Arc<BackendNode> {
        let idx = self.counter.fetch_add(1, Ordering::Relaxed) % self.nodes.len();
        self.nodes[idx].clone()
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

    fn make_test_node(url: &str) -> Arc<BackendNode> {
        Arc::new(BackendNode {
            url: url.to_string(),
            model: None,
            api_key: None,
            timeout_seconds: 300,
            http_client: reqwest::Client::new(),
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

        assert_eq!(balancer.select().base_url(), "http://localhost:8080");
        assert_eq!(balancer.select().base_url(), "http://localhost:8081");
        assert_eq!(balancer.select().base_url(), "http://localhost:8082");
        assert_eq!(balancer.select().base_url(), "http://localhost:8080");
    }

    #[test]
    fn test_round_robin_single_node() {
        let nodes = vec![make_test_node("http://localhost:8080")];
        let balancer = RoundRobinBalancer::new(nodes).unwrap();

        assert_eq!(balancer.select().base_url(), "http://localhost:8080");
        assert_eq!(balancer.select().base_url(), "http://localhost:8080");
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
}
