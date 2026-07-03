# Arya development targets. `make verify` is the full local gate and must be
# green before any milestone closes.

.PHONY: verify verify-front verify-rust help

help:
	@echo "verify        run the full gate: brand check, lint, typecheck, tests (TS + Rust)"
	@echo "verify-front  frontend gate only"
	@echo "verify-rust   rust shell gate only"

verify: verify-front verify-rust verify-sidecar verify-api

verify-sidecar:
	pnpm --filter arya-sidecar typecheck
	pnpm --filter arya-sidecar test

verify-api:
	cargo fmt --manifest-path arya-api/Cargo.toml --check
	cargo clippy --manifest-path arya-api/Cargo.toml --all-targets -- -D warnings
	cargo test --manifest-path arya-api/Cargo.toml

verify-front:
	pnpm brand:check
	node scripts/scan-keys.mjs
	pnpm check
	pnpm typecheck
	pnpm test

verify-rust:
	cargo fmt --manifest-path src-tauri/Cargo.toml --check
	cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
	cargo test --manifest-path src-tauri/Cargo.toml
