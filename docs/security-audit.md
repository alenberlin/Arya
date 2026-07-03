# Dependency security audit

`cargo audit` (Rust) and `pnpm audit` (JS) run in CI (`.github/workflows/
security-audit.yml`). `pnpm audit --prod` is clean. Three Rust advisories are
present in lockfiles but **not reachable**; each is ignored with the
reachability analysis below and re-checked whenever a dependency updates.

## Ignored advisories (with justification)

### RUSTSEC-2023-0071 — `rsa` Marvin timing attack (medium)
- **Where:** lockfile only, via `sqlx-mysql` (an *optional, disabled*
  dependency of the `sqlx` meta-crate).
- **Reachability:** none. Both crates use `sqlx` with `default-features =
  false` and only the `sqlite` backend enabled. `cargo tree -e normal` shows
  zero `rsa`/`sqlx-mysql` edges in the compiled graph — the vulnerable code is
  never built. The attack also requires triggering RSA *decryptions* (MySQL
  `caching_sha2_password` auth), which Arya never does.
- **Disposition:** ignore; revisit when `sqlx` ships a fix or drops the
  transitive optional dep.

### RUSTSEC-2026-0194 / RUSTSEC-2026-0195 — `quick-xml` XML DoS (high)
- **Where:** via `plist` → `tauri` (framework transitive).
- **Reachability:** none with untrusted input. `plist`/`quick-xml` parse
  Arya's *own* bundle metadata (Info.plist, resources) at build/run time, not
  attacker-controlled documents. The updater consumes JSON (`latest.json`),
  not XML. The DoS requires parsing hostile XML, which no code path exposes.
- **Disposition:** ignore; clears automatically when Tauri bumps `plist`.

## Informational (not failures)

The `gtk`/`atk`/`gdk*`/`proc-macro-error` "unmaintained" warnings are
Linux-only GUI transitives of Tauri. Arya's v1 target is macOS, where they are
not compiled. They do not fail the audit (warnings, not vulnerabilities).

## Re-running locally

```sh
cargo install cargo-audit
cargo audit --file src-tauri/Cargo.lock \
  --ignore RUSTSEC-2023-0071 --ignore RUSTSEC-2026-0194 --ignore RUSTSEC-2026-0195
cargo audit --file arya-api/Cargo.lock --ignore RUSTSEC-2023-0071
pnpm audit --prod
```
