//! Resolve a profile (optionally overridden by a partial contract policy) into a
//! validated [`ResolvedPolicy`], and validate unsupported combinations.

use std::fmt;

use catalyst_contracts::request::ExecutionOverrides;
use catalyst_contracts::SimulationPolicy as ContractPolicy;
use serde::de::DeserializeOwned;

use crate::*;

/// A policy could not be resolved or is internally inconsistent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyError {
    UnknownProfile(String),
    UnknownValue { field: String, value: String },
    Invalid(String),
}

impl fmt::Display for PolicyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PolicyError::UnknownProfile(p) => write!(f, "unknown policy profile {p:?}"),
            PolicyError::UnknownValue { field, value } => {
                write!(f, "unsupported value {value:?} for policy field {field:?}")
            }
            PolicyError::Invalid(m) => write!(f, "invalid policy: {m}"),
        }
    }
}

impl std::error::Error for PolicyError {}

fn parse_enum<T: DeserializeOwned>(field: &str, value: &str) -> Result<T, PolicyError> {
    serde_json::from_value(serde_json::Value::String(value.to_string())).map_err(|_| {
        PolicyError::UnknownValue { field: field.to_string(), value: value.to_string() }
    })
}

/// Resolve a bare profile name into its default [`ResolvedPolicy`].
pub fn resolve(profile: &str) -> Result<ResolvedPolicy, PolicyError> {
    match profile {
        "strict_v1" => Ok(strict_v1()),
        "conservative_v1" => Ok(conservative_v1()),
        "research_v1" => Ok(research_v1()),
        other => Err(PolicyError::UnknownProfile(other.to_string())),
    }
}

/// Resolve a (possibly partial) contract policy: start from its profile's
/// defaults, apply any explicitly set knobs, then validate.
pub fn resolve_policy(contract: &ContractPolicy) -> Result<ResolvedPolicy, PolicyError> {
    let mut p = resolve(&contract.profile)?;

    if let Some(balance) = &contract.balance {
        if let Some(v) = &balance.insufficient_balance {
            p.insufficient_balance = parse_enum("balance.insufficient_balance", v)?;
        }
    }
    if let Some(fills) = &contract.fills {
        if let Some(v) = &fills.partial_fills {
            p.partial_fills = parse_enum("fills.partial_fills", v)?;
        }
        if let Some(v) = &fills.price_selection {
            p.price_selection = parse_enum("fills.price_selection", v)?;
        }
        if let Some(slip) = &fills.slippage {
            if let Some(v) = &slip.model {
                p.slippage_model = parse_enum("fills.slippage.model", v)?;
            }
            if let Some(v) = &slip.bps {
                p.slippage_bps = v.clone();
            }
        }
        if let Some(fees) = &fills.fees {
            if let Some(v) = &fees.model {
                p.fee_model = parse_enum("fills.fees.model", v)?;
            }
            if let Some(v) = &fees.bps {
                p.fee_bps = v.clone();
            }
        }
    }
    if let Some(gas) = &contract.gas {
        if let Some(v) = &gas.model {
            p.gas_model = parse_enum("gas.model", v)?;
        }
        if let Some(fb) = &gas.fallback {
            if let Some(v) = &fb.model {
                p.gas_fallback_model = parse_enum("gas.fallback.model", v)?;
            }
            if let Some(v) = &fb.amount {
                p.gas_fixed_amount = v.clone();
            }
        }
    }
    if let Some(signals) = &contract.signals {
        if let Some(v) = &signals.trigger {
            p.signal_trigger = parse_enum("signals.trigger", v)?;
        }
        if let Some(v) = &signals.repeat {
            p.repeat = parse_enum("signals.repeat", v)?;
        }
        if signals.cooldown.is_some() {
            p.cooldown = signals.cooldown.clone();
        }
        if signals.max_count.is_some() {
            p.repeat_max_count = signals.max_count;
        }
    }
    if let Some(ordering) = &contract.ordering {
        if let Some(v) = &ordering.same_tick {
            p.same_tick = parse_enum("ordering.same_tick", v)?;
        }
    }
    if let Some(data) = &contract.data {
        if let Some(v) = &data.missing_required {
            p.missing_required = parse_enum("data.missing_required", v)?;
        }
        if let Some(v) = &data.missing_optional {
            p.missing_optional = parse_enum("data.missing_optional", v)?;
        }
    }
    if let Some(perps) = &contract.perps {
        if let Some(v) = &perps.liquidation_check {
            p.liquidation_check = parse_enum("perps.liquidation_check", v)?;
        }
        if let Some(v) = &perps.funding {
            p.funding = parse_enum("perps.funding", v)?;
        }
        if let Some(v) = &perps.reduce_only_validation {
            p.reduce_only_validation = parse_enum("perps.reduce_only_validation", v)?;
        }
    }
    if let Some(y) = &contract.yield_ {
        if let Some(v) = &y.accrual {
            p.yield_accrual = parse_enum("yield.accrual", v)?;
        }
    }

    validate(&p)?;
    Ok(p)
}

