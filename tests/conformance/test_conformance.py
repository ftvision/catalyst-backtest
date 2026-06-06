"""Python-side conformance over the shared golden fixtures.

The Rust engine is checked against the same `tests/golden/` fixtures in
`crates/simulation-engine/tests/conformance.rs`. Per [ADR 0001] the run path is
Rust now; the only thing the two languages still share is the **data shapes**.
So the Python side here proves that every golden input still validates against the
shared JSON Schemas (`schemas/`) that both the Rust serde types and the Python
Pydantic models are generated from — keeping the contract aligned across the
boundary.

[ADR 0001]: ../../docs/adr/0001-language-boundary.md
"""

from __future__ import annotations

import json
from pathlib import Path

import pytest

from catalyst_contracts import validate


def _repo_root() -> Path:
    for parent in Path(__file__).resolve().parents:
        if (parent / "tests" / "golden").is_dir():
            return parent
    raise RuntimeError("repo root not found")


ROOT = _repo_root()
GOLDEN = sorted((ROOT / "tests" / "golden").glob("*.json"))
CASES = [(p.stem, json.loads(p.read_text())) for p in GOLDEN]

EXPECT_KEYS = {"executed", "rejected", "open_perps", "open_yields", "balances_present"}


def test_golden_suite_is_non_empty() -> None:
    assert CASES, "no golden fixtures found in tests/golden/"


@pytest.mark.parametrize("name,doc", CASES, ids=[n for n, _ in CASES])
def test_golden_inputs_validate_against_schemas(name: str, doc: dict) -> None:
    inp = doc["input"]
    validate(inp["graph"], "graph")
    validate(inp["market_data"], "market-data-bundle")
    # expected invariants are well-formed
    assert EXPECT_KEYS <= set(doc["expect"]), f"{name} missing expect keys"
