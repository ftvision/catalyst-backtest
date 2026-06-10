"""Cross-language policy-contract conformance guard (#168).

The policy enums are hand-mirrored across the Rust enums
(``crates/simulation-policies/src/lib.rs``), the Python literals in
``catalyst_contracts/policy.py``, and the shared JSON Schema
``schemas/simulation-policy.schema.json``. These tests pin the Python side to
the schema (the Rust side is pinned by
``crates/simulation-policies/tests/schema_conformance.rs``):

* every ``Literal`` field's allowed values equal the schema enum, both ways;
* every policy section model's field names (serialization alias where present)
  equal the schema object's ``properties`` keys, both ways.

Any new variant or field added on one side without the other fails here.
"""

from __future__ import annotations

import typing
from typing import Any

import pytest

from catalyst_contracts import policy
from catalyst_contracts.schemas import load_schema

# Maps each policy section model to the JSON pointer of its object node in
# the schema. The root model maps to the document root.
MODEL_POINTERS: dict[type, str] = {
    policy.SimulationPolicy: "",
    policy.BalancePolicy: "/properties/balance",
    policy.FillsPolicy: "/properties/fills",
    policy.SlippagePolicy: "/properties/fills/properties/slippage",
    policy.FeePolicy: "/properties/fills/properties/fees",
    policy.GasPolicy: "/properties/gas",
    policy.GasFallback: "/properties/gas/properties/fallback",
    policy.SignalPolicy: "/properties/signals",
    policy.OrderingPolicy: "/properties/ordering",
    policy.DataPolicy: "/properties/data",
    policy.PerpPolicy: "/properties/perps",
    policy.YieldPolicy: "/properties/yield",
}


def _resolve_pointer(doc: dict[str, Any], pointer: str) -> dict[str, Any]:
    node: Any = doc
    for part in pointer.split("/"):
        if part == "":
            continue
        node = node[part]
    return node


@pytest.fixture(scope="module")
def schema() -> dict[str, Any]:
    return load_schema("simulation-policy")


def _wire_name(field_name: str, field_info: Any) -> str:
    return field_info.serialization_alias or field_info.alias or field_name


def _model_id(model: type) -> str:
    return model.__name__


@pytest.mark.parametrize("model", MODEL_POINTERS, ids=_model_id)
def test_literal_fields_match_schema_enums(model: type, schema: dict[str, Any]) -> None:
    node = _resolve_pointer(schema, MODEL_POINTERS[model])
    properties = node["properties"]

    checked = 0
    for field_name, field_info in model.model_fields.items():
        annotation = field_info.annotation
        if typing.get_origin(annotation) is not typing.Literal:
            continue
        wire = _wire_name(field_name, field_info)
        assert wire in properties, (
            f"{model.__name__}.{field_name}: schema object at "
            f"{MODEL_POINTERS[model] or '<root>'} has no property {wire!r}"
        )
        schema_enum = set(properties[wire]["enum"])
        literal_values = set(typing.get_args(annotation))
        missing_in_python = sorted(schema_enum - literal_values)
        missing_in_schema = sorted(literal_values - schema_enum)
        assert literal_values == schema_enum, (
            f"{model.__name__}.{field_name} drifted from the schema enum:\n"
            f"  in schema but not in the Python Literal: {missing_in_python}\n"
            f"  in the Python Literal but not in schema: {missing_in_schema}"
        )
        checked += 1

    if model is not policy.SimulationPolicy:
        assert checked > 0 or not any(
            "enum" in prop for prop in properties.values() if isinstance(prop, dict)
        ), f"{model.__name__} has no Literal field but the schema object has enums"


@pytest.mark.parametrize("model", MODEL_POINTERS, ids=_model_id)
def test_field_names_match_schema_properties(model: type, schema: dict[str, Any]) -> None:
    node = _resolve_pointer(schema, MODEL_POINTERS[model])
    schema_keys = set(node["properties"])
    model_keys = {_wire_name(name, info) for name, info in model.model_fields.items()}
    missing_in_python = sorted(schema_keys - model_keys)
    missing_in_schema = sorted(model_keys - schema_keys)
    assert model_keys == schema_keys, (
        f"{model.__name__} field names drifted from the schema:\n"
        f"  in schema but not on the model: {missing_in_python}\n"
        f"  on the model but not in schema: {missing_in_schema}"
    )


def test_every_schema_object_is_covered(schema: dict[str, Any]) -> None:
    """Every object node with properties in the schema must appear in MODEL_POINTERS,
    so a new policy section can't dodge the parity tests."""

    pointers: set[str] = set()

    def walk(node: Any, pointer: str) -> None:
        if not isinstance(node, dict):
            return
        if node.get("type") == "object" and "properties" in node:
            pointers.add(pointer)
            for key, child in node["properties"].items():
                walk(child, f"{pointer}/properties/{key}")

    walk(schema, "")
    assert pointers == set(MODEL_POINTERS.values()), (
        "schema object nodes and MODEL_POINTERS disagree:\n"
        f"  schema objects not covered: {sorted(pointers - set(MODEL_POINTERS.values()))}\n"
        f"  stale pointers in the test: {sorted(set(MODEL_POINTERS.values()) - pointers)}"
    )
