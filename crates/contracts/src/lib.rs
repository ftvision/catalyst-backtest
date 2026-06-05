//! Shared Rust contract types for Catalyst backtesting.
//!
//! These `serde` structs are kept aligned with the language-neutral JSON Schemas
//! in the repo `schemas/` directory and mirror the Python models in
//! `packages/contracts`. Decimal/quantity values are carried as `String`
//! ([`Decimal`]) to preserve precision across the Python <-> JSON <-> Rust
//! boundary.

pub mod graph;
pub mod market_data;
pub mod policy;
pub mod request;
pub mod result;
pub mod trace;

/// A decimal value carried as a string to preserve precision on the wire.
pub type Decimal = String;

pub use graph::{Edge, Graph, Node};
pub use market_data::MarketDataBundle;
pub use policy::SimulationPolicy;
pub use request::{BacktestConfig, BacktestRequest};
pub use result::BacktestResult;
pub use trace::{Portfolio, SimulationTrace};

pub const CRATE_NAME: &str = "catalyst-contracts";
