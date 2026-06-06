.PHONY: check rust-check python-check test conformance rust-conformance python-conformance

check: rust-check python-check

rust-check:
	cargo check --workspace

python-check:
	uv sync
	uv run ruff check packages
	uv run python -m compileall packages

# Full test suite across both languages.
test:
	cargo test --workspace
	uv run pytest

# Cross-language conformance over the shared golden fixtures (network-free).
conformance: rust-conformance python-conformance

rust-conformance:
	cargo test -p catalyst-simulation-engine --test conformance

python-conformance:
	uv run pytest tests/conformance
