//! The three named policy profiles, fully resolved.

use crate::*;

const SCHEMA_VERSION: &str = "catalyst.backtest.policy.v1";

/// Deterministic correctness; good for early testing. Fills at the next bar's
/// open (`next_open`) so the default profile has no intra-bar look-ahead.
pub fn strict_v1() -> ResolvedPolicy {
    ResolvedPolicy {
        schema_version: SCHEMA_VERSION.to_string(),
        profile: Profile::StrictV1,
        insufficient_balance: InsufficientBalance::Reject,
        partial_fills: PartialFills::None,
        price_selection: PriceSelection::NextOpen,
        slippage_model: SlippageModel::FixedBps,
        slippage_bps: "10".to_string(),
        fee_model: FeeModel::FixedBps,
        fee_bps: "5".to_string(),
        gas_model: GasModel::HistoricalFeeHistory,
        gas_fallback_model: GasModel::FixedUsd,
        gas_fixed_amount: "0.25".to_string(),
        signal_trigger: SignalTrigger::Crossing,
        repeat: Repeat::OnEachSignalFire,
        cooldown: None,
        repeat_max_count: None,
        same_tick: SameTick::TopologicalOrder,
        missing_required: MissingRequired::Fail,
        missing_optional: MissingOptional::Warn,
        liquidation_check: LiquidationCheck::EveryTick,
        funding: Funding::Historical,
        reduce_only_validation: ReduceOnlyValidation::Strict,
        yield_accrual: YieldAccrual::CompoundApy,
    }
}

/// Less optimistic, user-facing backtests: worse-side fills, higher slippage
/// and fees. (It previously also *declared* adverse same-tick ordering and a
/// fallback provider for optional data — neither was implemented, so the
/// profile no longer claims them; see #141/#142.)
pub fn conservative_v1() -> ResolvedPolicy {
    ResolvedPolicy {
        profile: Profile::ConservativeV1,
        price_selection: PriceSelection::WorseSideOhlc,
        slippage_bps: "25".to_string(),
        fee_bps: "8".to_string(),
        ..strict_v1()
    }
}

/// Quick exploratory analysis: same-bar close fills (look-ahead caveat, #122),
/// lower slippage, warn-and-continue on missing required data. (It previously
/// declared partial fills and forward-fill — neither was implemented; see
/// #144/#159.)
pub fn research_v1() -> ResolvedPolicy {
    ResolvedPolicy {
        profile: Profile::ResearchV1,
        price_selection: PriceSelection::Close,
        slippage_bps: "5".to_string(),
        missing_required: MissingRequired::Warn,
        ..strict_v1()
    }
}
