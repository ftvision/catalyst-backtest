//! Conformance harness: run the engine over the shared golden fixtures in
//! `tests/golden/` and assert the expected invariants.
//!
//! The golden files are language-neutral (engine input + expected invariants),
//! so a future re-implementation can be checked against the same suite.

use std::fs;
use std::path::PathBuf;

use serde_json::Value;

use catalyst_contracts::{BacktestConfig, Graph, MarketDataBundle, SimulationPolicy};
use catalyst_simulation_engine::{run, SimulationInput};

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/golden")
        .canonicalize()
        .expect("tests/golden exists")
}

fn count_events(trace_events: &[catalyst_contracts::trace::Event], kind: &str) -> usize {
    trace_events.iter().filter(|e| e.event_type == kind).count()
}

#[test]
fn golden_cases_conform() {
    let mut files: Vec<_> = fs::read_dir(golden_dir())
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map(|x| x == "json").unwrap_or(false))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no golden fixtures found");

    for path in files {
        let doc: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap())
            .unwrap_or_else(|e| panic!("parse {path:?}: {e}"));
        let name = doc["name"].as_str().unwrap_or("<unnamed>");
        let input = &doc["input"];
        let expect = &doc["expect"];

        let graph: Graph = serde_json::from_value(input["graph"].clone()).unwrap();
        let config: BacktestConfig = serde_json::from_value(input["config"].clone()).unwrap();
        let policy: SimulationPolicy = serde_json::from_value(input["policy"].clone()).unwrap();
        let market_data: MarketDataBundle =
            serde_json::from_value(input["market_data"].clone()).unwrap();

        let trace = run(&SimulationInput { graph, config, policy, market_data })
            .unwrap_or_else(|e| panic!("[{name}] engine error: {e}"));

        let executed = count_events(&trace.events, "action_executed");
        let rejected = count_events(&trace.events, "action_rejected");

        assert_eq!(executed as u64, expect["executed"].as_u64().unwrap(), "[{name}] executed");
        assert_eq!(rejected as u64, expect["rejected"].as_u64().unwrap(), "[{name}] rejected");

        if let Some(fired) = expect.get("signals_fired").and_then(|v| v.as_u64()) {
            assert_eq!(
                count_events(&trace.events, "signal_fired") as u64,
                fired,
                "[{name}] signals_fired"
            );
        }

        assert_eq!(
            trace.final_portfolio.perp_positions.len() as u64,
            expect["open_perps"].as_u64().unwrap(),
            "[{name}] open_perps"
        );
        assert_eq!(
            trace.final_portfolio.yield_positions.len() as u64,
            expect["open_yields"].as_u64().unwrap(),
            "[{name}] open_yields"
        );

        if let Some(min) = expect.get("snapshots_min").and_then(|v| v.as_u64()) {
            assert!(trace.snapshots.len() as u64 >= min, "[{name}] snapshots_min");
        }

        for pair in expect["balances_present"].as_array().unwrap() {
            let venue = pair[0].as_str().unwrap();
            let asset = pair[1].as_str().unwrap();
            let present = trace
                .final_portfolio
                .balances
                .get(venue)
                .and_then(|a| a.get(asset))
                .is_some();
            assert!(present, "[{name}] expected balance {venue}/{asset} present");
        }
    }
}
