//! Round-robin load balancing strategy
//!
//! Cycles through the nodes in order, skipping nodes in failure cooldown (big-fix E-M1);
//! when every node is cooled, the soonest-to-recover one serves with a warning.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

use super::balancer::{BackendGuard, LoadBalancer};
use super::node::{soonest_recovering_node, BackendNode};
use crate::config::NoMatchingBackend;

/// Round-robin load balancer — cycles through nodes in order
pub struct RoundRobinBalancer {
    nodes: Vec<Arc<BackendNode>>,
    counter: AtomicUsize,
    all_cooled_fallbacks: AtomicU64,
}

impl RoundRobinBalancer {
    pub fn new(nodes: Vec<Arc<BackendNode>>) -> Result<Self, Box<dyn std::error::Error>> {
        if nodes.is_empty() {
            return Err("RoundRobinBalancer requires at least one node".into());
        }
        Ok(Self {
            nodes,
            counter: AtomicUsize::new(0),
            all_cooled_fallbacks: AtomicU64::new(0),
        })
    }
}

impl LoadBalancer for RoundRobinBalancer {
    fn select(&self, _model: Option<&str>) -> Result<BackendGuard, NoMatchingBackend> {
        // Model routing is handled by GroupedLoadBalancer; this balancer just cycles through nodes
        // (modulo first: both summands stay < len, so the walk over cooled nodes cannot overflow).
        let now = Instant::now();
        let len = self.nodes.len();
        let start = self.counter.fetch_add(1, Ordering::Relaxed) % len;
        for step in 0..len {
            let node = &self.nodes[(start + step) % len];
            if !node.in_cooldown(now) {
                return Ok(BackendGuard::new(node.clone()));
            }
        }

        // Every node is cooled: serve from the soonest to recover rather than refuse.
        let hits = self.all_cooled_fallbacks.fetch_add(1, Ordering::Relaxed) + 1;
        if hits == 1 || hits.is_multiple_of(100) {
            tracing::warn!(
                fallbacks = hits,
                nodes = len,
                "Every round_robin node is in failure cooldown - serving from the soonest to recover"
            );
        }
        Ok(BackendGuard::new(soonest_recovering_node(&self.nodes)))
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
    use std::time::{Duration, Instant};

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
    fn test_round_robin_cycling() {
        let nodes = vec![
            make_test_node("http://localhost:8080"),
            make_test_node("http://localhost:8081"),
            make_test_node("http://localhost:8082"),
        ];
        let balancer = RoundRobinBalancer::new(nodes).unwrap();

        assert_eq!(balancer.select(None).unwrap().node.base_url(), "http://localhost:8080");
        assert_eq!(balancer.select(None).unwrap().node.base_url(), "http://localhost:8081");
        assert_eq!(balancer.select(None).unwrap().node.base_url(), "http://localhost:8082");
        assert_eq!(balancer.select(None).unwrap().node.base_url(), "http://localhost:8080");
    }

    #[test]
    fn test_round_robin_single_node() {
        let nodes = vec![make_test_node("http://localhost:8080")];
        let balancer = RoundRobinBalancer::new(nodes).unwrap();

        assert_eq!(balancer.select(None).unwrap().node.base_url(), "http://localhost:8080");
        assert_eq!(balancer.select(None).unwrap().node.base_url(), "http://localhost:8080");
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
            let _guard = balancer.select(None).unwrap();
            assert_eq!(node.active_requests.load(Ordering::Relaxed), 1);
        }
        assert_eq!(node.active_requests.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_guard_multiple_concurrent() {
        let node = make_test_node("http://localhost:8080");
        let nodes = vec![node.clone()];
        let balancer = RoundRobinBalancer::new(nodes).unwrap();

        let g1 = balancer.select(None).unwrap();
        let g2 = balancer.select(None).unwrap();
        assert_eq!(node.active_requests.load(Ordering::Relaxed), 2);
        drop(g1);
        assert_eq!(node.active_requests.load(Ordering::Relaxed), 1);
        drop(g2);
        assert_eq!(node.active_requests.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_model_parameter_ignored() {
        // Model parameter is now ignored - routing is handled by GroupedLoadBalancer
        let nodes = vec![
            make_test_node("http://localhost:8080"),
            make_test_node("http://localhost:8081"),
        ];
        let balancer = RoundRobinBalancer::new(nodes).unwrap();

        // All selects should cycle through nodes regardless of model
        assert_eq!(
            balancer.select(Some("haiku")).unwrap().node.base_url(),
            "http://localhost:8080"
        );
        assert_eq!(
            balancer.select(Some("sonnet")).unwrap().node.base_url(),
            "http://localhost:8081"
        );
        assert_eq!(balancer.select(None).unwrap().node.base_url(), "http://localhost:8080");
    }

    #[test]
    fn cooled_nodes_are_skipped_while_a_healthy_one_remains() {
        let a = make_test_node("http://localhost:8080");
        let b = make_test_node("http://localhost:8081");
        let t0 = Instant::now();
        b.mark_failed(Duration::from_secs(30));

        let balancer = RoundRobinBalancer::new(vec![a.clone(), b.clone()]).unwrap();
        for round in 0..4 {
            assert_eq!(
                balancer.select(None).unwrap().node.base_url(),
                "http://localhost:8080",
                "round {round}: the cycle must step over the cooled node"
            );
        }
        assert!(
            b.in_cooldown(t0 + Duration::from_secs(1)) && !b.in_cooldown(t0 + Duration::from_secs(31)),
            "the cooled node's window must exclude it now and release it after 30s"
        );
    }

    #[test]
    fn all_cooled_fallback_returns_the_soonest_expiry_node() {
        let a = make_test_node("http://localhost:8080");
        let b = make_test_node("http://localhost:8081");
        b.mark_failed(Duration::from_secs(30));
        a.mark_failed(Duration::from_secs(30));

        let balancer = RoundRobinBalancer::new(vec![a.clone(), b.clone()]).unwrap();
        let guard = balancer.select(None).unwrap();
        assert_eq!(
            guard.node.base_url(),
            "http://localhost:8081",
            "with every node cooled, the soonest-expiring one (b was marked first) must serve"
        );
        assert_eq!(b.active_requests.load(Ordering::Acquire), 1);
        drop(guard);

        b.mark_healthy();
        assert_eq!(
            balancer.select(None).unwrap().node.base_url(),
            "http://localhost:8081",
            "the recovered node must re-enter rotation immediately"
        );
    }
}
