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

fn parse_decimal(field: &str, value: &str) -> Result<f64, PolicyError> {
    let v: f64 = value
        .parse()
        .map_err(|_| PolicyError::Invalid(format!("{field} is not a decimal: {value:?}")))?;
    if v < 0.0 {
        return Err(PolicyError::Invalid(format!("{field} must be non-negative: {value:?}")));
    }
    Ok(v)
}

/// Reject internally inconsistent or unsupported policy combinations.
pub fn validate(p: &ResolvedPolicy) -> Result<(), PolicyError> {
    // Decimal knobs must be valid non-negative decimals when their model is active.
    if p.slippage_model == SlippageModel::FixedBps {
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

    // Contradiction: rejecting insufficient balance while also asking for partial
    // fills on insufficient balance.
    if p.insufficient_balance == InsufficientBalance::PartialFill
        && p.partial_fills == PartialFills::None
    {
        return Err(PolicyError::Invalid(
            "insufficient_balance=partial_fill requires fills.partial_fills != none".to_string(),
        ));
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

    /// Project to a contract [`SimulationPolicy`] envelope for embedding in a
    /// trace. The full resolved knobs travel as the [`ResolvedPolicy`] itself;
    /// this carries the versioned profile identity that every result must echo.
    pub fn to_contract(&self) -> ContractPolicy {
        ContractPolicy {
            schema_version: self.schema_version.clone(),
            profile: self.profile_name().to_string(),
            balance: None,
            fills: None,
            gas: None,
            signals: None,
            ordering: None,
            data: None,
            perps: None,
            yield_: None,
        }
    }
}
