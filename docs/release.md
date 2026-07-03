# Release runbook

## Prerequisites (one-time, needs the owner's accounts)

These are the external credentials Arya needs before its first public
release. Everything else — build, test, and the full app in local mode —
works without them.

| Secret | Where | Purpose |
|---|---|---|
| `APPLE_CERTIFICATE` (+ password) | Apple Developer | Code-sign the `.app` |
| `APPLE_SIGNING_IDENTITY` | Apple Developer | Developer ID Application name |
| `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID` | Apple Developer | Notarization |
| `TAURI_SIGNING_PRIVATE_KEY` (+ password) | `pnpm tauri signer generate` | Sign the auto-update manifest |
| Clerk keys | Clerk dashboard | Hosted sign-in (`ARYA_API_MODE=clerk`) |
| Stripe keys | Stripe dashboard | Subscriptions + credit top-ups |
| Provider keys | Anthropic / OpenAI | Cloud model inference (Arya API env only) |

## Desktop release

1. Bump `version` in `package.json` and `src-tauri/tauri.conf.json`.
2. Set the updater `pubkey` and `endpoints` in `tauri.conf.json` to the
   public releases repo.
3. Run the `release-macos` workflow (channel `rc` first). `tauri-action`
   builds, signs, notarizes, and publishes the DMG + signed `latest.json`.
4. Verify an update from the previous build installs on a clean macOS 14 VM.
5. Promote `rc` → `stable` by re-running with channel `stable`.

## Arya API deploy

1. `build-arya-api` publishes `ghcr.io/<org>/arya-api:<sha>` + `:staging`.
2. Deploy the immutable per-commit tag (never `:latest`) so the running
   digest is always traceable to a source commit.
3. Provider keys and auth mode come from the deployment environment, never
   the image.

## Pre-flight checklist

- [ ] `make verify` green (all four gates).
- [ ] `node scripts/scan-keys.mjs` — no provider key in the desktop bundle.
- [ ] Security review findings addressed (docs/darkfactory/REPORT.md).
- [ ] Update from build N-1 → N succeeds on a clean machine.
- [ ] Rollback: keep the previous `latest.json` + DMG; re-publishing them
      reverts the fleet on the next update check.
