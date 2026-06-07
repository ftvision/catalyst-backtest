//! Tests for policy profiles, resolution, overrides, and validation.

use catalyst_contracts::policy::{BalancePolicy, FillsPolicy, SignalPolicy, SlippagePolicy};
use catalyst_contracts::SimulationPolicy as ContractPolicy;
use catalyst_simulation_policies::{
    conservative_v1, research_v1, resolve, resolve_policy, strict_v1, InsufficientBalance,
    MissingRequired, PartialFills, PolicyError, PriceSelection, Profile, ResolvedPolicy,
    SignalTrigger,
};

fn contract(profile: &str) -> ContractPolicy {
    ContractPolicy {
        schema_version: "catalyst.backtest.policy.v1".to_string(),
        profile: profile.to_string(),
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

// --- Profile defaults (balance, partial fill, trigger, missing data, price) ---

#[test]
fn strict_profile_defaults() {
    let p = strict_v1();
    assert_eq!(p.insufficient_balance, InsufficientBalance::Reject);
    assert_eq!(p.partial_fills, PartialFills::None);
    assert_eq!(p.price_selection, PriceSelection::NextOpen);
    assert_eq!(p.signal_trigger, SignalTrigger::Crossing);
    assert_eq!(p.missing_required, MissingRequired::Fail);
}

#[test]
fn conservative_profile_is_more_adverse() {
    let p = conservative_v1();
    assert_eq!(p.profile, Profile::ConservativeV1);
    assert_eq!(p.price_selection, PriceSelection::WorseSideOhlc);
    assert_eq!(p.slippage_bps, "25");
    // still rejects insufficient balance, like strict
    assert_eq!(p.insufficient_balance, InsufficientBalance::Reject);
}

#[test]
fn research_profile_tolerates_fallback_data() {
    let p = research_v1();
    assert_eq!(p.profile, Profile::ResearchV1);
    assert_eq!(p.missing_required, MissingRequired::ForwardFill);
    assert_eq!(p.partial_fills, PartialFills::AllowIfConfigured);
}

// --- Resolution by name ---

#[test]
fn resolve_known_profiles() {
    assert_eq!(resolve("strict_v1").unwrap(), strict_v1());
    assert_eq!(resolve("conservative_v1").unwrap(), conservative_v1());
    assert_eq!(resolve("research_v1").unwrap(), research_v1());
}

#[test]
fn resolve_unknown_profile_errors() {
    assert!(matches!(resolve("yolo_v9"), Err(PolicyError::UnknownProfile(_))));
}

// --- Serializability + round-trip ---

#[test]
fn resolved_policy_round_trips_through_json() {
    let p = strict_v1();
    let json = serde_json::to_string(&p).unwrap();
    let back: ResolvedPolicy = serde_json::from_str(&json).unwrap();
    assert_eq!(p, back);
    // enums serialize to the schema's snake_case strings
    assert!(json.contains("\"reject\""));
    assert!(json.contains("\"next_open\""));
}

#[test]
fn to_contract_carries_profile_identity() {
    let c = strict_v1().to_contract();
    assert_eq!(c.profile, "strict_v1");
    assert_eq!(c.schema_version, "catalyst.backtest.policy.v1");
}

// --- Overrides on top of a profile ---

#[test]
fn overrides_apply_on_top_of_profile() {
    let mut c = contract("strict_v1");
    c.fills = Some(FillsPolicy {
        partial_fills: None,
        price_selection: Some("worse_side_ohlc".to_string()),
        slippage: Some(SlippagePolicy {
            model: Some("fixed_bps".to_string()),
            bps: Some("30".to_string()),
        }),
        fees: None,
    });
    let p = resolve_policy(&c).unwrap();
    assert_eq!(p.price_selection, PriceSelection::WorseSideOhlc);
    assert_eq!(p.slippage_bps, "30");
    // untouched knobs keep strict defaults
    assert_eq!(p.insufficient_balance, InsufficientBalance::Reject);
}

#[test]
fn unknown_override_value_errors() {
    let mut c = contract("strict_v1");
    c.balance = Some(BalancePolicy { insufficient_balance: Some("explode".to_string()) });
    assert!(matches!(
        resolve_policy(&c),
        Err(PolicyError::UnknownValue { .. })
    ));
}

// --- Validation of unsupported combinations ---

#[test]
fn partial_fill_balance_without_partial_fills_is_rejected() {
    let mut c = contract("strict_v1");
    c.balance = Some(BalancePolicy { insufficient_balance: Some("partial_fill".to_string()) });
    // strict has partial_fills = none -> contradiction
    assert!(matches!(resolve_policy(&c), Err(PolicyError::Invalid(_))));
}

#[test]
fn crossing_with_cooldown_requires_cooldown() {
    let mut c = contract("strict_v1");
    c.signals = Some(SignalPolicy {
        trigger: Some("crossing_with_cooldown".to_string()),
        repeat: None,
        cooldown: None,
        max_count: None,
    });
    assert!(matches!(resolve_policy(&c), Err(PolicyError::Invalid(_))));

    // ...but is accepted once a cooldown is supplied
    let mut ok = contract("strict_v1");
    ok.signals = Some(SignalPolicy {
        trigger: Some("crossing_with_cooldown".to_string()),
        repeat: None,
        cooldown: Some("1h".to_string()),
        max_count: None,
    });
    let resolved = resolve_policy(&ok).unwrap();
    assert_eq!(resolved.signal_trigger, SignalTrigger::CrossingWithCooldown);
}

#[test]
fn non_decimal_slippage_is_rejected() {
    let mut c = contract("strict_v1");
    c.fills = Some(FillsPolicy {
        partial_fills: None,
        price_selection: None,
        slippage: Some(SlippagePolicy {
            model: Some("fixed_bps".to_string()),
            bps: Some("abc".to_string()),
        }),
        fees: None,
    });
    assert!(matches!(resolve_policy(&c), Err(PolicyError::Invalid(_))));
}
