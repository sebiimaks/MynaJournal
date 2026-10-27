# Dependency notice snapshot

`THIRD_PARTY_NOTICES.txt` is the distribution document. It contains the reviewed
dependency notices, native-runtime notices, and the complete unchanged published
source of the MPL-2.0 dependency `option-ext` 0.2.0. Ship that file together with
the application's `LICENSE` and `ATTRIBUTION.md`, including with workflow binary
artifacts. Dependencies retain their own licences.

The snapshot covers all 422 external packages in the current `Cargo.lock`,
including build dependencies and other target platforms. It is intentionally a
superset of any individual Mac or Linux binary. Root licence files are not
sufficient for every package: the snapshot also retains nested notices, including
WHATWG data, Unicode data, the Wayland protocols, tracing's spin-lock code,
libgit2's native dependencies, SQLite's public-domain notice, zlib, and the
Unicode CLDR 37 data transformed into rules by `intl_pluralrules`.

## Files and verification

- `registry.json` records exact versions, crate archive checksums, declared and
  selected licences, source revisions where published, and the provenance and
  SHA-256 of each notice or source file.
- `texts.json` stores those UTF-8 texts keyed by their SHA-256. Identical
  files are stored once. Explicitly labelled source-header excerpts preserve the
  relevant copyright/licence statements; other upstream files are complete. A
  separately labelled maintainer explanation identifies the CLDR transformation.
- `runtime/manifest.json` and its adjacent files cover the reviewed Rust, Zig,
  musl and unwind runtimes. See `scripts/update-runtime-notices.py` for the
  runtime snapshot procedure. `runtime-supplemental.json` records additional
  Go/LLVM licences referenced by ports in Zig; review these on runtime upgrades.

Run these commands with Python 3.11 or newer; they do not access the network:

```sh
python3 scripts/generate-notices.py --check
python3 scripts/test-generate-notices.py
```

The generator rejects added/removed dependencies, changed crate checksums,
missing licence texts, altered notice text, missing MPL source, and changed native
runtime notices. The source snapshot was checked against every original cached
`.crate` archive and each archive's checksum in `Cargo.lock` during this review.

## Updating dependencies

A changed lockfile deliberately blocks the notice check. Review the new packages
before updating the snapshot; a package's SPDX expression alone does not identify
all bundled components or satisfy notice requirements.

1. Use `cargo metadata --locked --offline --format-version 1` after fetching the
   locked dependencies to locate the exact crate sources. Check the `.crate`
   archive checksum against `Cargo.lock` before collecting notices.
2. Inspect each changed crate's licence files, copyright statements, bundled
   native code/data and enabled features. Record a permitted licence selection
   and all required additional notices. Retain any source-availability duties.
3. Add the corresponding package records to `registry.json`. Store original
   notice/source text in `texts.json` under its UTF-8 SHA-256 and reference that
   digest from each record. Preserve the original source path and package
   version. For a file missing from a published crate, use the exact revision in
   `.cargo_vcs_info.json` to obtain its upstream file and record the immutable URL.
4. If upstream only references a standard licence, preserve that grant as well
   as the referenced standard text. Do not invent a copyright holder or date.
5. Review native runtime updates separately, then run
   `python3 scripts/generate-notices.py` and both checks above. Review the diff
   before distributing a new build.

`option-ext`'s entire source archive is represented as separate files in the
source appendix, excluding only Cargo's local `.cargo-ok` cache marker. Its
licence is MPL-2.0; the application remains MIT. `libgit2`'s GPLv2 linking
exception permits linking the unchanged library with the application. No
`tree_magic_db` GPL database is enabled or bundled by the current dependency graph.

## Runtime versions

The runtime snapshot covers Rust **1.99.1** and, for the static Linux test build,
Zig **0.15.2** and its bundled musl/unwind code. The Debian workflow pins
`rust:1.99.1-bookworm` to match. CI also tests the app on stable Rust and its MSRV;
those checks do not certify a different runtime for redistribution. A custom
binary distributor must review the actual toolchain and any changed native
libraries before reusing this notice bundle.

To update the runtime snapshot, use the complete Rust distribution's
`COPYRIGHT-library.html` and adjacent `licenses/` directory, the matching Rust
source tree, and the Zig distribution used for static Linux builds:

```sh
python3 scripts/update-runtime-notices.py \
  --rust-version VERSION --rust-docs /path/to/rust/share/doc/rust \
  --rust-source /path/to/rust-source \
  --zig-version VERSION --zig /path/to/zig
```

The script converts the Rust HTML report to plain text while retaining its
original checksum, copies runtime licence texts, and collects additional musl
source notices. Review the resulting provenance and component changes, update
the Debian toolchain pin as needed, then regenerate and check the bundle.

See [the review record](REVIEW.md) for findings and the scope of this audit.
