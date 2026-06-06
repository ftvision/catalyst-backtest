//! The three named policy profiles, fully resolved.

use crate::*;

const SCHEMA_VERSION: &str = "catalyst.backtest.policy.v1";

/// Deterministic correctness; good for early testing.
pub fn strict_v1() -> ResolvedPolicy {
    ResolvedPolicy {
        schema_version: SCHEMA_VERSION.to_string(),
        profile: Profile::StrictV1,
        insufficient_balance: InsufficientBalance::Reject,
        partial_fills: PartialFills::None,
        price_selection: PriceSelection::Close,
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
        same_tick: SameTick::TopologicalOrder,
        missing_required: MissingRequired::Fail,
        missing_optional: MissingOptional::Warn,
        liquidation_check: LiquidationCheck::EveryTick,
        funding: Funding::Historical,
        reduce_only_validation: ReduceOnlyValidation::Strict,
        yield_accrual: YieldAccrual::SimpleApr,
    }
}

/// Less optimistic, user-facing backtests: worse-side fills, higher slippage.
pub fn conservative_v1() -> ResolvedPolicy {
    ResolvedPolicy {
        profile: Profile::ConservativeV1,
        price_selection: PriceSelection::WorseSideOhlc,
        slippage_bps: "25".to_string(),
        fee_bps: "8".to_string(),
        same_tick: SameTick::ConservativeAdverseOrder,
        missing_optional: MissingOptional::FallbackProvider,
        ..strict_v1()
    }
}

/// Quick exploratory analysis: close fills, tolerant of fallback data.
pub fn research_v1() -> ResolvedPolicy {
    ResolvedPolicy {
        profile: Profile::ResearchV1,
        partial_fills: PartialFills::AllowIfConfigured,
        price_selection: PriceSelection::Close,
        slippage_bps: "5".to_string(),
        missing_required: MissingRequired::ForwardFill,
        missing_optional: MissingOptional::FallbackProvider,
        ..strict_v1()
    }
}