/// Parse a duration like `30s`, `15m`, `1h`, `2d` into seconds. This is the
/// single authoritative duration grammar for policy knobs (`signals.cooldown`);
/// [`validate`] guarantees any cooldown carried by a [`ResolvedPolicy`] parses,
/// so consumers (the engine's cooldown gate) can rely on `Some`.
pub fn parse_duration_secs(s: &str) -> Option<i64> {
    let s = s.trim();
    let (num, unit) = s.split_at(s.len().checked_sub(1)?);
    let n: i64 = num.parse().ok()?;
    let mult = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 3600,
        "d" => 86_400,
        _ => return None,
    };
    Some(n * mult)
}

fn parse_decimal(field: &str, value: &str) -> Result<f64, PolicyError> {
    let v: f64 = value
        .parse()
        .map_err(|_| PolicyError::Invalid(format!("{field} is not a decimal: {value:?}")))?;
    if v < 0.0 {
        return Err(PolicyError::Invalid(format!("{field} must be non-negative: {value:?}")));
    }
    Ok(v)
}

/// A policy value that parses but has no engine implementation. Accepting it
/// would silently run different assumptions than the user selected, so it is
/// an error pointing at the tracking issue (implement-or-reject; epic #131).
fn unimplemented_value(field: &str, value: &str, issue: &str) -> PolicyError {
    PolicyError::Invalid(format!(
        "{field} = {value:?} is not implemented yet ({issue}); selecting it would \
         silently run different behavior than requested"
    ))
}

/// Reject internally inconsistent or unsupported policy combinations, and any
/// accepted-but-unimplemented variant (the engine must never silently ignore a
/// policy the user selected).
pub fn validate(p: &ResolvedPolicy) -> Result<(), PolicyError> {
    // Decimal knobs must be valid non-negative decimals when any model that
    // consumes them is active. `slippage_bps` feeds every slippage model
    // (fixed_bps directly; volume_based as the base term; amm_price_impact as
    // the no-reserves fallback), so it is validated unless slippage is off
    // (#163 — a malformed bps must never silently mean zero slippage).
    if p.slippage_model != SlippageModel::None {
        parse_decimal("slippage_bps", &p.slippage_bps)?;
    }
    if p.fee_model == FeeModel::FixedBps {
        parse_decimal("fee_bps", &p.fee_bps)?;
    }
    if matches!(p.gas_model, GasModel::FixedUsd | GasModel::FixedNative)
        || matches!(p.gas_fallback_model, GasModel::FixedUsd | GasModel::FixedNative)
    {
        parse_decimal("gas_fixed_amount", &p.gas_fixed_amount)?;
    }

    // Implement-or-reject: every variant below parses (it is part of the
    // contract surface) but has no engine behavior. Selecting one is an error,
    // not a silent no-op.
    match p.insufficient_balance {
        InsufficientBalance::PartialFill => {
            return Err(unimplemented_value("balance.insufficient_balance", "partial_fill", "#144"));
        }
        InsufficientBalance::ClampToAvailable => {
            return Err(unimplemented_value(
                "balance.insufficient_balance",
                "clamp_to_available",
                "#144",
            ));
        }
        InsufficientBalance::Reject | InsufficientBalance::AllowNegative => {}
    }
    if p.partial_fills != PartialFills::None {
        return Err(unimplemented_value("fills.partial_fills", "allow_*", "#144"));
    }
    if p.fee_model == FeeModel::VenueFeeTable {
        return Err(unimplemented_value("fills.fees.model", "venue_fee_table", "#143"));
    }
    if p.gas_model == GasModel::FixedNative {
        return Err(unimplemented_value("gas.model", "fixed_native", "#146"));
    }
    if p.gas_fallback_model != GasModel::FixedUsd {
        return Err(unimplemented_value("gas.fallback.model", "non-fixed_usd", "#145"));
    }
    if p.same_tick != SameTick::TopologicalOrder {
        return Err(unimplemented_value("ordering.same_tick", "non-topological_order", "#141"));
    }
    if matches!(p.missing_required, MissingRequired::SkipTick | MissingRequired::ForwardFill) {
        return Err(unimplemented_value(
            "data.missing_required",
            "skip_tick / forward_fill",
            "#159; use \"warn\" (warn-and-continue) or \"fail\"",
        ));
    }
    if p.missing_optional != MissingOptional::Warn {
        return Err(unimplemented_value("data.missing_optional", "non-warn", "#142"));
    }
    if p.reduce_only_validation == ReduceOnlyValidation::Lenient {
        return Err(unimplemented_value("perps.reduce_only_validation", "lenient", "#158"));
    }
    if p.yield_accrual == YieldAccrual::ProtocolIndex {
        return Err(unimplemented_value("yield.accrual", "protocol_index", "#164"));
    }

    // Any cooldown present must parse — even when no cooldown-consuming trigger
    // or repeat is active — because the value is echoed in the executed policy
    // (`to_contract`) and must be honest. A malformed duration must never
    // silently mean "no cooldown" (#160).
    if let Some(cd) = &p.cooldown {
        if parse_duration_secs(cd).is_none() {
            return Err(PolicyError::Invalid(format!(
                "signals.cooldown is not a valid duration: {cd:?} (expected <integer><s|m|h|d>, e.g. \"30m\")"
            )));
        }
    }

    // A cooldown trigger needs a cooldown duration.
    if p.signal_trigger == SignalTrigger::CrossingWithCooldown && p.cooldown.is_none() {
        return Err(PolicyError::Invalid(
            "signal_trigger=crossing_with_cooldown requires signals.cooldown".to_string(),
        ));
    }
    if p.repeat == Repeat::WithCooldown && p.cooldown.is_none() {
        return Err(PolicyError::Invalid(
            "repeat=with_cooldown requires signals.cooldown".to_string(),
        ));
    }
    if p.repeat == Repeat::MaxCount && p.repeat_max_count.is_none() {
        return Err(PolicyError::Invalid(
            "repeat=max_count requires signals.max_count".to_string(),
        ));
    }

    Ok(())
}

