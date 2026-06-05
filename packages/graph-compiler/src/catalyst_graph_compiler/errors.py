"""Errors raised while compiling a Catalyst graph."""

from __future__ import annotations


class CompileError(ValueError):
    """A graph could not be compiled.

    Carries an optional ``node_id``/``edge`` so callers (and the API layer) can
    point the user at the offending part of the graph.
    """

    def __init__(self, message: str, *, node_id: str | None = None) -> None:
        self.node_id = node_id
        if node_id is not None:
            message = f"{message} (node {node_id!r})"
        super().__init__(message)
