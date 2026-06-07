use std::error::Error;
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
}

#[derive(Debug, Deserialize)]
struct CatalogScenario {
    id: String,
    scenario: PathBuf,
}

#[derive(Debug, Deserialize)]
struct Scenario {
    config: BacktestConfig,
    policy: SimulationPolicy,
    market_data: MarketDataBundle,
}

fn load_json<T: DeserializeOwned>(path: &Path) -> Result<T, Box<dyn Error>> {
    let body = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&body)?)
}

fn event_count(trace: &catalyst_contracts::SimulationTrace, kind: &str) -> usize {
    trace
        .events
        .iter()
        .filter(|event| event.event_type == kind)
        .count()
}

fn main() -> Result<(), Box<dyn Error>> {
    let dataset_dir = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "strategies".to_string());
    let dataset_dir = PathBuf::from(dataset_dir);
    let catalog: Catalog = load_json(&dataset_dir.join("catalog.json"))?;

    println!("strategy_id,scenario_id,snapshots,signals,executed,rejected,liquidations,final_equity_usd,warnings");

    for strategy in &catalog.strategies {
        let graph: Graph = load_json(&dataset_dir.join(&strategy.graph))?;

        for scenario_ref in &catalog.scenarios {
            let scenario: Scenario = load_json(&dataset_dir.join(&scenario_ref.scenario))?;
            let trace = run(&SimulationInput {
                graph: graph.clone(),
                config: scenario.config,
                policy: scenario.policy,
                market_data: scenario.market_data,
            })?;
            let final_equity = trace
                .snapshots
                .last()
                .map(|snapshot| snapshot.equity_usd.to_string())
                .unwrap_or_else(|| "0".to_string());

            println!(
                "{},{},{},{},{},{},{},{},{}",
                strategy.id,
                scenario_ref.id,
                trace.snapshots.len(),
                event_count(&trace, "signal_fired"),
                event_count(&trace, "action_executed"),
                event_count(&trace, "action_rejected"),
                event_count(&trace, "liquidation"),
                final_equity,
                trace.warnings.len()
            );
        }
    }

    Ok(())
}
