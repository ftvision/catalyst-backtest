.PHONY: check rust-check python-check

check: rust-check python-check

rust-check:
	cargo check --workspace

python-check:
	uv sync
	uv run ruff check packages
	uv run python -m compileall packages

