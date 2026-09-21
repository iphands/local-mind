//! Priority-free load balancing strategy
//!
//! Always dispatches to the lowest-index backend that is not currently handling a request.
//! If all backends are busy, picks the one with the fewest active requests (lowest index wins ties).

use std::sync::atomic::Ordering;
use std::sync::Arc;

use super::balancer::{BackendGuard, LoadBalancer};
use super::node::BackendNode;
use crate::config::NoMatchingBackend;

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
    fn select(&self, _model: Option<&str>) -> Result<BackendGuard, NoMatchingBackend> {
        // Model routing is handled by GroupedLoadBalancer; this balancer just selects from its nodes
        // Claim the first node that is idle. compare_exchange is the atomic test-and-set: it both
        // observes 0 and reserves the slot, so two concurrent selects can never both free-claim the
        // same idle node. Lowest index is attempted first, so the first free node wins ties.
        for node in &self.nodes {
            if node
                .active_requests
                .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                // The CAS already recorded the +1; build the guard directly so its Drop is the only
                // matching decrement (BackendGuard::new would add a second one).
                return Ok(BackendGuard {
                    node: node.clone(),
                    group_name: None,
                });
            }
        }

        // All busy: increment-then-backtrack to find the least-loaded node with no unclaimed window.
        // Every candidate is reserved (fetch_add) the instant it is considered, so a concurrent
        // select never mistakes one for free mid-scan; the fetch_add return value is that candidate's
        // load at arrival. Only a strictly lower load supersedes the current best, so the lowest index
        // wins ties. After the scan the winner keeps its reservation and every candidate the winner
        // was chosen over is released (fetch_sub). Releasing ALL non-selected candidates (not just
        // those momentarily held as best) is what makes each select net exactly +1: a candidate that
        // tied the best was still reserved and must be released, or it leaks a phantom +1.
        let mut best_idx = 0usize;
        let mut best_load = self.nodes[0].active_requests.fetch_add(1, Ordering::AcqRel);
        for (idx, node) in self.nodes.iter().enumerate().skip(1) {
            let load = node.active_requests.fetch_add(1, Ordering::AcqRel);
            if load < best_load {
                best_idx = idx;
                best_load = load;
            }
        }
        for (idx, node) in self.nodes.iter().enumerate() {
            if idx != best_idx {
                node.active_requests.fetch_sub(1, Ordering::AcqRel);
            }
        }

        Ok(BackendGuard {
            node: self.nodes[best_idx].clone(),
            group_name: None,
        })
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
            strip_path_prefix: None,
            temperature: None,
        })
    }

    #[test]
    fn test_single_node_always_selected() {
        let nodes = vec![make_test_node("http://localhost:8080")];
        let balancer = PriorityFreeBalancer::new(nodes).unwrap();
        assert_eq!(balancer.select(None).unwrap().node.base_url(), "http://localhost:8080");
        assert_eq!(balancer.select(None).unwrap().node.base_url(), "http://localhost:8080");
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
        assert_eq!(balancer.select(None).unwrap().node.base_url(), "http://localhost:8080");
        assert_eq!(balancer.select(None).unwrap().node.base_url(), "http://localhost:8080");
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
        assert_eq!(balancer.select(None).unwrap().node.base_url(), "http://localhost:8082");
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
        assert_eq!(balancer.select(None).unwrap().node.base_url(), "http://localhost:8081");
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
        assert_eq!(balancer.select(None).unwrap().node.base_url(), "http://localhost:8080");
    }

    #[test]
    fn test_guard_decrements_on_drop() {
        let node = make_test_node("http://localhost:8080");
        let nodes = vec![node.clone()];
        let balancer = PriorityFreeBalancer::new(nodes).unwrap();

        assert_eq!(node.active_requests.load(Ordering::Relaxed), 0);
        {
            let _guard = balancer.select(None).unwrap();
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
        assert_eq!(balancer.select(None).unwrap().node.base_url(), "http://localhost:8082");
    }

    #[test]
    fn test_model_parameter_ignored() {
        // Model parameter is now ignored - routing is handled by GroupedLoadBalancer
        let nodes = vec![
            make_test_node("http://localhost:8080"),
            make_test_node("http://localhost:8081"),
        ];

        // Mark node 0 as busy
        nodes[0].active_requests.store(1, Ordering::Relaxed);

        let balancer = PriorityFreeBalancer::new(nodes).unwrap();

        // All selects should use priority_free logic regardless of model
        assert_eq!(
            balancer.select(Some("haiku")).unwrap().node.base_url(),
            "http://localhost:8081"
        );
        assert_eq!(
            balancer.select(Some("sonnet")).unwrap().node.base_url(),
            "http://localhost:8081"
        );
        assert_eq!(balancer.select(None).unwrap().node.base_url(), "http://localhost:8081");
    }

    /// Free-claim concurrency invariant harness.
    ///
    /// A free-claim "winner" is a contender that observes the idle node free AND successfully claims
    /// it. The double-claim bug lives in that CHECK->CLAIM decision, which is deliberately left
    /// unobservable through the public `select()` (a racy and an atomic claim both return exactly one
    /// guard), so this harness models the free path at its decision point and counts successful
    /// claims. `claim_step(node)` MUST mirror select()'s free-claim and report whether the claim
    /// SUCCEEDED. A non-atomic claim (`fetch_add`) always succeeds, so all barrier-released contenders
    /// win (the bug); the atomic `compare_exchange` succeeds for exactly one.
    ///
    /// Returns `(successful_free_claims_per_round, real_selects)`. `real_selects` additionally drives
    /// the REAL `select()` under identical contention and counts guards produced + confirms the node
    /// settles to idle, so the guard/counter wiring stays exercised.
    fn drive_free_claim_rounds<F>(iters: usize, claim_step: &F) -> (Vec<usize>, usize)
    where
        F: Fn(&BackendNode) -> bool + Sync,
    {
        const THREADS: usize = 8;
        let mut claims_per_round = Vec::with_capacity(iters);
        let mut real_selects = 0usize;

        for _ in 0..iters {
            let claim_node = make_test_node("http://localhost:8080");
            let claims = Arc::new(AtomicUsize::new(0));
            let barrier = Arc::new(std::sync::Barrier::new(THREADS));

            std::thread::scope(|scope| {
                for _ in 0..THREADS {
                    let claim_node = Arc::clone(&claim_node);
                    let claims = Arc::clone(&claims);
                    let barrier = Arc::clone(&barrier);
                    scope.spawn(move || {
                        barrier.wait();
                        if claim_node.active_requests.load(Ordering::Acquire) == 0 && claim_step(&claim_node) {
                            claims.fetch_add(1, Ordering::AcqRel);
                        }
                    });
                }
            });

            claims_per_round.push(claims.load(Ordering::Acquire));

            // Drive the real select() under the same contention; guards settle the node to idle.
            let node = make_test_node("http://localhost:8081");
            let balancer = Arc::new(PriorityFreeBalancer::new(vec![node.clone()]).unwrap());
            let select_barrier = Arc::new(std::sync::Barrier::new(THREADS));
            let selected = Arc::new(AtomicUsize::new(0));
            std::thread::scope(|scope| {
                for _ in 0..THREADS {
                    let balancer = Arc::clone(&balancer);
                    let select_barrier = Arc::clone(&select_barrier);
                    let selected = Arc::clone(&selected);
                    scope.spawn(move || {
                        select_barrier.wait();
                        let guard = balancer.select(None).expect("select must return a node");
                        selected.fetch_add(1, Ordering::AcqRel);
                        drop(guard);
                    });
                }
            });
            real_selects += selected.load(Ordering::Acquire);
            assert_eq!(
                node.active_requests.load(Ordering::Acquire),
                0,
                "real select() must settle the node to idle after all guards drop"
            );
        }

        (claims_per_round, real_selects)
    }

    #[test]
    fn test_free_claim_has_exactly_one_winner_stress() {
        const ITERS: usize = 1000;
        const THREADS: usize = 8;

        // Mirrors select()'s atomic free-claim: the compare_exchange test-and-set succeeds for the
        // single contender that actually reserved the idle node.
        let atomic_claim = |node: &BackendNode| -> bool {
            node.active_requests
                .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
        };

        let (claims_per_round, real_selects) = drive_free_claim_rounds(ITERS, &atomic_claim);
        let double_claim_rounds = claims_per_round.iter().filter(|&&c| c > 1).count();
        let no_claim_rounds = claims_per_round.iter().filter(|&&c| c == 0).count();
        let max_claims = *claims_per_round.iter().max().unwrap_or(&0);

        println!(
            "[task17 stress] iters={ITERS} threads={THREADS} double_claim_rounds={double_claim_rounds} \
             no_claim_rounds={no_claim_rounds} max_free_claims_in_a_round={max_claims} \
             real_selects_ok={real_selects} (expected {})",
            ITERS * THREADS
        );

        assert_eq!(
            real_selects,
            ITERS * THREADS,
            "every barrier-released select() must yield a guard"
        );
        assert_eq!(
            no_claim_rounds, 0,
            "an idle node must always be free-claimed by at least one contender"
        );
        assert_eq!(
            double_claim_rounds, 0,
            "double free-claim: {double_claim_rounds}/{ITERS} rounds let >1 contender claim the same \
             idle node (worst round had {max_claims} simultaneous free-claims)"
        );
    }

    #[test]
    fn test_all_busy_concurrent_selects_net_one_increment_per_guard() {
        const THREADS: usize = 8;
        // Baseline >= THREADS: the all-busy path increments a candidate then backtracks it, so a
        // node's counter can transiently drop back toward baseline. A baseline this high keeps every
        // node strictly > 0 while only these THREADS selects are in flight, pinning every select onto
        // the all-busy increment-then-backtrack path (the free-claim CAS at 0 is never reachable).
        const BASELINE_A: usize = THREADS;
        const BASELINE_B: usize = THREADS;

        let node_a = make_test_node("http://localhost:8080");
        let node_b = make_test_node("http://localhost:8081");
        node_a.active_requests.store(BASELINE_A, Ordering::Relaxed);
        node_b.active_requests.store(BASELINE_B, Ordering::Relaxed);

        let balancer = Arc::new(PriorityFreeBalancer::new(vec![node_a.clone(), node_b.clone()]).unwrap());
        let barrier = Arc::new(std::sync::Barrier::new(THREADS));
        let guards: Arc<std::sync::Mutex<Vec<BackendGuard>>> = Arc::new(std::sync::Mutex::new(Vec::new()));

        std::thread::scope(|scope| {
            for _ in 0..THREADS {
                let balancer = Arc::clone(&balancer);
                let barrier = Arc::clone(&barrier);
                let guards = Arc::clone(&guards);
                scope.spawn(move || {
                    barrier.wait();
                    let guard = balancer.select(None).expect("select must return a node");
                    guards.lock().unwrap().push(guard);
                });
            }
        });

        // All 8 selects have returned and their guards are retained, so the node set has netted
        // exactly +1 per live guard: every backtrack has unwound to 0 and only each winner's single
        // retained increment remains.
        let baseline_total = BASELINE_A + BASELINE_B;
        let held_total = node_a.active_requests.load(Ordering::Acquire) + node_b.active_requests.load(Ordering::Acquire);
        assert_eq!(
            held_total,
            baseline_total + THREADS,
            "all-busy path must net exactly +1 across the node set per live guard (backtracks net to 0)"
        );

        drop(guards);

        let settled_a = node_a.active_requests.load(Ordering::Acquire);
        let settled_b = node_b.active_requests.load(Ordering::Acquire);
        assert_eq!(
            (settled_a, settled_b),
            (BASELINE_A, BASELINE_B),
            "after all guards drop both counters must return exactly to their busy baseline"
        );
    }
}
