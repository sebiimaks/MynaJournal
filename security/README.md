# Dependency security checks

The `Dependency security` workflow checks the locked dependency graph against the
current RustSec database on pull requests, main pushes, manual runs and weekly.
It runs only in development/CI; the journal app does not contact an advisory
service or add network activity.

The 9 November 2026 SQLite update addresses
[RUSTSEC-2026-0459](https://rustsec.org/advisories/RUSTSEC-2026-0459.html)
with `libsqlite3-sys` 0.39.1 and bundled SQLite 3.54.1. SQLx 0.8.6's driver is
vendored with a dependency/feature patch to retain Rust 1.92 support; its Rust
source is unchanged. See [the patch record](../vendor/sqlx-sqlite/MYNA-PATCH.md).
The SQLite integration tests check the linked runtime's patched version, and
the notice check verifies the vendored source and licence inventory. This
advisory has no exception in the security policy.

With `cargo-audit` 0.22.2 installed, run:

```sh
cargo fetch --locked
python3 scripts/test-audit-dependencies.py
python3 scripts/audit-dependencies.py
```

An existing database checkout can be supplied with
`--database /path/to/advisory-db --no-fetch`. Refresh that checkout before treating
the result as a current security review.

## Reviewed exceptions

`advisory-exceptions.json` records exact advisory IDs, package versions, warning
categories, reasons and expiry dates. New findings, changed versions, expired
decisions and obsolete exceptions fail the check. The script also checks the
normal/build graph with every application feature and target enabled.

The 28 October 2026 review removed the affected Tokio and LRU versions and the
unmaintained `paste` dependency. Three findings remain in the lockfile:

- `crypto-bigint` 0.5.5 (`RUSTSEC-2026-0387`) is used by P256/HPKE recipient code
  in the locked `age` 0.12.1 dependency. The 9 November 2026 review checked the
  added journal-protection paths: individual notes and wrapped journal keys
  require scrypt envelopes, while journal records use explicit X25519 recipients
  and identities. The reviewed X25519 implementation uses `x25519-dalek`, HKDF
  and ChaCha20-Poly1305, and ignores other recipient stanzas before key operations.
  The app does not invoke generic identity dispatch or P256/HPKE recipients.
  Regression tests cover these envelope restrictions. Reassess this exception if
  `age`, identity dispatch or recipient types change; this is not a general claim
  that the dependency is unaffected.
- `rsa` 0.9.10 (`RUSTSEC-2023-0071`) belongs to optional dependency features that
  are absent from the application's normal/build graph. CI fails if it becomes
  enabled. SSH recipients and MySQL are not enabled.
- `basic-toml` 0.1.10 (`RUSTSEC-2026-0374`) is an unmaintained build-time i18n
  dependency of the current `age` release. The advisory reports maintenance
  status rather than a vulnerability.

All three decisions expire on **31 December 2026**. Review upstream fixes and
the actual call paths before changing a version or extending an exception.
Dependency checks supplement code review and regression tests; a passing check
means there are no unreviewed findings in that database, not that the app is
guaranteed free of vulnerabilities.