impl ResolvedPolicy {
    /// Apply per-run execution overrides (`BacktestConfig.execution`) onto a
    /// resolved policy. Each field, when present, wins over the profile's value.
    /// Lets a single run tune firing/cost knobs without defining a new profile.
    pub fn apply_execution_overrides(
        &mut self,
        overrides: &ExecutionOverrides,
    ) -> Result<(), PolicyError> {
        if let Some(v) = &overrides.signal_trigger {
            self.signal_trigger = parse_enum("execution.signal_trigger", v)?;
        }
        if let Some(v) = &overrides.gas_model {
            self.gas_model = parse_enum("execution.gas_model", v)?;
        }
        if let Some(v) = &overrides.slippage_bps {
            self.slippage_bps = v.clone();
        }
        if let Some(v) = &overrides.action_cooldown {
            self.cooldown = Some(v.clone());
        }
        // Overrides must obey the same rules as profile knobs — a malformed
        // slippage_bps or an unimplemented gas model is rejected here too
        // (#163: an override path must never silently mean zero slippage).
        validate(self)?;
        Ok(())
    }

    /// The profile's canonical string name.
    pub fn profile_name(&self) -> &'static str {
        match self.profile {
            Profile::StrictV1 => "strict_v1",
            Profile::ConservativeV1 => "conservative_v1",
            Profile::ResearchV1 => "research_v1",
        }
    }

    /// Serialize an enum knob to its snake_case contract string.
    fn knob<T: serde::Serialize>(v: &T) -> Option<String> {
        match serde_json::to_value(v) {
            Ok(serde_json::Value::String(s)) => Some(s),
            _ => None,
        }
    }

    /// Project to a contract [`SimulationPolicy`] that echoes **every executed
    /// knob**, not just the profile name. The engine embeds this in the trace
    /// *after* per-run execution overrides are applied, so the result metadata
    /// always reports the policy that actually ran (#157) — re-resolving this
    /// contract reproduces the executed [`ResolvedPolicy`] exactly.
    pub fn to_contract(&self) -> ContractPolicy {
        ContractPolicy {
            schema_version: self.schema_version.clone(),
            profile: self.profile_name().to_string(),
            balance: Some(catalyst_contracts::policy::BalancePolicy {
                insufficient_balance: Self::knob(&self.insufficient_balance),
            }),
            fills: Some(catalyst_contracts::policy::FillsPolicy {
                partial_fills: Self::knob(&self.partial_fills),
                price_selection: Self::knob(&self.price_selection),
                slippage: Some(catalyst_contracts::policy::SlippagePolicy {
                    model: Self::knob(&self.slippage_model),
                    bps: Some(self.slippage_bps.clone()),
                }),
                fees: Some(catalyst_contracts::policy::FeePolicy {
                    model: Self::knob(&self.fee_model),
                    bps: Some(self.fee_bps.clone()),
                }),
            }),
            gas: Some(catalyst_contracts::policy::GasPolicy {
                model: Self::knob(&self.gas_model),
                fallback: Some(catalyst_contracts::policy::GasFallback {
                    model: Self::knob(&self.gas_fallback_model),
                    amount: Some(self.gas_fixed_amount.clone()),
                }),
            }),
            signals: Some(catalyst_contracts::policy::SignalPolicy {
                trigger: Self::knob(&self.signal_trigger),
                repeat: Self::knob(&self.repeat),
                cooldown: self.cooldown.clone(),
                max_count: self.repeat_max_count,
            }),
            ordering: Some(catalyst_contracts::policy::OrderingPolicy {
                same_tick: Self::knob(&self.same_tick),
            }),
            data: Some(catalyst_contracts::policy::DataPolicy {
                missing_required: Self::knob(&self.missing_required),
                missing_optional: Self::knob(&self.missing_optional),
            }),
            perps: Some(catalyst_contracts::policy::PerpPolicy {
                liquidation_check: Self::knob(&self.liquidation_check),
                funding: Self::knob(&self.funding),
                reduce_only_validation: Self::knob(&self.reduce_only_validation),
            }),
            yield_: Some(catalyst_contracts::policy::YieldPolicy {
                accrual: Self::knob(&self.yield_accrual),
            }),
        }
    }
}
