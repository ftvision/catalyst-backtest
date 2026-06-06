# ADR 0001 — Language boundary: Rust service path, Python data/analysis client

- **Status:** Accepted
- **Supersedes framing of:** #28 (graph-compile / policy dedup)
- **Confirms:** #29 (data-plane: Rust reads the Parquet store directly)
- **Tracked by:** #43 (migration)

## Context

The system uses two languages. The deterministic core ended up **split across the
language boundary**: graph compilation, result reporting, orchestration, and the
HTTP API were in Python, while policy, ledger, execution, the engine, the
simulation service, and the Parquet loader were in Rust.

That split is the root cause of two recurring problems:

- **#28** — the same *behavior* (graph trigger derivation; policy resolution) is
  implemented twice and can drift.
- **#29** — bulk market data crosses the boundary as JSON, re-encoded on both
  sides.

There is **no capability gap** forcing the split: Rust has the full stack we
need — `reqwest` (HTTP client), `serde_json`, `arrow`/`parquet`/`object_store`,
`axum`/`tokio` (server), `chrono`, `rust_decimal`. Python's durable value is at
the **edges**: data-source adapters (provider churn + rich SDKs) and
research/analysis (notebooks, plotting), not the deterministic core.

The guiding principle: **never split domain logic across a language boundary.**
Put it entirely on one side; let the other side call across a thin seam.

## Decision

1. **The entire service/run path is Rust**: contracts (serde), compile, policy,
   execution, ledger, engine, Parquet store read (loader), reporter,
   orchestration, and the HTTP API (`axum`).
2. **Python is a client + data plumbing only**:
   - **Ingestion** — fetch from data sources and write the Parquet store.
   - **Analysis** — notebooks/research that **call the Rust HTTP API** and
     deserialize results (via Pydantic) for plotting.
3. **The boundary is data at rest**: the Parquet market-data store (Python
   writes, Rust reads) and run/result artifacts, plus the Rust HTTP API for
   clients. **No domain logic is shared across languages.**
4. **The only permitted cross-language overlap is data *shapes*** — the
   JSON-Schema contracts in `schemas/`, projected to Rust `serde` and Python
   `pydantic`, single-sourced and guarded by round-trip fixtures. Python
   `catalyst-contracts` survives solely to deserialize results in notebooks.

## Consequences

- Resolves **#28** (one compiler + one policy resolver, in Rust) and **#29**
  (Rust reads the store directly; no JSON bundle across the boundary).
- One deployable for the service; determinism and performance end-to-end.
- Python run-path packages (`graph-compiler`, `result-reporter`,
  `backtest-worker`, `backtest-api`) are retired from the critical path; kept only
  if useful for research, never authoritative.
- **Cost (accepted):** porting compiler/reporter/orchestration/API to Rust;
  service-path contributors need Rust.
- **Non-goals:** rewriting data-source adapters or research tooling in Rust.
  Python stays there while providers churn. Going 100% Rust later is possible but
  out of scope for this ADR.

## Migration (incremental, each shippable — see #43)

**Status: complete.** All five steps have shipped; the run path is entirely Rust.

1. ✅ **Reporter → Rust** — the service returns the summarized result.
2. ✅ **Compiler → Rust** — data-requirement extraction beside `ExecGraph`; the
   service compiles internally instead of taking `data_requirements` in the
   request (resolves #28).
3. ✅ **API → Rust (`axum`)** — run lifecycle + preview/coverage/policy-profiles
   using the Rust compiler/policy directly → delete the Python policy mirror.
   (Runs are async: submit → poll → fetch, drained by a bounded worker pool.)
4. ✅ **Orchestration + persistence → Rust** — run status + artifact store.
5. ✅ **Retire** the Python run-path packages; Python = ingestion + analysis
   client (`contracts` + `market-data` only).
