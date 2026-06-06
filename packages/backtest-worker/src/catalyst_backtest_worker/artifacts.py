"""Artifact persistence for a backtest run.

The raw simulation trace and the summarized result are persisted *separately* so
the heavy trace can be retained for debugging while the compact result is served
to users. A run-level metadata document records the selected policy and the data
providers used.
"""

from __future__ import annotations

import json
from pathlib import Path
from typing import Protocol


class ArtifactStore(Protocol):
    def write_trace(self, run_id: str, trace: dict) -> str: ...
    def write_result(self, run_id: str, result: dict) -> str: ...
    def write_metadata(self, run_id: str, metadata: dict) -> str: ...
    def read_result(self, run_id: str) -> dict | None: ...
    def read_trace(self, run_id: str) -> dict | None: ...


class InMemoryArtifactStore:
    """Keeps artifacts in memory (for tests and ephemeral runs)."""

    def __init__(self) -> None:
        self.traces: dict[str, dict] = {}
        self.results: dict[str, dict] = {}
        self.metadata: dict[str, dict] = {}

    def write_trace(self, run_id: str, trace: dict) -> str:
        self.traces[run_id] = trace
        return f"memory://{run_id}/trace"

    def write_result(self, run_id: str, result: dict) -> str:
        self.results[run_id] = result
        return f"memory://{run_id}/result"

    def write_metadata(self, run_id: str, metadata: dict) -> str:
        self.metadata[run_id] = metadata
        return f"memory://{run_id}/metadata"

    def read_result(self, run_id: str) -> dict | None:
        return self.results.get(run_id)

    def read_trace(self, run_id: str) -> dict | None:
        return self.traces.get(run_id)


class FileArtifactStore:
    """Writes artifacts under ``<root>/<run_id>/{trace,result,metadata}.json``."""

    def __init__(self, root: str | Path = "artifacts") -> None:
        self.root = Path(root)

    def _dir(self, run_id: str) -> Path:
        d = self.root / run_id
        d.mkdir(parents=True, exist_ok=True)
        return d

    def _write(self, run_id: str, name: str, payload: dict) -> str:
        path = self._dir(run_id) / f"{name}.json"
        path.write_text(json.dumps(payload, indent=2))
        return str(path)

    def write_trace(self, run_id: str, trace: dict) -> str:
        return self._write(run_id, "trace", trace)

    def write_result(self, run_id: str, result: dict) -> str:
        return self._write(run_id, "result", result)

    def write_metadata(self, run_id: str, metadata: dict) -> str:
        return self._write(run_id, "metadata", metadata)

    def _read(self, run_id: str, name: str) -> dict | None:
        path = self.root / run_id / f"{name}.json"
        return json.loads(path.read_text()) if path.exists() else None

    def read_result(self, run_id: str) -> dict | None:
        return self._read(run_id, "result")

    def read_trace(self, run_id: str) -> dict | None:
        return self._read(run_id, "trace")


__all__ = ["ArtifactStore", "InMemoryArtifactStore", "FileArtifactStore"]
