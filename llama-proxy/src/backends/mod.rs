//! Multi-backend load balancing

mod balancer;
mod grouped;
mod node;
pub mod preflight;
mod priority_free;
mod round_robin;

pub use balancer::{BackendGuard, LoadBalancer};
pub use grouped::GroupedLoadBalancer;
pub use node::BackendNode;
pub(crate) use node::{node_url, with_auth};
pub use priority_free::PriorityFreeBalancer;
pub use round_robin::RoundRobinBalancer;

use std::sync::Arc;

use serde::Deserialize;

use crate::config::BackendsConfig;

/// Typed load-balancing strategy (E-L9). Deserialize takes the canonical
/// kebab-case spellings plus the legacy snake_case config spellings.
/// NOTE the deliberate mismatch: this enum's Default is PriorityFree per the
/// plan, while the still-stringly config default (config/mod.rs, W9-owned)
/// remains "round_robin". The reconciliation is W9's carry with task 71's
/// typed field — until then config values flow through FromStr unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BalancingStrategy {
    #[serde(alias = "priority_free")]
    #[default]
    PriorityFree,
    #[serde(alias = "round_robin")]
    RoundRobin,
}

impl std::str::FromStr for BalancingStrategy {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "priority-free" | "priority_free" => Ok(Self::PriorityFree),
            "round-robin" | "round_robin" => Ok(Self::RoundRobin),
            other => Err(format!(
                "Unknown load balancer strategy: '{other}'. Supported: priority-free, round-robin"
            )),
        }
    }
}

/// Build a load balancer from backend group configurations
pub fn build_balancer_from_groups(groups: BackendsConfig) -> Result<Arc<dyn LoadBalancer>, Box<dyn std::error::Error>> {
    Ok(Arc::new(GroupedLoadBalancer::new(groups)?))
}

/// Build a load balancer for a single group (used internally by GroupedLoadBalancer)
pub fn build_balancer_for_group(
    nodes: Vec<Arc<BackendNode>>,
    strategy: &str,
) -> Result<Arc<dyn LoadBalancer>, Box<dyn std::error::Error>> {
    match strategy.parse::<BalancingStrategy>()? {
        BalancingStrategy::PriorityFree => Ok(Arc::new(PriorityFreeBalancer::new(nodes)?)),
        BalancingStrategy::RoundRobin => Ok(Arc::new(RoundRobinBalancer::new(nodes)?)),
    }
}

/// Build a load balancer from a single backend configuration (backward compatibility)
pub fn build_balancer_from_single(
    url: String,
    timeout_seconds: u64,
    tls: Option<&crate::config::TlsConfig>,
    model: Option<String>,
    api_key: Option<String>,
    strip_path_prefix: Option<String>,
) -> Result<Arc<dyn LoadBalancer>, Box<dyn std::error::Error>> {
    let node = BackendNode::from_config(url, timeout_seconds, tls, model, api_key, strip_path_prefix, None)?;
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
            active_requests: Arc::new(AtomicUsize::new(0)),
            strip_path_prefix: None,
            temperature: None,
            healthy: std::sync::atomic::AtomicBool::new(true),
            cooldown_until: std::sync::Mutex::new(std::time::Instant::now()),
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
    fn strategy_serde_takes_kebab_plus_snake_aliases() {
        let parse = |s: &str| serde_json::from_str::<BalancingStrategy>(s).ok();
        assert_eq!(parse("\"priority-free\""), Some(BalancingStrategy::PriorityFree));
        assert_eq!(
            parse("\"priority_free\""),
            Some(BalancingStrategy::PriorityFree),
            "legacy config spelling keeps parsing"
        );
        assert_eq!(parse("\"round-robin\""), Some(BalancingStrategy::RoundRobin));
        assert_eq!(parse("\"round_robin\""), Some(BalancingStrategy::RoundRobin));
        assert_eq!(parse("\"least_busy\""), None, "unknown spellings stay rejected");
    }

    #[test]
    fn strategy_default_is_priority_free_per_plan() {
        assert_eq!(BalancingStrategy::default(), BalancingStrategy::PriorityFree);
        assert_eq!(
            "round_robin".parse::<BalancingStrategy>().unwrap(),
            BalancingStrategy::RoundRobin,
            "the stringly config default still routes to RoundRobin"
        );
    }

    #[test]
    fn build_balancer_for_group_takes_both_spellings() {
        let nodes = vec![make_test_node("http://localhost:8080")];
        assert_eq!(
            build_balancer_for_group(nodes, "priority-free").unwrap().strategy_name(),
            "priority_free"
        );
        let nodes = vec![make_test_node("http://localhost:8080")];
        assert_eq!(
            build_balancer_for_group(nodes, "round-robin").unwrap().strategy_name(),
            "round_robin"
        );
    }

    #[test]
    fn test_build_balancer_from_groups() {
        let mut groups = HashMap::new();
        groups.insert(
            "test".to_string(),
            BackendGroupConfig {
                mappings: vec![],
                strategy: "round_robin".to_string(),
                failure_cooldown_secs: 30,
                exclusive: false,
                nodes: vec![BackendNodeConfig {
                    url: "http://localhost:8080".to_string(),
                    timeout_seconds: 300,
                    tls: None,
                    model: None,
                    api_key: None,
                    strip_path_prefix: None,
                    temperature: None,
                }],
            },
        );

        let balancer = build_balancer_from_groups(groups).unwrap();
        assert_eq!(balancer.strategy_name(), "grouped");
    }
}
