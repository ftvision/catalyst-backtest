//! The job worker: runs one queued backtest and records the outcome.
//!
//! Orchestration (compile → resolve policy → load/accept data → engine →
//! summarize) lives here, off the request path. The CPU-bound engine run is sent
//! to `spawn_blocking` so it never ties up the async HTTP worker threads.

use serde_json::Value;

use catalyst_graph_compiler::compile;
use catalyst_market_data_loader::load_bundle;
use catalyst_result_reporter::summarize;
use catalyst_simulation_engine::{run, SimulationInput};

use crate::state::{now, AppState};
use crate::support;

/// Run the job with id `id`, advancing its status and storing the result/error.
pub async fn run_job(state: &AppState, id: &str) {
    let Some(job) = state.get(id) else { return };
    state.update(id, |j| {
        j.status = "running".into();
        j.started_at = Some(now());
    });

    let req = job.request;

    // 1. Compile the graph.
    let compiled = match compile(&req.graph) {
        Ok(c) => c,
        Err(e) => return fail(state, id, format!("graph did not compile: {e}")),
    };

    // 2. Market data: inline, or load from the configured store.
    let bundle = match req.market_data {
        Some(b) => b,
        None => match state.store_root() {
            Some(root) => {
                let reference = support::bundle_ref(root, &compiled);
                // Load extra history before `start` so derived signals warm up.
                let load_start = support::warmup_start(
                    &req.config.start,
                    &req.config.interval,
                    compiled.data_requirements.lookback_bars,
                );
                match load_bundle(&reference, &load_start, &req.config.end, &req.config.interval)
                    .await
                {
                    Ok(b) => b,
                    Err(e) => return fail(state, id, format!("data load failed: {e}")),
                }
            }
            None => {
                return fail(state, id, "no market_data supplied and no store configured".into())
            }
        },
    };

    let providers = support::provider_values(&bundle);

    // 3. Run the engine off the async threads (CPU-bound).
    let input = SimulationInput {
        graph: req.graph,
        config: req.config,
        policy: req.policy,
        market_data: bundle,
    };
    let trace = match tokio::task::spawn_blocking(move || run(&input)).await {
        Ok(Ok(trace)) => trace,
        Ok(Err(e)) => return fail(state, id, format!("simulation error: {e}")),
        Err(join) => return fail(state, id, format!("worker panicked: {join}")),
    };

    // 4. Summarize and store.
    let result = summarize(&trace, providers.clone(), None);
    let result_json = serde_json::to_value(&result).unwrap_or(Value::Null);
    let summary = result_json.get("summary").cloned().unwrap_or(Value::Null);
    let warnings = trace.warnings.clone();
    let trace_json = serde_json::to_value(&trace).ok();

    state.update(id, |j| {
        j.status = "succeeded".into();
        j.finished_at = Some(now());
        j.summary = summary;
        j.result = Some(result_json);
        j.trace = trace_json;
        j.data_coverage = providers;
        j.warnings = warnings;
    });
}

fn fail(state: &AppState, id: &str, error: String) {
    state.update(id, |j| {
        j.status = "failed".into();
        j.error = Some(error);
        j.finished_at = Some(now());
    });
}
