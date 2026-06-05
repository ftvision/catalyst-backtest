"""Shared base model and the decimal-string convention used across contracts.

Monetary and quantity values are carried as decimal *strings* (e.g. ``"100"``,
``"0.04"``) so that no precision is lost crossing the Python <-> JSON <-> Rust
boundary. Parsing into a real decimal type is the consumer's responsibility.
"""

from __future__ import annotations

from pydantic import BaseModel, ConfigDict

# A decimal value carried as a string to preserve precision on the wire.
Decimal = str


class StrictModel(BaseModel):
    """Base model that forbids unknown fields, matching ``additionalProperties: false``."""

    model_config = ConfigDict(extra="forbid")


class OpenModel(BaseModel):
    """Base model that tolerates unknown fields (for forward-compatible envelopes)."""

    model_config = ConfigDict(extra="allow")
