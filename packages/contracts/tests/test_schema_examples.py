"""Every example payload must validate against its JSON Schema."""

from __future__ import annotations

import json
from pathlib import Path

import pytest

from catalyst_contracts.schemas import schemas_dir, validate

EXAMPLES_DIR = schemas_dir() / "examples"

# (example filename, schema name)
CASES = [
    ("graph.swap.json", "graph"),
    ("graph.perp-signal.json", "graph"),
    ("graph.yield.json", "graph"),
    ("graph.limit-order.json", "graph"),
    ("graph.threshold-funding.json", "graph"),
    ("graph.threshold-composed.json", "graph"),
    ("graph.relative-sizing.json", "graph"),
    ("graph.ma-cross.json", "graph"),
    ("backtest-request.json", "backtest-request"),
    ("simulation-policy.strict_v1.json", "simulation-policy"),
    ("market-data-bundle.json", "market-data-bundle"),
    ("simulation-trace.json", "simulation-trace"),
    ("backtest-result.json", "backtest-result"),
]


def _load(name: str) -> dict:
    return json.loads((EXAMPLES_DIR / name).read_text())


@pytest.mark.parametrize("filename,schema_name", CASES)
def test_example_validates_against_schema(filename: str, schema_name: str) -> None:
    validate(_load(filename), schema_name)


def test_all_examples_are_covered() -> None:
    on_disk = {p.name for p in Path(EXAMPLES_DIR).glob("*.json")}
    covered = {filename for filename, _ in CASES}
    assert on_disk == covered, f"uncovered example fixtures: {on_disk - covered}"


def test_swap_node_with_bad_config_is_rejected() -> None:
    import jsonschema

    bad = _load("graph.swap.json")
    del bad["nodes"][0]["config"]["amount"]  # required by swapConfig
    with pytest.raises(jsonschema.ValidationError):
        validate(bad, "graph")


def test_unknown_node_subtype_is_rejected() -> None:
    import jsonschema

    bad = _load("graph.swap.json")
    bad["nodes"][0]["subtype"] = "options_order"
    with pytest.raises(jsonschema.ValidationError):
        validate(bad, "graph")


def test_variable_token_validates_in_decimal_fields() -> None:
    # a "$name" token is accepted where a decimal is expected; the compiler
    # resolves it later.
    g = _load("graph.swap.json")
    g.setdefault("variables", {})["size"] = "100"
    g["nodes"][0]["config"]["amount"] = "$size"
    validate(g, "graph")  # must not raise


def test_non_scalar_variable_value_is_rejected() -> None:
    import jsonschema

    g = _load("graph.swap.json")
    g["variables"] = {"size": {"nested": "object"}}
    with pytest.raises(jsonschema.ValidationError):
        validate(g, "graph")
