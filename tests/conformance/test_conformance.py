"""Python-side conformance over the shared golden fixtures.

The Rust engine is checked against the same `tests/golden/` fixtures in
`crates/simulation-engine/tests/conformance.rs`. Here we check that the *Python*
orchestration pipeline agrees on the same inputs, fully offline:

- the engine input (graph + market data bundle) validates against the schemas,
- every graph compiles, and
- the embedded market data bundle satisfies the compiled data requirements
  (building with ``missing="fail"`` proves coverage).

Keeping both languages green on one fixture suite is what keeps them aligned.
"""

from __future__ import annotations

import json
from datetime import datetime
from pathlib import Path

import pytest

from catalyst_contracts import validate
from catalyst_graph_compiler import CompiledGraph, compile_graph
from catalyst_market_data import FixtureSource, build_bundle


def _repo_root() -> Path:
    for parent in Path(__file__).resolve().parents:
        if (parent / "tests" / "golden").is_dir():
            return parent
    raise RuntimeError("repo root not found")


ROOT = _repo_root()
GOLDEN = sorted((ROOT / "tests" / "golden").glob("*.json"))
CASES = [(p.stem, json.loads(p.read_text())) for p in GOLDEN]

EXPECT_KEYS = {"executed", "rejected", "open_perps", "open_yields", "balances_present"}


def _dt(value: str) -> datetime:
    return datetime.fromisoformat(value.replace("Z", "+00:00"))


def test_golden_suite_is_non_empty() -> None:
    assert CASES, "no golden fixtures found in tests/golden/"


@pytest.mark.parametrize("name,doc", CASES, ids=[n for n, _ in CASES])
def test_golden_inputs_validate_against_schemas(name: str, doc: dict) -> None:
    inp = doc["input"]
    validate(inp["graph"], "graph")
    validate(inp["market_data"], "market-data-bundle")
    # expected invariants are well-formed
    assert EXPECT_KEYS <= set(doc["expect"]), f"{name} missing expect keys"


@pytest.mark.parametrize("name,doc", CASES, ids=[n for n, _ in CASES])
def test_golden_graphs_compile(name: str, doc: dict) -> None:
    compiled = compile_graph(doc["input"]["graph"])
    assert isinstance(compiled, CompiledGraph)


@pytest.mark.parametrize("name,doc", CASES, ids=[n for n, _ in CASES])
def test_golden_market_data_covers_requirements(name: str, doc: dict) -> None:
    inp = doc["input"]
    compiled = compile_graph(inp["graph"])
    source = FixtureSource.from_dict(inp["market_data"])
    cfg = inp["config"]
    # missing="fail" raises if any required series is absent -> proves coverage.
    bundle = build_bundle(
        compiled,
        start=_dt(cfg["start"]),
        end=_dt(cfg["end"]),
        interval=cfg["interval"],
        source=source,
        missing="fail",
    )
    assert bundle.warnings == []
