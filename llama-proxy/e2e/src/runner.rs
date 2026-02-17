//! Test runner - executes tests and reports results

use colored::Colorize;
use std::time::Instant;

use crate::types::{SharedBackendState, TestResult};

/// A single test case
pub struct TestCase {
    pub name: &'static str,
    pub description: &'static str,
    pub run: Box<
        dyn Fn(TestContext) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send>> + Send + Sync,
    >,
}

/// Context passed to each test - contains proxy address and backend state
#[derive(Clone)]
pub struct TestContext {
    pub proxy_addr: String,
    pub backend_state: SharedBackendState,
    pub http_client: reqwest::Client,
}

/// Run all provided test cases sequentially and report results
pub async fn run_tests(cases: Vec<TestCase>, ctx: TestContext, filter: Option<&str>) -> Vec<TestResult> {
    let mut results = Vec::new();
    let mut passed = 0;
    let mut failed = 0;

    println!("\n{}", "═══════════════════════════════════════════════════".bright_blue());
    println!("{}", "  llama-proxy End-to-End Tests".bright_white().bold());
    println!("{}", "═══════════════════════════════════════════════════".bright_blue());
    println!("  Proxy:   {}", ctx.proxy_addr.bright_cyan());

    // Filter tests if requested
    let cases_to_run: Vec<&TestCase> = if let Some(f) = filter {
        cases.iter().filter(|c| c.name.contains(f)).collect()
    } else {
        cases.iter().collect()
    };

    println!("  Running: {} test(s)\n", cases_to_run.len().to_string().bright_cyan());

    for case in &cases_to_run {
        // Reset backend state before each test
        {
            let mut state = ctx.backend_state.lock().unwrap();
            state.response_queue.clear();
            state.received_requests.clear();
        }

        let start = Instant::now();
        print!("  {} {} ... ", "▶".bright_blue(), case.name.bright_white());

        let ctx_clone = ctx.clone();
        let fut = (case.run)(ctx_clone);
        let result = fut.await;
        let duration_ms = start.elapsed().as_millis() as u64;

        let test_result = match result {
            Ok(()) => {
                println!("{} ({duration_ms}ms)", "PASS".bright_green().bold());
                passed += 1;
                TestResult {
                    name: case.name.to_string(),
                    passed: true,
                    error: None,
                    duration_ms,
                }
            }
            Err(e) => {
                println!("{} ({duration_ms}ms)", "FAIL".bright_red().bold());
                println!("    {} {}", "Error:".bright_red(), e);
                // Print cause chain
                let mut src = e.source();
                while let Some(cause) = src {
                    println!("    {} {}", "Caused by:".yellow(), cause);
                    src = cause.source();
                }
                failed += 1;
                TestResult {
                    name: case.name.to_string(),
                    passed: false,
                    error: Some(e.to_string()),
                    duration_ms,
                }
            }
        };

        results.push(test_result);
    }

    println!("\n{}", "───────────────────────────────────────────────────".bright_blue());
    let summary = format!("  Results: {} passed, {} failed", passed, failed);
    if failed == 0 {
        println!("{}", summary.bright_green().bold());
    } else {
        println!("{}", summary.bright_red().bold());
    }
    println!("{}\n", "═══════════════════════════════════════════════════".bright_blue());

    results
}

/// Helper to list all available tests
pub fn list_tests(cases: &[TestCase]) {
    println!("\n{}", "Available tests:".bright_white().bold());
    for case in cases {
        println!("  {} - {}", case.name.bright_cyan(), case.description);
    }
    println!();
}
