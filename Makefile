# Arya development targets. `make verify` is the full local gate and must be
# green before any milestone closes.

.PHONY: verify verify-front verify-rust verify-sidecar verify-api verify-release help

help:
	@echo "verify         run the full gate: brand, secret-scan, lint (incl. sidecar), typecheck, tests (TS + Rust)"
	@echo "verify-front   frontend + secret-scan gate only"
	@echo "verify-rust    rust shell gate only"
	@echo "verify-sidecar sidecar typecheck + tests"
	@echo "verify-api     arya-api fmt + clippy + tests"
	@echo "verify-release compile both Rust workspaces in RELEASE (catches release-only breakage the debug gate misses)"

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

# The default `verify` gate compiles everything in DEBUG (cargo test/clippy),
# where `debug_assertions` is ON. Release-only code is therefore never compiled:
# anything behind `cfg(not(debug_assertions))`, and the non-debug arm of
# `generate_handler!` in lib.rs (the `#[cfg(debug_assertions)]` command is
# dropped in release — if a sibling symbol is mis-gated the release build
# breaks). The signed release workflow is manual-dispatch only and never runs
# on PRs, so without this target a release-only break merges undetected. Builds
# the frontend first because tauri's `generate_context!` embeds `../dist`.
verify-release:
	pnpm build
	cargo build --release --manifest-path src-tauri/Cargo.toml
	cargo build --release --manifest-path arya-api/Cargo.toml
