"""Locate and validate against the language-neutral JSON Schemas.

The schemas live in the repo-level ``schemas/`` directory and are the source of
truth shared by the Python and Rust contract packages. This module finds that
directory, loads schemas (resolving cross-file ``$ref``s by ``$id``), and offers
a thin ``validate`` helper.

``jsonschema`` is an optional dependency; the helpers import it lazily so that
merely importing the contract models never requires it.
"""

from __future__ import annotations

import json
import os
from functools import lru_cache
from pathlib import Path
from typing import Any

# Maps a logical contract name to its schema filename.
SCHEMA_FILES: dict[str, str] = {
    "graph": "graph.schema.json",
    "backtest-request": "backtest-request.schema.json",
    "backtest-result": "backtest-result.schema.json",
    "market-data-bundle": "market-data-bundle.schema.json",
    "simulation-policy": "simulation-policy.schema.json",
    "simulation-trace": "simulation-trace.schema.json",
}


def schemas_dir() -> Path:
    """Return the repo ``schemas/`` directory.

    Honors ``CATALYST_SCHEMAS_DIR`` when set, otherwise walks up from this file
    until a directory containing ``graph.schema.json`` is found.
    """

    override = os.environ.get("CATALYST_SCHEMAS_DIR")
    if override:
        return Path(override)

    for parent in Path(__file__).resolve().parents:
        candidate = parent / "schemas"
        if (candidate / "graph.schema.json").exists():
            return candidate
    raise FileNotFoundError(
        "Could not locate the schemas/ directory; set CATALYST_SCHEMAS_DIR to point at it."
    )


@lru_cache(maxsize=None)
def load_schema(name: str) -> dict[str, Any]:
    """Load one schema document by logical name (see ``SCHEMA_FILES``)."""

    if name not in SCHEMA_FILES:
        raise KeyError(f"Unknown schema {name!r}; known: {sorted(SCHEMA_FILES)}")
    path = schemas_dir() / SCHEMA_FILES[name]
    return json.loads(path.read_text())


def _build_registry():
    """Build a ``referencing`` registry of all schemas keyed by their ``$id``."""

    from referencing import Registry, Resource

    resources = []
    for filename in SCHEMA_FILES.values():
        doc = json.loads((schemas_dir() / filename).read_text())
        resource = Resource.from_contents(doc)
        resources.append((doc["$id"], resource))
    return Registry().with_resources(resources)


def validate(instance: Any, schema_name: str) -> None:
    """Validate ``instance`` against the named schema, resolving cross-file refs.

    Raises ``jsonschema.ValidationError`` on failure.
    """

    from jsonschema import Draft202012Validator

    schema = load_schema(schema_name)
    validator = Draft202012Validator(schema, registry=_build_registry())
    validator.validate(instance)


__all__ = ["SCHEMA_FILES", "schemas_dir", "load_schema", "validate"]
