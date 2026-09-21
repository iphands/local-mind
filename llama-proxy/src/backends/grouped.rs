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
    /// Opts this group out of being the no-catch-all fallback target (E-M6).
    /// Driven by `exclusive` on the group's config (BackendGroupConfig,
    /// plumbed by the 87-follow); default false keeps every group a
    /// fallback candidate.
    exclusive: bool,
}

impl BackendGroup {
    fn maps_model(&self, model: &str) -> bool {
        self.mappings.iter().any(|m| m == model)
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
    /// Create a new grouped load balancer from group configurations.
    /// An empty map builds (server.rs boots a placeholder through this path);
    /// loader task 71 (W9-owned) adds the check-config-time rejection of empty
    /// `backends:`. Until then every select on an empty map is an honest
    /// NoMatchingBackend error — never a panic (pinned in tests, E-L10).
    pub fn new(
        group_configs: std::collections::HashMap<String, BackendGroupConfig>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut entries: Vec<(String, BackendGroupConfig)> = group_configs.into_iter().collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));

        let mut groups = Vec::with_capacity(entries.len());

        for (name, config) in entries {
            // Build BackendNode instances for this group
            let mut nodes = Vec::with_capacity(config.nodes.len());
            for node_cfg in &config.nodes {
                let node = BackendNode::from_config(
                    node_cfg.url.clone(),
                    node_cfg.timeout_seconds,
                    node_cfg.tls.as_ref(),
                    node_cfg.model.clone(),
                    node_cfg.api_key.clone(),
                    node_cfg.strip_path_prefix.clone(),
                    node_cfg.temperature,
                )?;
                nodes.push(Arc::new(node));
            }

            // Build the internal load balancer for this group
            let balancer = super::build_balancer_for_group(nodes, &config.strategy)?;

            groups.push(BackendGroup {
                mappings: config.mappings,
                balancer,
                name,
                exclusive: config.exclusive,
            });
        }

