//! Deterministic graph simulation over normalized market data.
//!
//! The engine runs a tick/event loop: at each tick it accrues funding and yield,
//! checks perp liquidations, runs initial actions (once), evaluates signals and
//! executes the actions they trigger (following action→action chains), and
//! records a mark-to-market snapshot. The output is a [`catalyst_contracts::SimulationTrace`]
//! with the resolved policy embedded.
//!
//! The engine never fetches raw market data — it reads only the
//! [`catalyst_contracts::MarketDataBundle`] handed to it via [`SimulationInput`].

mod engine;
mod exec_graph;
mod market;

pub use engine::{run, EngineError, SimulationInput};
pub use exec_graph::{eval_threshold, ExecGraph};
pub use market::{format_ts, parse_ts, BundleIndex};

pub const CRATE_NAME: &str = "catalyst-simulation-engine";
