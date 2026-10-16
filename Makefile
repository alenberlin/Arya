# Arya development targets. `make verify` is the full local gate and must be
# green before any milestone closes.

.PHONY: verify verify-front verify-rust verify-sidecar verify-release asr-bench e2e help

# On macOS, the active MacOSX27.0 SDK ships a libSystem.B.tbd referencing the
# arm64e.x1 slice. whisper-rs-sys's CMake C-compiler probe can't consume that
# TBD on arm64e machines, so `cargo test` aborts before any of our code runs.
# The shell helper below picks the newest non-27 SDK on the host and exports
# SDKROOT for every cargo invocation in this Makefile. No-op when the active
# SDK is fine, or when no other SDKs are installed.
ifeq ($(shell uname),Darwin)
  ACTIVE_SDK_VERSION := $(shell xcrun --sdk macosx --show-sdk-version 2>/dev/null)
  ifeq ($(ACTIVE_SDK_VERSION),27.0)
    FALLBACK_SDK := $(shell ls -d /Library/Developer/CommandLineTools/SDKs/MacOSX2*.sdk 2>/dev/null | grep -v 'MacOSX27\.0\.sdk' | sort -V | tail -1)
    ifneq ($(FALLBACK_SDK),)
      export SDKROOT := $(FALLBACK_SDK)
      $(info verify: SDKROOT=$(SDKROOT) (auto-pinned; active 27.0 SDK has arm64e TBD breakage))
    endif
  endif
endif

help:
	@echo "verify         run the full gate: brand, secret-scan, lint (incl. sidecar), typecheck, tests (TS + Rust)"
	@echo "verify-front   frontend + secret-scan gate only"
	@echo "verify-rust    rust shell gate only"
	@echo "verify-sidecar sidecar typecheck + tests"
	@echo "verify-release compile the Rust shell in RELEASE (catches release-only breakage the debug gate misses)"
	@echo "asr-bench      run the ignored ASR acceptance gates (downloads ~148 MB models; CI gate)"
	@echo "e2e            run the dev_hooks E2E runner (requires a built debug binary; macOS only)"

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

# E2E runner for the env-gated dev_hooks module. Each scenario spawns the
# local debug binary with the matching ARYA_DEV_* var set and asserts the
# expected tracing line lands on stderr within a bounded timeout. The hooks
# are `#[cfg(debug_assertions)]`, so this gate is meaningful only on a debug
# build — CI builds debug, runs this, then keeps going.
e2e:
	node scripts/e2e-dev-hooks.mjs
