# Arya development targets. `make verify` is the full local gate and must be
# green before any milestone closes.

.PHONY: verify verify-front verify-rust verify-sidecar verify-release asr-bench help

help:
	@echo "verify         run the full gate: brand, secret-scan, lint (incl. sidecar), typecheck, tests (TS + Rust)"
	@echo "verify-front   frontend + secret-scan gate only"
	@echo "verify-rust    rust shell gate only"
	@echo "verify-sidecar sidecar typecheck + tests"
	@echo "verify-release compile the Rust shell in RELEASE (catches release-only breakage the debug gate misses)"
	@echo "asr-bench      run the ignored ASR acceptance gates (downloads ~148 MB models; CI gate)"

verify: verify-front verify-rust verify-sidecar

verify-sidecar:
	pnpm --filter arya-sidecar typecheck
	pnpm --filter arya-sidecar test

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

# ASR acceptance gates (WER + RTF + speaker embedding sanity). These tests
# are `#[ignore]`'d because they download large speech models on first run
# (~148 MB whisper base, ~73 MB streaming zipformer, ~30 MB embedding). Run
# them in release mode so the RTF budget matches what users actually see.
# The CI workflow `.github/workflows/asr-bench.yml` invokes this on every PR
# so regressions land a red CI immediately.
asr-bench:
	cargo test --release --manifest-path src-tauri/Cargo.toml \
	    --test speech_bench -- --ignored --nocapture
	cargo test --release --manifest-path src-tauri/Cargo.toml \
	    --test streaming_bench -- --ignored --nocapture
	cargo test --release --manifest-path src-tauri/Cargo.toml \
	    --test diarize_probe -- --ignored --nocapture
