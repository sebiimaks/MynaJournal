# SQLite security compatibility patch

This directory contains the published `sqlx-sqlite` 0.8.6 crate, retaining its
upstream source and MIT/Apache-2.0 licence files.

- Source archive: https://crates.io/api/v1/crates/sqlx-sqlite/0.8.6/download
- Archive SHA-256: `c2d12fe70b2c1b4401038055f90f151b78208de1f9f89a7dbfd41587a10c3eea`
- Upstream revision: `bab1b022bd56a64f9a08b46b36b97c5cff19d77e`
- Upstream directory: `sqlx-sqlite`

Only the normalized `Cargo.toml` dependency declaration changes:

1. Require `libsqlite3-sys` 0.39.1, which bundles SQLite 3.54.1 and fixes
   [RUSTSEC-2026-0459](https://rustsec.org/advisories/RUSTSEC-2026-0459.html).
2. Enable its `column_metadata` feature, retaining the SQLite column metadata
   APIs that SQLx 0.8.6 uses and that older `libsqlite3-sys` enabled implicitly.

`Cargo.toml.orig` is the unchanged upstream manifest for comparison. Driver Rust
source is unchanged. Cargo's local cache marker is not included.

This narrow patch retains MynaJournal's Rust 1.92 minimum. SQLx 0.9 requires Rust
1.94, and its published SQLite driver does not yet accept `libsqlite3-sys` 0.39.
Remove this patch when a supported upstream release can use a patched SQLite
runtime while preserving the application's supported compiler range.
