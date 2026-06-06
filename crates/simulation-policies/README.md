# catalyst-simulation-policies

Centralized, versioned rules for every tunable/ambiguous simulation decision, so
assumptions are explicit and reproducible. See
[`docs/simulation-policies.md`](../../docs/simulation-policies.md) and
[`schemas/simulation-policy.schema.json`](../../schemas/simulation-policy.schema.json).

## Model

A `ResolvedPolicy` has **every** knob populated (typed enums + decimal-string
amounts), ready for the engine to consume. Named profiles supply complete
defaults:

| Profile | Character |
| --- | --- |
| `strict_v1` | Deterministic correctness: reject insufficient balance, no partial fills, close fills, crossing triggers, fail on missing required data. |
| `conservative_v1` | Less optimistic: worse-side OHLC fills, higher slippage, adverse same-tick ordering, fallback for optional data. |
| `research_v1` | Exploratory: close fills, lower slippage, forward-fill missing data, tolerate fallbacks. |

## Resolving

```rust
use catalyst_simulation_policies::{resolve, resolve_policy};

let strict = resolve("strict_v1")?;             // by profile name
let resolved = resolve_policy(&contract_policy)?; // profile + partial overrides
```

`resolve_policy` starts from the profile's defaults, applies any knobs explicitly
set in a partial `catalyst_contracts::SimulationPolicy`, then **validates**.

## Validation

`validate` rejects unsupported combinations, e.g.:

- `insufficient_balance = partial_fill` while `partial_fills = none` (contradiction)
- `signal_trigger = crossing_with_cooldown` (or `repeat = with_cooldown`) with no `cooldown`
- non-decimal / negative `slippage_bps`, `fee_bps`, or `gas_fixed_amount` when the
  corresponding model is active

Unknown profile names and unknown enum values produce typed `PolicyError`s.

## Serialization

`ResolvedPolicy` round-trips through JSON (enums use the schema's snake_case
strings). `to_contract()` projects the versioned profile identity back into a
contract policy envelope for embedding in a simulation trace/result.

## Tests

```bash
cargo test -p catalyst-simulation-policies
```

Cover profile defaults (balance, partial fill, trigger, missing data, price
selection), name/override resolution, serialization round-trip, and validation
of unsupported combinations.
