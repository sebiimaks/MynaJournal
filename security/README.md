# Dependency security checks

The `Dependency security` workflow checks the locked dependency graph against the
current RustSec database on pull requests, main pushes, manual runs and weekly.
It runs only in development/CI; the journal app does not contact an advisory
service or add network activity.

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

- `crypto-bigint` 0.5.5 (`RUSTSEC-2026-0387`) is used by recipient types in the
  current `age` 0.12.1 release. MynaJournal uses passphrase/scrypt recipients and
  rejects other envelope types. Reassess this exception if note protection or
  `age` changes; this is not a general claim that the dependency is unaffected.
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
