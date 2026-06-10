//! Cross-language policy-contract conformance guard (#168).
//!
//! The policy enums are hand-mirrored in three places: the Rust enums in this
//! crate, the Python literals in `catalyst_contracts/policy.py`, and the
//! shared JSON Schema `schemas/simulation-policy.schema.json`. This test pins
//! the Rust side to the schema (set equality in both directions) so that any
//! new variant added to one side without the other fails the standard test
//! run. The Python side is pinned to the same schema by
//! `packages/contracts/tests/test_policy_schema_conformance.py`.

use std::collections::BTreeSet;

use catalyst_simulation_policies::{
    FeeModel, Funding, GasModel, InsufficientBalance, LiquidationCheck, MissingOptional,
    MissingRequired, PartialFills, PriceSelection, Profile, ReduceOnlyValidation, Repeat, SameTick,
    SignalTrigger, SlippageModel, YieldAccrual,
};
use serde::Serialize;
use serde_json::Value;

const SCHEMA_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../schemas/simulation-policy.schema.json"
);

fn load_schema() -> Value {
    let text = std::fs::read_to_string(SCHEMA_PATH)
        .unwrap_or_else(|e| panic!("failed to read {SCHEMA_PATH}: {e}"));
    serde_json::from_str(&text).expect("simulation-policy.schema.json is valid JSON")
}

/// Serialize every variant to its snake_case wire string.
fn wire_names<T: Serialize>(variants: &[T]) -> BTreeSet<String> {
    variants
        .iter()
        .map(|v| {
            serde_json::to_value(v)
                .expect("variant serializes")
                .as_str()
                .expect("variant serializes to a string")
                .to_string()
        })
        .collect()
}

/// Extract the `enum` array at a JSON pointer into the schema.
fn schema_enum(schema: &Value, pointer: &str) -> BTreeSet<String> {
    let node = schema
        .pointer(pointer)
        .unwrap_or_else(|| panic!("schema has no enum at pointer {pointer}"));
    node.as_array()
        .unwrap_or_else(|| panic!("schema node at {pointer} is not an array"))
        .iter()
        .map(|v| {
            v.as_str()
                .unwrap_or_else(|| panic!("non-string enum value at {pointer}"))
                .to_string()
        })
        .collect()
}

fn assert_set_equal(field: &str, schema_values: &BTreeSet<String>, rust_values: &BTreeSet<String>) {
    let missing_in_rust: Vec<_> = schema_values.difference(rust_values).collect();
    let missing_in_schema: Vec<_> = rust_values.difference(schema_values).collect();
    assert!(
        missing_in_rust.is_empty() && missing_in_schema.is_empty(),
        "policy field `{field}` drifted between Rust and the JSON Schema:\n  \
         in schema but not in Rust enum: {missing_in_rust:?}\n  \
         in Rust enum but not in schema: {missing_in_schema:?}\n  \
         (schema: schemas/simulation-policy.schema.json; Rust: crates/simulation-policies/src/lib.rs)"
    );
}

#[test]
fn rust_enums_match_schema_enums() {
    let schema = load_schema();

    // (human-readable field name, JSON pointer into the schema, Rust variants)
    let table: Vec<(&str, &str, BTreeSet<String>)> = vec![
        (
            "profile",
            "/properties/profile/enum",
            wire_names(Profile::VARIANTS),
        ),
        (
            "balance.insufficient_balance",
            "/properties/balance/properties/insufficient_balance/enum",
            wire_names(InsufficientBalance::VARIANTS),
        ),
        (
            "fills.partial_fills",
            "/properties/fills/properties/partial_fills/enum",
            wire_names(PartialFills::VARIANTS),
        ),
        (
            "fills.price_selection",
            "/properties/fills/properties/price_selection/enum",
            wire_names(PriceSelection::VARIANTS),
        ),
        (
            "fills.slippage.model",
            "/properties/fills/properties/slippage/properties/model/enum",
            wire_names(SlippageModel::VARIANTS),
        ),
        (
            "fills.fees.model",
            "/properties/fills/properties/fees/properties/model/enum",
            wire_names(FeeModel::VARIANTS),
        ),
        (
            "gas.model",
            "/properties/gas/properties/model/enum",
            wire_names(GasModel::VARIANTS),
        ),
        // gas.fallback.model is intentionally a strict subset of GasModel:
        // the fallback is what `historical_fee_history` falls back TO when no
        // historical data is available, so it cannot itself be
        // `historical_fee_history`. Encode the real expected set rather than
        // GasModel::VARIANTS.
        (
            "gas.fallback.model",
            "/properties/gas/properties/fallback/properties/model/enum",
            ["none", "fixed_usd", "fixed_native"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
        ),
        (
            "signals.trigger",
            "/properties/signals/properties/trigger/enum",
            wire_names(SignalTrigger::VARIANTS),
        ),
        (
            "signals.repeat",
            "/properties/signals/properties/repeat/enum",
            wire_names(Repeat::VARIANTS),
        ),
        (
            "ordering.same_tick",
            "/properties/ordering/properties/same_tick/enum",
            wire_names(SameTick::VARIANTS),
        ),
        (
            "data.missing_required",
            "/properties/data/properties/missing_required/enum",
            wire_names(MissingRequired::VARIANTS),
        ),
        (
            "data.missing_optional",
            "/properties/data/properties/missing_optional/enum",
            wire_names(MissingOptional::VARIANTS),
        ),
        (
            "perps.liquidation_check",
            "/properties/perps/properties/liquidation_check/enum",
            wire_names(LiquidationCheck::VARIANTS),
        ),
        (
            "perps.funding",
            "/properties/perps/properties/funding/enum",
            wire_names(Funding::VARIANTS),
        ),
        (
            "perps.reduce_only_validation",
            "/properties/perps/properties/reduce_only_validation/enum",
            wire_names(ReduceOnlyValidation::VARIANTS),
        ),
        (
            "yield.accrual",
            "/properties/yield/properties/accrual/enum",
            wire_names(YieldAccrual::VARIANTS),
        ),
    ];

    for (field, pointer, rust_values) in &table {
        let schema_values = schema_enum(&schema, pointer);
        assert_set_equal(field, &schema_values, rust_values);
    }
}

/// The gas fallback subset assumption above: the fallback enum must be exactly
/// GasModel minus `historical_fee_history`, so a new GasModel variant forces a
/// deliberate decision about the fallback enum too.
#[test]
fn gas_fallback_is_gas_model_minus_historical() {
    let schema = load_schema();
    let fallback = schema_enum(
        &schema,
        "/properties/gas/properties/fallback/properties/model/enum",
    );
    let mut expected = wire_names(GasModel::VARIANTS);
    expected.remove("historical_fee_history");
    assert_set_equal("gas.fallback.model (derived)", &fallback, &expected);
}
