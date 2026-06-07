"""Every catalog strategy graph must validate against the Python `Graph` model.

This guards against the Python contract drifting behind the Rust contract / JSON
schema (e.g. a new node subtype added there but not here), which would make
Python clients reject graphs the service accepts.
"""

from __future__ import annotations

import json
from pathlib import Path

import pytest

from catalyst_contracts.graph import Graph
from catalyst_contracts.schemas import schemas_dir

GRAPHS_DIR = schemas_dir().parent / "strategies" / "graphs"
GRAPH_FILES = sorted(GRAPHS_DIR.glob("*.json"))


def test_catalog_graphs_present() -> None:
    # g01-g18 originals + g19-g26 ADR-0002 strategies.
    assert len(GRAPH_FILES) >= 26, f"expected the catalog graphs, found {len(GRAPH_FILES)}"


@pytest.mark.parametrize("path", GRAPH_FILES, ids=lambda p: p.stem)
def test_catalog_graph_validates(path: Path) -> None:
    Graph.model_validate(json.loads(path.read_text()))


def test_adr0002_subtypes_accepted() -> None:
    for subtype in ("threshold", "all", "any", "not"):
        Graph.model_validate(
            {"nodes": [{"id": "n", "kind": "signal", "subtype": subtype, "config": {}}]}
        )
