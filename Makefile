# Arya development targets. `make verify` is the full local gate and must be
# green before any milestone closes.

.PHONY: verify verify-front verify-rust help

help:
	@echo "verify        run the full gate: brand check, lint, typecheck, tests (TS + Rust)"
	@echo "verify-front  frontend gate only"
	@echo "verify-rust   rust shell gate only"

verify: verify-front verify-rust

verify-front:
	pnpm brand:check
	pnpm check
	pnpm typecheck
	pnpm test

verify-rust:
	cargo fmt --manifest-path src-tauri/Cargo.toml --check
	cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
	cargo test --manifest-path src-tauri/Cargo.toml