        Ok(Self { groups })
    }

    /// Find a group for the given model, in priority order (E-M2, E-M6):
    /// 1. first group (sorted by name) that specifically maps the model
    /// 2. the catch-all group (empty mappings)
    /// 3. the first non-exclusive group, with a warn — add a `mappings: []`
    ///    catch-all to opt out of the positional fallback
    ///
    /// Groups are stored sorted, so every rung (and any duplicate mapping,
    /// which loader task 71 will reject outright) resolves deterministically.
    fn find_group(&self, model: Option<&str>) -> Option<&BackendGroup> {
        if let Some(model_str) = model {
            for group in &self.groups {
                if !group.is_catch_all() && group.maps_model(model_str) {
                    return Some(group);
                }
            }
        }
        for group in &self.groups {
            if group.is_catch_all() {
                return Some(group);
            }
        }
        let fallback = self.groups.iter().find(|g| !g.exclusive)?;
        tracing::warn!(
            group = %fallback.name,
            model = ?model,
            "No catch-all group configured — routing unmatched request to first sorted group; \
            add a group with 'mappings: []' to own unmatched traffic explicitly"
        );
        Some(fallback)
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
                let mut guard = group.balancer.select(model)?;
                // Attach group name to the guard
                guard.group_name = Some(group.name.clone());
                Ok(guard)
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
            failure_cooldown_secs: 30,
            exclusive: false,
            nodes: urls
                .iter()
                .map(|url| BackendNodeConfig {
                    url: url.to_string(),
                    timeout_seconds: 300,
                    tls: None,
                    model: None,
                    api_key: None,
                    strip_path_prefix: None,
                    temperature: None,
                })
                .collect(),
        }
    }

    #[test]
    fn test_grouped_balancer_model_routing() {
        let mut groups = HashMap::new();
        groups.insert(
            "opus".to_string(),
            make_group_config(vec!["opus", "opus4.5"], vec!["http://localhost:8080"]),
        );
        groups.insert(
            "haiku".to_string(),
            make_group_config(vec!["haiku"], vec!["http://localhost:8081"]),
        );

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

        // No catch-all: unmatched traffic falls back to the FIRST SORTED group ("haiku" < "opus")
        let guard = balancer.select(Some("unknown")).unwrap();
        assert_eq!(guard.node.base_url(), "http://localhost:8081");

        let guard = balancer.select(None).unwrap();
        assert_eq!(guard.node.base_url(), "http://localhost:8081");
    }

    #[test]
    fn test_grouped_balancer_catch_all() {
        let mut groups = HashMap::new();
        groups.insert(
            "opus".to_string(),
            make_group_config(vec!["opus"], vec!["http://localhost:8080"]),
        );
        groups.insert(
            "catch_all".to_string(),
            make_group_config(vec![], vec!["http://localhost:8081"]),
        );

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
        groups.insert(
            "catch_all".to_string(),
            make_group_config(vec![], vec!["http://localhost:8080"]),
        );

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
        let balancer = GroupedLoadBalancer::new(groups)
            .expect("empty map must still BUILD — server placeholder path and loader-71 own the rejection");

        let result = balancer.select(Some("anything"));
        assert!(result.is_err(), "select on empty groups is an honest error, not a panic");
        assert!(balancer.select(None).is_err());
        assert!(balancer.all_nodes().is_empty());
    }

    #[test]
    fn test_grouped_balancer_multiple_nodes_in_group() {
        let mut groups = HashMap::new();
        groups.insert(
            "haiku".to_string(),
            BackendGroupConfig {
                mappings: vec!["haiku".to_string()],
                strategy: "round_robin".to_string(),
                failure_cooldown_secs: 30,
                exclusive: false,
                nodes: vec![
                    BackendNodeConfig {
                        url: "http://localhost:8080".to_string(),
                        timeout_seconds: 300,
                        tls: None,
                        model: None,
                        api_key: None,
                        strip_path_prefix: None,
                        temperature: None,
                    },
                    BackendNodeConfig {
                        url: "http://localhost:8081".to_string(),
                        timeout_seconds: 300,
                        tls: None,
                        model: None,
                        api_key: None,
                        strip_path_prefix: None,
                        temperature: None,
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

    #[test]
    fn duplicate_mapping_resolves_to_the_first_sorted_group() {
        let mut groups = HashMap::new();
        groups.insert(
            "zeta".to_string(),
            make_group_config(vec!["dup"], vec!["http://localhost:8080"]),
        );
        groups.insert(
            "alpha".to_string(),
            make_group_config(vec!["dup"], vec!["http://localhost:8081"]),
        );

        let balancer = GroupedLoadBalancer::new(groups).unwrap();
        let guard = balancer.select(Some("dup")).unwrap();
        assert_eq!(
            guard.group_name.as_deref(),
            Some("alpha"),
            "loader task 71 will REJECT duplicate mappings; until then sorted-first wins deterministically"
        );
    }

    #[test]
    fn no_catch_all_falls_back_to_first_sorted_group() {
        let mut groups = HashMap::new();
        groups.insert(
            "zulu".to_string(),
            make_group_config(vec!["m1"], vec!["http://localhost:8080"]),
        );
        groups.insert(
            "alpha".to_string(),
            make_group_config(vec!["m2"], vec!["http://localhost:8081"]),
        );

        let balancer = GroupedLoadBalancer::new(groups).unwrap();
        assert_eq!(balancer.select(Some("other")).unwrap().group_name.as_deref(), Some("alpha"));
        assert_eq!(balancer.select(None).unwrap().group_name.as_deref(), Some("alpha"));
        assert_eq!(balancer.select(Some("m2")).unwrap().group_name.as_deref(), Some("alpha"));
        assert_eq!(balancer.select(Some("m1")).unwrap().group_name.as_deref(), Some("zulu"));
    }

    #[test]
    fn catch_all_outranks_positional_fallback() {
        let mut groups = HashMap::new();
        groups.insert(
            "aaa_specific".to_string(),
            make_group_config(vec!["mine"], vec!["http://localhost:8080"]),
        );
        groups.insert(
            "zzz_catch".to_string(),
            make_group_config(vec![], vec!["http://localhost:8081"]),
        );

        let balancer = GroupedLoadBalancer::new(groups).unwrap();
        assert_eq!(
            balancer.select(Some("nope")).unwrap().group_name.as_deref(),
            Some("zzz_catch"),
            "an explicit catch-all beats positional fallback even when it sorts last"
        );
        assert_eq!(
            balancer.select(Some("mine")).unwrap().group_name.as_deref(),
            Some("aaa_specific")
        );
    }

    #[test]
    fn exclusive_groups_are_not_fallback_targets() {
        let node =
            Arc::new(BackendNode::from_config("http://localhost:8080".to_string(), 300, None, None, None, None, None).unwrap());
        let lb = GroupedLoadBalancer {
            groups: vec![BackendGroup {
                mappings: vec!["m1".to_string()],
                balancer: crate::backends::build_balancer_for_group(vec![node], "round_robin").unwrap(),
                name: "solo".to_string(),
                exclusive: true,
            }],
        };
        assert_eq!(
            lb.select(Some("m1")).unwrap().node.base_url(),
            "http://localhost:8080",
            "exclusive still serves its own mappings"
        );
        assert!(
            lb.select(Some("other")).is_err(),
            "exclusive opted out of positional fallback"
        );
        assert!(lb.select(None).is_err());
    }

    #[test]
    fn exclusive_from_config_is_not_fallback_target() {
        // The struct-literal pin above cannot catch a plumbing regression
        // (GroupedLoadBalancer::new ignoring BackendGroupConfig.exclusive);
        // this one drives the SAME semantics through the config path.
        let mut cfg = make_group_config(vec!["m1"], vec!["http://localhost:8080"]);
        cfg.exclusive = true;
        let mut groups = HashMap::new();
        groups.insert("solo".to_string(), cfg);

        let balancer = GroupedLoadBalancer::new(groups).unwrap();
        assert_eq!(
            balancer.select(Some("m1")).unwrap().node.base_url(),
            "http://localhost:8080",
            "exclusive group still serves its own mappings"
        );
        assert!(
            balancer.select(Some("other")).is_err(),
            "config exclusive: true must reach find_group — no positional fallback"
        );
        assert!(balancer.select(None).is_err());
    }

    #[test]
    fn exclusive_group_sorting_first_yields_fallback_to_first_inclusive_group() {
        let mut aaa = make_group_config(vec!["m1"], vec!["http://localhost:8080"]);
        aaa.exclusive = true;
        let mut groups = HashMap::new();
        groups.insert("aaa".to_string(), aaa);
        groups.insert(
            "bbb".to_string(),
            make_group_config(vec!["m2"], vec!["http://localhost:8081"]),
        );

        let balancer = GroupedLoadBalancer::new(groups).unwrap();
        assert_eq!(
            balancer.select(Some("nope")).unwrap().group_name.as_deref(),
            Some("bbb"),
            "sorted-first aaa opted out; the positional fallback moves to the first non-exclusive group"
        );
        assert_eq!(balancer.select(Some("m1")).unwrap().group_name.as_deref(), Some("aaa"));
    }

    #[test]
    fn catch_all_group_with_exclusive_still_catches_all() {
        // f414d16 semantics pinned: the catch-all rung never consults
        // exclusive — a group with empty mappings owns unmatched traffic by
        // definition, so exclusive on it is a documented no-op (the honest
        // loader-side rejection of this combo is a recorded carry).
        let mut catch = make_group_config(vec![], vec!["http://localhost:8081"]);
        catch.exclusive = true;
        let mut groups = HashMap::new();
        groups.insert(
            "aaa_specific".to_string(),
            make_group_config(vec!["mine"], vec!["http://localhost:8080"]),
        );
        groups.insert("zzz_catch".to_string(), catch);

        let balancer = GroupedLoadBalancer::new(groups).unwrap();
        assert_eq!(
            balancer.select(Some("nope")).unwrap().group_name.as_deref(),
            Some("zzz_catch")
        );
        assert_eq!(balancer.select(None).unwrap().group_name.as_deref(), Some("zzz_catch"));
    }
}
