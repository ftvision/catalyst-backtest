"""Policy profile resolution for the API (offline).

The authoritative resolver lives in the Rust ``catalyst-simulation-policies``
crate. The API needs to resolve profiles *offline* (for the preview and
profile-selector endpoints) without a round trip to the simulation service, so we
mirror the three profiles here. The contract ``SimulationPolicy`` already encodes
the ``strict_v1`` defaults, so each profile is "strict + a few overrides", exactly
like ``crates/simulation-policies/src/profiles.rs``.

NOTE: keep these in sync with that crate (tracked alongside the dedup work in
issue #28; PyO3 would let us drop this mirror entirely).
"""

from __future__ import annotations

from catalyst_contracts import SimulationPolicy

# Profile id -> (label, one-line description, overrides on top of strict defaults).
_PROFILES: dict[str, tuple[str, str, dict]] = {
    "strict_v1": (
        "Strict",
        "Deterministic correctness: reject insufficient balance, no partial fills, "
        "close fills, crossing triggers, fail on missing required data.",
        {},
    ),
    "conservative_v1": (
        "Conservative",
        "Less optimistic: worse-side OHLC fills, higher slippage, adverse same-tick "
        "ordering, fallback for optional data.",
        {
            "fills": {
                "price_selection": "worse_side_ohlc",
                "slippage": {"model": "fixed_bps", "bps": "25"},
                "fees": {"model": "fixed_bps", "bps": "8"},
            },
            "ordering": {"same_tick": "conservative_adverse_order"},
            "data": {"missing_optional": "fallback_provider"},
        },
    ),
    "research_v1": (
        "Research",
        "Exploratory: close fills, lower slippage, forward-fill missing data, tolerate fallbacks.",
        {
            "fills": {
                "partial_fills": "allow_if_configured",
                "slippage": {"model": "fixed_bps", "bps": "5"},
            },
            "data": {"missing_required": "forward_fill", "missing_optional": "fallback_provider"},
        },
    ),
}

PROFILE_IDS = tuple(_PROFILES)


def resolve_profile(profile: str) -> SimulationPolicy:
    """Resolve a profile id into a fully-populated ``SimulationPolicy``."""

    if profile not in _PROFILES:
        raise KeyError(profile)
    _, _, overrides = _PROFILES[profile]
    return SimulationPolicy.model_validate({"profile": profile, **overrides})


def resolve_request_policy(policy: dict | None) -> SimulationPolicy:
    """Resolve the ``policy`` field of a request (a ``{profile}`` selector)."""

    profile = (policy or {}).get("profile", "strict_v1")
    return resolve_profile(profile)


def list_profiles() -> list[dict]:
    """List profiles for the selector UI: id, label, description, resolved policy."""

    out = []
    for pid, (label, description, _) in _PROFILES.items():
        out.append(
            {
                "id": pid,
                "label": label,
                "description": description,
                "resolved_policy": resolve_profile(pid).model_dump(by_alias=True),
            }
        )
    return out


__all__ = ["resolve_profile", "resolve_request_policy", "list_profiles", "PROFILE_IDS"]
