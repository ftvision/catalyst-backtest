//! Strategy repository smoke test.
//!
//! The catalog under `strategies/` is user-facing fixture data, not just test
//! data. This test keeps every catalog strategy executable against every
//! catalog scenario with the deterministic Rust engine.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use catalyst_contracts::{BacktestConfig, Graph, MarketDataBundle, SimulationPolicy};
use catalyst_simulation_engine::{run, SimulationInput};
use serde::de::DeserializeOwned;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Catalog {
    strategies: Vec<CatalogStrategy>,
    scenarios: Vec<CatalogScenario>,
}

#[derive(Debug, Deserialize)]
struct CatalogStrategy {
    id: String,
    graph: PathBuf,
    source: String,
}

#[derive(Debug, Deserialize)]
struct CatalogScenario {
    id: String,
    scenario: PathBuf,
}

#[derive(Debug, Deserialize)]
struct Scenario {
    id: String,
    config: BacktestConfig,
    policy: SimulationPolicy,
    market_data: MarketDataBundle,
}

fn strategies_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../strategies")
        .canonicalize()
        .expect("strategies directory exists")
}

fn load_json<T: DeserializeOwned>(path: &Path) -> T {
    let body = fs::read_to_string(path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
    serde_json::from_str(&body).unwrap_or_else(|e| panic!("parse {path:?}: {e}"))
}

#[test]
fn strategy_catalog_contains_the_pasted_graphs() {
    let root = strategies_dir();
    let catalog: Catalog = load_json(&root.join("catalog.json"));
    let sources: BTreeSet<_> = catalog
        .strategies
        .iter()
        .map(|strategy| strategy.source.as_str())
        .collect();

    for n in 1..=15 {
        let source = format!("pasted_graph_{n}");
        assert!(sources.contains(source.as_str()), "missing {source}");
    }
    assert!(
        catalog.scenarios.len() >= 3,
        "expected a few market scenarios"
    );
}

#[test]
fn catalog_strategies_run_against_catalog_scenarios() {
    let root = strategies_dir();
    let catalog: Catalog = load_json(&root.join("catalog.json"));
    assert!(!catalog.strategies.is_empty(), "strategy catalog is empty");
    assert!(!catalog.scenarios.is_empty(), "scenario catalog is empty");

    let mut runs = 0;
    for strategy in catalog.strategies {
        let graph: Graph = load_json(&root.join(&strategy.graph));

        for scenario_ref in &catalog.scenarios {
            let scenario: Scenario = load_json(&root.join(&scenario_ref.scenario));
            assert_eq!(
                scenario.id, scenario_ref.id,
                "scenario file id should match catalog id"
            );
            let trace = run(&SimulationInput {
                graph: graph.clone(),
                config: scenario.config,
                policy: scenario.policy,
                market_data: scenario.market_data,
            })
            .unwrap_or_else(|e| panic!("{} x {} failed: {e}", strategy.id, scenario.id));

            assert!(
                !trace.snapshots.is_empty(),
                "{} x {} produced no snapshots",
                strategy.id,
                scenario.id
            );
            assert!(
                trace.errors.is_empty(),
                "{} x {} produced trace errors: {:?}",
                strategy.id,
                scenario.id,
                trace.errors
            );
            runs += 1;
        }
    }

    assert_eq!(
        runs, 45,
        "expected fifteen strategies across three scenarios"
    );
}
