//! Grouped load balancer that routes requests to backend groups based on model matching

use std::sync::Arc;

use super::balancer::{BackendGuard, LoadBalancer};
use super::node::BackendNode;
use crate::config::{BackendGroupConfig, NoMatchingBackend};

/// A single backend group with its own load balancer
struct BackendGroup {
    /// Model names this group handles (empty = catch-all)
    mappings: Vec<String>,
    /// The internal load balancer for this group
    balancer: Arc<dyn LoadBalancer>,
    /// Group name for logging
    name: String,
}

impl BackendGroup {
    /// Check if this group handles the given model
    /// - If mappings is empty: this is a catch-all group (handles all models)
    /// - If mappings contains the model: this group handles it
    fn handles_model(&self, model: &str) -> bool {
        self.mappings.is_empty() || self.mappings.contains(&model.to_string())
    }

    /// Check if this is a catch-all group (handles all models)
    fn is_catch_all(&self) -> bool {
        self.mappings.is_empty()
    }
}

/// Load balancer that manages multiple backend groups with model-based routing
pub struct GroupedLoadBalancer {
    /// All backend groups
    groups: Vec<BackendGroup>,
}

impl GroupedLoadBalancer {
    /// Create a new grouped load balancer from group configurations
    pub fn new(
        group_configs: std::collections::HashMap<String, BackendGroupConfig>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut groups = Vec::new();

        for (name, config) in group_configs {
            // Build BackendNode instances for this group
            let mut nodes = Vec::with_capacity(config.nodes.len());
            for node_cfg in &config.nodes {
                let node = BackendNode::from_config(
                    node_cfg.url.clone(),
                    node_cfg.timeout_seconds,
                    node_cfg.tls.as_ref(),
                    node_cfg.model.clone(),
                    node_cfg.api_key.clone(),
                )?;
                nodes.push(Arc::new(node));
            }

            // Build the internal load balancer for this group
            let balancer = super::build_balancer_for_group(nodes, &config.strategy)?;

            groups.push(BackendGroup {
                mappings: config.mappings,
                balancer,
                name,
            });
        }

        Ok(Self { groups })
    }

    /// Find a group that handles the given model
    /// Returns the first matching group, or the catch-all if no specific match
    fn find_group(&self, model: Option<&str>) -> Option<&BackendGroup> {
        match model {
            Some(model_str) => {
                // First, look for a group with specific mapping for this model
                for group in &self.groups {
                    if !group.is_catch_all() && group.handles_model(model_str) {
                        return Some(group);
                    }
                }
                // Fall back to catch-all group
                for group in &self.groups {
                    if group.is_catch_all() {
                        return Some(group);
                    }
                }
                None
            }
            None => {
                // No model specified - use catch-all group
                for group in &self.groups {
                    if group.is_catch_all() {
                        return Some(group);
                    }
                }
                None
            }
        }
    }
}

impl LoadBalancer for GroupedLoadBalancer {
    fn select(&self, model: Option<&str>) -> Result<BackendGuard, NoMatchingBackend> {
        match self.find_group(model) {
            Some(group) => {
                tracing::debug!(
                    group = %group.name,
                    model = ?model,
                    "Routing request to backend group"
                );
                // The internal balancer always succeeds (non-empty nodes guaranteed at construction)
                group.balancer.select(model)
            }
            None => Err(NoMatchingBackend {
                requested_model: model.map(|s| s.to_string()),
            }),
        }
    }

