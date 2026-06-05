//! Action execution models used by the simulator.
//!
//! Each model takes the action config, a read-only [`MarketContext`] for the
//! current tick, and the resolved policy, then drives the [`Ledger`] through its
//! explicit accounting operations and returns an [`Execution`] outcome (a
//! [`Fill`] or a rejection). Models never mutate global state directly and a
//! rejection leaves the ledger unchanged.
//!
//! Implemented venues/actions:
//! - EVM + Hyperliquid spot swaps ([`execute_swap`])
//! - Hyperliquid perp open/add and reduce-only close ([`execute_perp`])
//! - Aave-style yield deposit/withdraw ([`execute_yield_deposit`],
//!   [`execute_yield_withdraw`])

mod context;
mod outcome;
mod perp;
mod pricing;
mod swap;
mod yields;

pub use context::{Bar, MarketContext};
pub use outcome::{Execution, Fill};
pub use perp::execute_perp;
pub use pricing::{is_stable, Direction};
pub use swap::execute_swap;
pub use yields::{execute_yield_deposit, execute_yield_withdraw};

pub const CRATE_NAME: &str = "catalyst-execution-models";
