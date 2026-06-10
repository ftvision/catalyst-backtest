//! Tests for policy profiles, resolution, overrides, and validation.

use catalyst_contracts::policy::{BalancePolicy, FillsPolicy, SignalPolicy, SlippagePolicy};
use catalyst_contracts::SimulationPolicy as ContractPolicy;
use catalyst_simulation_policies::{
    conservative_v1, parse_duration_secs, research_v1, resolve, resolve_policy, strict_v1,
    InsufficientBalance, MissingRequired, PartialFills, PolicyError, PriceSelection, Profile,
    ResolvedPolicy, SignalTrigger,
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
fn research_profile_warns_on_missing_data() {
    // research_v1 declares only behavior the engine implements (#159/#144):
    // warn-and-continue on missing required data, and no partial fills.
    let p = research_v1();
    assert_eq!(p.profile, Profile::ResearchV1);
    assert_eq!(p.missing_required, MissingRequired::Warn);
    assert_eq!(p.partial_fills, PartialFills::None);
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

// --- #160: malformed cooldown strings are rejected, never silent no-cooldown ---

#[test]
fn malformed_cooldown_is_rejected_at_resolve() {
    let mut c = contract("strict_v1");
    c.signals = Some(SignalPolicy {
        trigger: Some("crossing_with_cooldown".to_string()),
        repeat: None,
        cooldown: Some("15x".to_string()),
        max_count: None,
    });
    let err = resolve_policy(&c).unwrap_err();
    assert_eq!(
        err.to_string(),
        "invalid policy: signals.cooldown is not a valid duration: \"15x\" \
         (expected <integer><s|m|h|d>, e.g. \"30m\")"
    );
}

#[test]
fn malformed_cooldown_is_rejected_even_without_a_cooldown_consumer() {
    // The cooldown is validated unconditionally: it is echoed in the executed
    // policy (`to_contract`) and must be honest even when the trigger/repeat
    // don't consume it.
    let mut c = contract("strict_v1");
    c.signals = Some(SignalPolicy {
        trigger: None, // strict default: crossing — no cooldown gate
        repeat: None,
        cooldown: Some("soon".to_string()),
        max_count: None,
    });
    assert!(matches!(resolve_policy(&c), Err(PolicyError::Invalid(_))));
}

#[test]
fn malformed_cooldown_is_rejected_via_execution_overrides() {
    use catalyst_contracts::request::ExecutionOverrides;
    let mut p = strict_v1();
    let err = p
        .apply_execution_overrides(&ExecutionOverrides {
            signal_trigger: None,
            slippage_bps: None,
            gas_model: None,
            action_cooldown: Some("15x".into()),
        })
        .unwrap_err();
    assert!(err.to_string().contains("signals.cooldown is not a valid duration"), "{err}");
}

#[test]
fn parse_duration_secs_grammar() {
    use catalyst_simulation_policies::parse_duration_secs;
    assert_eq!(parse_duration_secs("30s"), Some(30));
    assert_eq!(parse_duration_secs("15m"), Some(900));
    assert_eq!(parse_duration_secs("1h"), Some(3600));
    assert_eq!(parse_duration_secs("2d"), Some(172_800));
    assert_eq!(parse_duration_secs("15x"), None);
    assert_eq!(parse_duration_secs("h"), None);
    assert_eq!(parse_duration_secs(""), None);
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

#[test]
fn execution_overrides_win_over_the_profile() {
    use catalyst_contracts::request::ExecutionOverrides;

    let mut p = strict_v1();
    assert_eq!(p.signal_trigger, SignalTrigger::Crossing);
    assert!(p.cooldown.is_none());

    p.apply_execution_overrides(&ExecutionOverrides {
        signal_trigger: Some("once_per_backtest".into()),
        slippage_bps: Some("50".into()),
        gas_model: Some("none".into()),
        action_cooldown: Some("6h".into()),
    })
    .unwrap();

    assert_eq!(p.signal_trigger, SignalTrigger::OncePerBacktest);
    assert_eq!(p.slippage_bps, "50");
    assert_eq!(p.cooldown.as_deref(), Some("6h"));
}

#[test]
fn unset_execution_overrides_leave_the_profile_unchanged() {
    use catalyst_contracts::request::ExecutionOverrides;
    let mut p = strict_v1();
    let before = p.clone();
    p.apply_execution_overrides(&ExecutionOverrides::default()).unwrap();
    assert_eq!(p, before);
}

#[test]
fn bad_execution_override_value_is_rejected() {
    use catalyst_contracts::request::ExecutionOverrides;
    let mut p = strict_v1();
    let err = p.apply_execution_overrides(&ExecutionOverrides {
        signal_trigger: Some("nonsense".into()),
        ..Default::default()
    });
    assert!(matches!(err, Err(PolicyError::UnknownValue { .. })));
}

// --- Implement-or-reject: unimplemented variants fail validation loudly ---

/// Every policy value the engine does not implement must be rejected at
/// resolution, never silently accepted (#141-#146, #158, #159, #164; epic #131).
#[test]
fn unimplemented_policy_values_are_rejected_not_ignored() {
    let cases: &[(&str, serde_json::Value)] = &[
        ("#144 partial_fill", serde_json::json!({"balance": {"insufficient_balance": "partial_fill"}})),
        ("#144 clamp_to_available", serde_json::json!({"balance": {"insufficient_balance": "clamp_to_available"}})),
        ("#144 partial_fills", serde_json::json!({"fills": {"partial_fills": "always_allow"}})),
        ("#143 venue_fee_table", serde_json::json!({"fills": {"fees": {"model": "venue_fee_table"}}})),
        ("#146 fixed_native", serde_json::json!({"gas": {"model": "fixed_native"}})),
        ("#145 gas fallback", serde_json::json!({"gas": {"fallback": {"model": "none"}}})),
        ("#141 same_tick", serde_json::json!({"ordering": {"same_tick": "conservative_adverse_order"}})),
        ("#159 skip_tick", serde_json::json!({"data": {"missing_required": "skip_tick"}})),
        ("#159 forward_fill", serde_json::json!({"data": {"missing_required": "forward_fill"}})),
        ("#142 missing_optional", serde_json::json!({"data": {"missing_optional": "fallback_provider"}})),
        ("#158 lenient", serde_json::json!({"perps": {"reduce_only_validation": "lenient"}})),
        ("#164 protocol_index", serde_json::json!({"yield": {"accrual": "protocol_index"}})),
    ];
    for (label, section) in cases {
        let mut v = serde_json::json!({
            "schema_version": "catalyst.backtest.policy.v1",
            "profile": "strict_v1"
        });
        v.as_object_mut().unwrap().extend(section.as_object().unwrap().clone());
        let c: ContractPolicy = serde_json::from_value(v).unwrap();
        let err = resolve_policy(&c);
        assert!(
            matches!(err, Err(PolicyError::Invalid(_))),
            "{label}: expected loud rejection, got {err:?}"
        );
        let msg = format!("{}", err.unwrap_err());
        assert!(
            msg.contains("not implemented"),
            "{label}: rejection should say the value is unimplemented; got {msg}"
        );
    }
}

/// Implemented variants of the same knobs still resolve.
#[test]
fn implemented_policy_values_still_resolve() {
    for section in [
        serde_json::json!({"balance": {"insufficient_balance": "allow_negative"}}),
        serde_json::json!({"data": {"missing_required": "warn"}}),
        serde_json::json!({"yield": {"accrual": "simple_apr"}}),
        serde_json::json!({"yield": {"accrual": "none"}}),
        serde_json::json!({"perps": {"funding": "none"}}),
    ] {
        let mut v = serde_json::json!({
            "schema_version": "catalyst.backtest.policy.v1",
            "profile": "strict_v1"
        });
        v.as_object_mut().unwrap().extend(section.as_object().unwrap().clone());
        let c: ContractPolicy = serde_json::from_value(v).unwrap();
        resolve_policy(&c).unwrap_or_else(|e| panic!("{section}: should resolve, got {e}"));
    }
}

// --- #163: slippage_bps is validated under every consuming model ---

#[test]
fn malformed_slippage_bps_rejected_under_all_consuming_models() {
    for model in ["fixed_bps", "volume_based", "amm_price_impact"] {
        let mut c = contract("strict_v1");
        c.fills = Some(FillsPolicy {
            slippage: Some(SlippagePolicy {
                model: Some(model.into()),
                bps: Some("ten bps".into()),
            }),
            ..Default::default()
        });
        let err = resolve_policy(&c);
        assert!(
            matches!(err, Err(PolicyError::Invalid(_))),
            "{model}: malformed slippage_bps must be rejected, not silently zero (#163); got {err:?}"
        );
    }
}

#[test]
fn malformed_slippage_bps_override_is_rejected() {
    use catalyst_contracts::request::ExecutionOverrides;
    let mut p = strict_v1();
    let err = p.apply_execution_overrides(&ExecutionOverrides {
        slippage_bps: Some("not a number".into()),
        ..Default::default()
    });
    assert!(
        matches!(err, Err(PolicyError::Invalid(_))),
        "#163: a malformed override must be rejected, not silently zero; got {err:?}"
    );
}

// --- #157: to_contract echoes every executed knob and round-trips ---

#[test]
fn to_contract_echoes_full_policy_and_round_trips() {
    let mut p = strict_v1();
    p.slippage_bps = "42".to_string(); // simulate a per-run override
    p.max_mark_staleness = Some("24h".to_string()); // #119(b) knob, off by default
    let c = p.to_contract();

    // Every section is populated — no silent omissions.
    let fills = c.fills.as_ref().expect("fills echoed");
    assert_eq!(fills.slippage.as_ref().unwrap().bps.as_deref(), Some("42"));
    assert_eq!(fills.price_selection.as_deref(), Some("next_open"));
    assert_eq!(c.data.as_ref().unwrap().missing_required.as_deref(), Some("fail"));
    assert_eq!(c.data.as_ref().unwrap().max_mark_staleness.as_deref(), Some("24h"));
    assert_eq!(c.yield_.as_ref().unwrap().accrual.as_deref(), Some("compound_apy"));
    assert_eq!(c.ordering.as_ref().unwrap().same_tick.as_deref(), Some("topological_order"));

    // Re-resolving the echoed contract reproduces the executed policy exactly.
    let round_tripped = resolve_policy(&c).expect("echoed policy re-resolves");
    assert_eq!(round_tripped, p, "#157: trace policy must reproduce the executed policy");
}

// --- #119(b): data.max_mark_staleness — default None, parsed, validated ---

#[test]
fn max_mark_staleness_defaults_to_unbounded_in_every_profile() {
    // Bounding the mark carry-forward changes valuations, so no profile turns
    // it on silently — it is opt-in per run.
    assert_eq!(strict_v1().max_mark_staleness, None);
    assert_eq!(conservative_v1().max_mark_staleness, None);
    assert_eq!(research_v1().max_mark_staleness, None);
}

#[test]
fn max_mark_staleness_resolves_from_contract() {
    let mut c = contract("strict_v1");
    c.data = Some(catalyst_contracts::policy::DataPolicy {
        missing_required: None,
        missing_optional: None,
        max_mark_staleness: Some("24h".to_string()),
    });
    let p = resolve_policy(&c).unwrap();
    assert_eq!(p.max_mark_staleness.as_deref(), Some("24h"));
    // The duration grammar is the shared one (#176): "24h" = 86400s.
    assert_eq!(parse_duration_secs("24h"), Some(86_400));
}

#[test]
fn malformed_max_mark_staleness_is_rejected_at_resolve() {
    // A malformed bound must never silently mean "unbounded" (#160 discipline).
    let mut c = contract("strict_v1");
    c.data = Some(catalyst_contracts::policy::DataPolicy {
        missing_required: None,
        missing_optional: None,
        max_mark_staleness: Some("fortnight".to_string()),
    });
    let err = resolve_policy(&c).unwrap_err();
    assert_eq!(
        err.to_string(),
        "invalid policy: data.max_mark_staleness is not a valid duration: \"fortnight\" \
         (expected <integer><s|m|h|d>, e.g. \"24h\")"
    );
}
