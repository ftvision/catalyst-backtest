//! Centralized, versioned simulation policy rules.
//!
//! The simulation engine makes every ambiguous decision (insufficient balance,
//! fill price, slippage, signal triggering, missing data, ...) through a single
//! [`ResolvedPolicy`]. Named profiles ([`Profile`]) supply complete defaults;
//! a partial [`catalyst_contracts::SimulationPolicy`] can override individual
//! knobs on top of a profile via [`resolve_policy`].
//!
//! All enums deserialize from the same snake_case strings used in
//! `simulation-policy.schema.json`, so a [`ResolvedPolicy`] round-trips through
//! JSON and can be embedded in a simulation trace.

mod profiles;
mod resolve;

pub use profiles::{conservative_v1, research_v1, strict_v1};
pub use resolve::{resolve, resolve_policy, validate, PolicyError};

use serde::{Deserialize, Serialize};

pub const CRATE_NAME: &str = "catalyst-simulation-policies";

macro_rules! str_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum $name {
            $($variant),+
        }
    };
}

str_enum!(
    /// Named policy profile.
    Profile { StrictV1, ConservativeV1, ResearchV1 }
);

str_enum!(
    /// What happens when an action requests more of an asset than is available.
    InsufficientBalance { Reject, PartialFill, ClampToAvailable, AllowNegative }
);

str_enum!(
    /// Whether an action may execute partially.
    PartialFills { None, AllowIfConfigured, AlwaysAllow }
);

str_enum!(
    /// Which candle price a market order fills at.
    PriceSelection { Close, Open, Mid, NextOpen, WorseSideOhlc }
);

str_enum!(
    /// Execution-price slippage model.
    SlippageModel { FixedBps, VolumeBased, AmmPriceImpact, None }
);

str_enum!(
    /// Trading/protocol fee model.
    FeeModel { FixedBps, VenueFeeTable, None }
);

str_enum!(
    /// Gas cost model for on-chain actions.
    GasModel { None, FixedUsd, FixedNative, HistoricalFeeHistory }
);

str_enum!(
    /// When a signal causes downstream actions to execute.
    SignalTrigger { Level, Crossing, CrossingWithCooldown, OncePerBacktest }
);

str_enum!(
    /// Whether/how actions repeat.
    Repeat { Never, OnEachSignalFire, WithCooldown, MaxCount }
);

str_enum!(
    /// Ordering when multiple signals/actions land on the same tick.
    SameTick { GraphOrder, TopologicalOrder, SignalsFirstThenActions, ConservativeAdverseOrder }
);

str_enum!(
    /// What happens when required market data is missing.
    ///
    /// `Warn` (warn-and-continue on the data-driven tick grid) and `Fail` are
    /// implemented. `SkipTick` and `ForwardFill` are rejected by [`validate`]
    /// until they do what they say (#159).
    MissingRequired { Fail, Warn, SkipTick, ForwardFill }
);

str_enum!(
    /// What happens when optional market data is missing.
    MissingOptional { Warn, Fail, ForwardFill, FallbackProvider }
);

str_enum!(
    /// How often perp liquidation is checked.
    LiquidationCheck { EveryTick, Never }
);

str_enum!(
    /// Funding accrual behavior.
    Funding { Historical, None }
);

str_enum!(
    /// Reduce-only validation strictness.
    ReduceOnlyValidation { Strict, Lenient }
);

str_enum!(
    /// How yield positions accrue. `None` disables yield accrual (symmetric
    /// with [`Funding::None`]); `ProtocolIndex` is rejected by [`validate`]
    /// until implemented (#164).
    YieldAccrual { SimpleApr, CompoundApy, ProtocolIndex, None }
);

/// A fully resolved policy: every knob populated, ready for the engine.
///
/// Decimal knobs (`slippage_bps`, `fee_bps`, `gas_fixed_amount`) are carried as
/// strings to preserve precision; [`validate`] guarantees they parse as
/// non-negative decimals.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResolvedPolicy {
    pub schema_version: String,
    pub profile: Profile,
    pub insufficient_balance: InsufficientBalance,
    pub partial_fills: PartialFills,
    pub price_selection: PriceSelection,
    pub slippage_model: SlippageModel,
    pub slippage_bps: String,
    pub fee_model: FeeModel,
    pub fee_bps: String,
    pub gas_model: GasModel,
    pub gas_fallback_model: GasModel,
    pub gas_fixed_amount: String,
    pub signal_trigger: SignalTrigger,
    pub repeat: Repeat,
    pub cooldown: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repeat_max_count: Option<u32>,
    pub same_tick: SameTick,
    pub missing_required: MissingRequired,
    pub missing_optional: MissingOptional,
    pub liquidation_check: LiquidationCheck,
    pub funding: Funding,
    pub reduce_only_validation: ReduceOnlyValidation,
    pub yield_accrual: YieldAccrual,
}
