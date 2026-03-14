//! Multi-backend load balancing

mod balancer;
mod grouped;
mod node;
mod priority_free;
mod round_robin;

pub use balancer::{BackendGuard, LoadBalancer};
pub use grouped::GroupedLoadBalancer;
pub use node::BackendNode;
pub use priority_free::PriorityFreeBalancer;
pub use round_robin::RoundRobinBalancer;

use std::sync::Arc;

use crate::config::BackendsConfig;

/// Build a load balancer from backend group configurations
pub fn build_balancer_from_groups(
    groups: BackendsConfig,
) -> Result<Arc<dyn LoadBalancer>, Box<dyn std::error::Error>> {
    Ok(Arc::new(GroupedLoadBalancer::new(groups)?))
}

/// Build a load balancer for a single group (used internally by GroupedLoadBalancer)
pub fn build_balancer_for_group(
    nodes: Vec<Arc<BackendNode>>,
    strategy: &str,
) -> Result<Arc<dyn LoadBalancer>, Box<dyn std::error::Error>> {
    match strategy {
        "round_robin" => Ok(Arc::new(RoundRobinBalancer::new(nodes)?)),
        "priority_free" => Ok(Arc::new(PriorityFreeBalancer::new(nodes)?)),
        other => Err(format!(
            "Unknown load balancer strategy: '{}'. Supported: round_robin, priority_free",
            other
        )
        .into()),
    }
}

/// Build a load balancer from a single backend configuration (backward compatibility)
pub fn build_balancer_from_single(
    url: String,
    timeout_seconds: u64,
    tls: Option<&crate::config::TlsConfig>,
    model: Option<String>,
    api_key: Option<String>,
) -> Result<Arc<dyn LoadBalancer>, Box<dyn std::error::Error>> {
    let node = BackendNode::from_config(url, timeout_seconds, tls, model, api_key)?;
    Ok(Arc::new(RoundRobinBalancer::new(vec![Arc::new(node)])?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{BackendGroupConfig, BackendNodeConfig};
    use std::collections::HashMap;
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
    fn test_build_balancer_for_group_round_robin() {
        let nodes = vec![make_test_node("http://localhost:8080")];
        let balancer = build_balancer_for_group(nodes, "round_robin").unwrap();
        assert_eq!(balancer.strategy_name(), "round_robin");
    }

    #[test]
    fn test_build_balancer_for_group_priority_free() {
        let nodes = vec![make_test_node("http://localhost:8080")];
        let balancer = build_balancer_for_group(nodes, "priority_free").unwrap();
        assert_eq!(balancer.strategy_name(), "priority_free");
    }

    #[test]
    fn test_build_balancer_for_group_unknown_strategy() {
        let nodes = vec![make_test_node("http://localhost:8080")];
        let result = build_balancer_for_group(nodes, "bogus");
        assert!(result.is_err());
        assert!(result.err().unwrap().to_string().contains("bogus"));
    }

    #[test]
    fn test_build_balancer_for_group_selects_node() {
        let nodes = vec![make_test_node("http://localhost:8080")];
        let balancer = build_balancer_for_group(nodes, "priority_free").unwrap();
        assert_eq!(balancer.select(None).unwrap().node.base_url(), "http://localhost:8080");
    }

    #[test]
    fn test_build_balancer_from_groups() {
        let mut groups = HashMap::new();
        groups.insert(
            "test".to_string(),
            BackendGroupConfig {
                mappings: vec![],
                strategy: "round_robin".to_string(),
                nodes: vec![BackendNodeConfig {
                    url: "http://localhost:8080".to_string(),
                    timeout_seconds: 300,
                    tls: None,
                    model: None,
                    api_key: None,
                }],
            },
        );

        let balancer = build_balancer_from_groups(groups).unwrap();
        assert_eq!(balancer.strategy_name(), "grouped");
    }
}
