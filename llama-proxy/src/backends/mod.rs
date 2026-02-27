//! Multi-backend load balancing

mod balancer;
mod node;
mod round_robin;

pub use balancer::LoadBalancer;
pub use node::BackendNode;
pub use round_robin::RoundRobinBalancer;

use std::sync::Arc;

/// Build a load balancer from a list of nodes and a strategy name
pub fn build_balancer(
    nodes: Vec<BackendNode>,
    strategy: &str,
) -> Result<Arc<dyn LoadBalancer>, Box<dyn std::error::Error>> {
    let arc_nodes: Vec<Arc<BackendNode>> = nodes.into_iter().map(Arc::new).collect();
    match strategy {
        "round_robin" => Ok(Arc::new(RoundRobinBalancer::new(arc_nodes)?)),
        other => Err(format!("Unknown load balancer strategy: '{}'. Supported: round_robin", other).into()),
    }
}