    fn strategy_name(&self) -> &'static str {
        "grouped"
    }

    fn all_nodes(&self) -> Vec<Arc<BackendNode>> {
        let mut all = Vec::new();
        for group in &self.groups {
            all.extend(group.balancer.all_nodes());
        }
        all
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::BackendNodeConfig;
    use std::collections::HashMap;

    fn make_group_config(mappings: Vec<&str>, urls: Vec<&str>) -> BackendGroupConfig {
        BackendGroupConfig {
            mappings: mappings.iter().map(|s| s.to_string()).collect(),
            strategy: "round_robin".to_string(),
            nodes: urls
                .iter()
                .map(|url| BackendNodeConfig {
                    url: url.to_string(),
                    timeout_seconds: 300,
                    tls: None,
                    model: None,
                    api_key: None,
                })
                .collect(),
        }
    }

    #[test]
    fn test_grouped_balancer_model_routing() {
        let mut groups = HashMap::new();
        groups.insert("opus".to_string(), make_group_config(vec!["opus", "opus4.5"], vec!["http://localhost:8080"]));
        groups.insert("haiku".to_string(), make_group_config(vec!["haiku"], vec!["http://localhost:8081"]));

        let balancer = GroupedLoadBalancer::new(groups).unwrap();

        // Request for opus should route to group 0
        let guard = balancer.select(Some("opus")).unwrap();
        assert_eq!(guard.node.base_url(), "http://localhost:8080");

        // Request for opus4.5 should also route to group 0
        let guard = balancer.select(Some("opus4.5")).unwrap();
        assert_eq!(guard.node.base_url(), "http://localhost:8080");

        // Request for haiku should route to group 1
        let guard = balancer.select(Some("haiku")).unwrap();
        assert_eq!(guard.node.base_url(), "http://localhost:8081");

        // Request for unknown model should fail (no catch-all)
        let result = balancer.select(Some("unknown"));
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().requested_model, Some("unknown".to_string()));

        // Request with no model should fail (no catch-all)
        let result = balancer.select(None);
        assert!(result.is_err());
        assert!(result.unwrap_err().requested_model.is_none());
    }

    #[test]
    fn test_grouped_balancer_catch_all() {
        let mut groups = HashMap::new();
        groups.insert("opus".to_string(), make_group_config(vec!["opus"], vec!["http://localhost:8080"]));
        groups.insert("catch_all".to_string(), make_group_config(vec![], vec!["http://localhost:8081"]));

        let balancer = GroupedLoadBalancer::new(groups).unwrap();

        // Request for opus should route to specific group
        let guard = balancer.select(Some("opus")).unwrap();
        assert_eq!(guard.node.base_url(), "http://localhost:8080");

        // Request for unknown model should fall back to catch-all
        let guard = balancer.select(Some("unknown")).unwrap();
        assert_eq!(guard.node.base_url(), "http://localhost:8081");

        // Request with no model should use catch-all
        let guard = balancer.select(None).unwrap();
        assert_eq!(guard.node.base_url(), "http://localhost:8081");
    }

    #[test]
    fn test_grouped_balancer_only_catch_all() {
        let mut groups = HashMap::new();
        groups.insert("catch_all".to_string(), make_group_config(vec![], vec!["http://localhost:8080"]));

        let balancer = GroupedLoadBalancer::new(groups).unwrap();

        // Any model should route to catch-all
        let guard = balancer.select(Some("anything")).unwrap();
        assert_eq!(guard.node.base_url(), "http://localhost:8080");

        let guard = balancer.select(None).unwrap();
        assert_eq!(guard.node.base_url(), "http://localhost:8080");
    }

    #[test]
    fn test_grouped_balancer_empty_groups() {
        let groups: HashMap<String, BackendGroupConfig> = HashMap::new();
        let balancer = GroupedLoadBalancer::new(groups).unwrap();

        // Should fail - no backends configured
        let result = balancer.select(Some("anything"));
        assert!(result.is_err());
    }

    #[test]
    fn test_grouped_balancer_multiple_nodes_in_group() {
        let mut groups = HashMap::new();
        groups.insert(
            "haiku".to_string(),
            BackendGroupConfig {
                mappings: vec!["haiku".to_string()],
                strategy: "round_robin".to_string(),
                nodes: vec![
                    BackendNodeConfig {
                        url: "http://localhost:8080".to_string(),
                        timeout_seconds: 300,
                        tls: None,
                        model: None,
                        api_key: None,
                    },
                    BackendNodeConfig {
                        url: "http://localhost:8081".to_string(),
                        timeout_seconds: 300,
                        tls: None,
                        model: None,
                        api_key: None,
                    },
                ],
            },
        );

        let balancer = GroupedLoadBalancer::new(groups).unwrap();

        // Round-robin within the group
        let guard1 = balancer.select(Some("haiku")).unwrap();
        let guard2 = balancer.select(Some("haiku")).unwrap();

        // Should cycle through nodes
        assert_ne!(guard1.node.base_url(), guard2.node.base_url());
    }

    #[test]
    fn test_strategy_name() {
        let groups: HashMap<String, BackendGroupConfig> = HashMap::new();
        let balancer = GroupedLoadBalancer::new(groups).unwrap();
        assert_eq!(balancer.strategy_name(), "grouped");
    }

    #[test]
    fn test_all_nodes() {
        let mut groups = HashMap::new();
        groups.insert("g1".to_string(), make_group_config(vec!["a"], vec!["http://localhost:8080"]));
        groups.insert("g2".to_string(), make_group_config(vec!["b"], vec!["http://localhost:8081"]));

        let balancer = GroupedLoadBalancer::new(groups).unwrap();
        let nodes = balancer.all_nodes();
        assert_eq!(nodes.len(), 2);
    }
}
