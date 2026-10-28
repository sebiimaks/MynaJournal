# Licensing and attribution review — 28 October 2026

This review covers the current MynaJournal working tree, its inherited source
and assets, the 501 external packages in `Cargo.lock`, and the Mac/Debian/static
Linux distribution paths. The initial review covered the MynaJournal rename,
artwork, menu and Priority changes released in 0.19.0. This update covers the
subsequent dependency changes in the local security and reliability work. It is
a technical compliance review, not a
legal opinion or a guarantee about third parties' underlying rights.

## Upstream permission and attribution

The root [LICENSE](../LICENSE) is unchanged from
[TUI-Journal's MIT license](https://github.com/AmmarAbouZor/tui-journal/blob/main/LICENSE),
including `Copyright (c) 2023 Ammar Abou Zor`. The reviewed kotachisam fork has
the same license. MIT permits modification, renaming and redistribution while
requiring the copyright and permission notices to accompany copies or
substantial portions. It does not require changes to be contributed upstream.
The upstream contribution/AI policy governs contributions; it is not an
additional condition in the MIT license.

[ATTRIBUTION.md](../ATTRIBUTION.md) records the original project, inherited
contributors, adapted library code and assets. README dependency credits now
name the actual editor library, ratatui-textarea. Its full MIT notice retains
rhysd's copyright; the original tui-textarea 0.7.0 notice is also explicitly
retained for the application's adapted clipboard mappings. Citation metadata
distinguishes
the fork maintainer from its upstream origin, and CODEOWNERS names the fork's
maintainer without implying upstream responsibility.

## Gaps corrected

- Previously distributed binaries lacked a complete dependency-notice bundle.
  [THIRD_PARTY_NOTICES.txt](../THIRD_PARTY_NOTICES.txt) now retains full notices,
  including nested native/data components that package SPDX fields omit. The
  exact crate archives and collected texts were checked against their hashes.
- `option-ext` 0.2.0 uses MPL-2.0. Its complete unchanged published source is
  included in the bundle and exposed by `mjournal licenses`; recipients do not
  need a separate download to obtain it. The MPL component remains MPL-licensed.
- libgit2's native GPLv2 license includes an explicit linking exception. Its
  full license and bundled notices are preserved; the unchanged library can be
  linked with this application without relicensing the app under the GPL.
- An inherited configuration helper copied Serde documentation, whose
  repository uses CC BY-SA 4.0 rather than the Serde crate's MIT/Apache licenses.
  The copied visitor/comments were removed and replaced with a small derived
  string-or-table decoder. Existing string/table formats and validation are
  covered by regression tests. Historical provenance is recorded in ATTRIBUTION.
- A test-only model referencing Linux kernel color-handling internals was
  replaced with independent terminal-output assertions. This avoids retaining
  that implementation model; it is not a conclusion that protocol facts or
  every former test expression were copyrightable. Production drawing is
  unchanged and all 256 indexed colors are checked.

The executable embeds the project license, attribution and third-party notices
and prints them before accessing settings or journals. Debian packages also
install the documents and check them in CI. The offline notice check rejects
dependency changes or corrupted/incomplete recorded notices/source.

## AI artwork

The maintainer clarified that the supplied Myna artwork was generated using
ChatGPT/OpenAI, and permitted MIT distribution to the extent they hold rights.
It is described as AI-generated, without claiming human authorship or exclusive
copyright. [OpenAI's terms](https://openai.com/policies/row-terms-of-use/) assign
OpenAI's rights, if any, in output to the user, subject to applicable law; they
also leave responsibility for input rights and use of output with the user.
That assignment is not proof of originality or third-party clearance.

## Scope and follow-through

The reviewed current tree has no identified outstanding upstream-MIT notice
gap. Dependency and runtime licenses retain their separate terms. The objc2
project's own Apple SDK licensing qualification is retained in the notice
bundle; this review does not resolve that upstream legal ambiguity.

The initial corrections are included in MynaJournal 0.19.0. The dependency
snapshot update described below remains local until a subsequent release.
These reviews do **not** retroactively
change older tags, published source archives, workflow artifacts or existing
installations; old releases should not be described as having passed this review.
Following the maintainer's subsequent release request, 0.19.0 includes Mac and
Debian binaries with these notices, as well as source archives and workflow
artifacts.

Runtime coverage is version-specific: Rust 1.99.1 and Zig 0.15.2 where used.
See [the maintenance procedure](README.md) before redistributing builds made
with different toolchains, modified dependencies or additional native code.

## Dependency snapshot update — 28 October 2026

The locked inventory now contains 501 external packages: 91 package versions
were added or updated and 12 former versions were removed. Every changed cached
crate archive was checked against its SHA-256 in `Cargo.lock`. Notices and
source-header excerpts were collected from those verified archives, including
nested files. All 410 unchanged package records, the complete `option-ext`
source appendix, and the native-runtime files remain unchanged.

The update retains the following notices that package metadata or root licence
files alone do not fully capture:

- `lab`'s SIMD math includes the Zlib notices of Giovanni Garberoglio and
  Julien Pommier. `minimal-lexical` retains Go's BSD-3-Clause notice for its
  optional Bellerophon implementation, Sun's notices for its optional math
  routines, and the provenance of its fast-float-derived implementation.
- `termina` retains its README's Crossterm and TermWiz MIT grants and the
  Alice Maz/rust-base64 notice embedded in its base64 implementation. Additional
  source copyright notices are retained for `atomic`, `document-features`,
  `euclid`, `memmem`, `memoffset`, `pest` and related crates, among others.
- `termwiz` contains Unicode 16 tables, widecharwidth-derived code under CC0,
  and a transformed Nerd Fonts codepoint-name mapping under its source-code
  MIT grant. The original source headers, Unicode licence, widecharwidth
  dedication, CC0 legal text, and complete Nerd Fonts licensing file are kept.
  This mapping does not bundle font files. The font terms in the complete
  upstream licence remain verbatim; `deltae`'s published documentation does
  contain fonts, whose separate OFL notices are also retained conservatively.
- `compact_str` retains provenance for its Rust-derived code and Unicode
  tables; the native-runtime section already supplies Rust's full notices.
  `ucd-trie`'s generated Unicode test-data notice is included, even though that
  module is compiled only for that dependency's own tests. The `ratatui-widgets`
  no-std math polyfills retain their micromath attribution and MIT grant.
- `finl_unicode` declares `Unicode-DFS-2016` in its package metadata, but its
  published `LICENSE-UNICODE` is explicitly Unicode License V3. The declared
  metadata is preserved; the selected additional licence records `Unicode-3.0`.
  Both its licence's 1991–2023 copyright and its README's 1991–2025 copyright
  are retained without rewriting either. `wezterm-bidi`'s combined MIT and
  Unicode-DFS-2016 notice remains under those stated terms.

`ratatui-termina`, `vtparse` and `wezterm-input-types` omit their full licence
files from the published crates. Their upstream files were retrieved at the
exact commits recorded in each crate's `.cargo_vcs_info.json`, with immutable
source URLs and text hashes in the snapshot. The complete WezTerm root notice
mentions fonts shipped by WezTerm itself; those references do not mean that
these smaller crates or MynaJournal include the WezTerm font distribution.

Some input provenance cannot be reconstructed exactly from the published
sources. Termwiz's generator downloads Nerd Fonts' `master/glyphnames.json`
without recording its revision. Its widecharwidth header records generator
object hashes and Unicode input hashes, but not a licence revision. The
fast-float-rust and micromath adaptation references likewise omit the original
revision. Their licence files were retained from immutable upstream commits
resolved during this review, with the original adaptation references kept.
These are licence snapshots, not assertions that the exact original input
revision has been identified. No source code was changed or newly relicensed
as part of the notice update.

### Validation of this update

- All 91 changed crate archives match their locked checksums.
- The offline generator accepts all 501 package records, every notice hash,
  the complete MPL source appendix, and unchanged native-runtime hashes.
- All seven notice-integrity tests pass.
- The CLI regression verifies that the executable prints the complete embedded
  notices even with invalid configuration, without creating settings, logs,
  themes or journal files.

## Initial release validation — 27 October 2026

- 551 Rust tests and seven notice-integrity tests passed.
- Clippy with warnings denied, formatting, and all four individual-backend
  compile checks passed.
- Mac arm64 and static Linux amd64 release builds passed; both binaries contain
  the complete notice bundle, and the Mac command prints it byte-for-byte.
- Source-package contents include the license, attribution, notices and audit
  files. Workflow YAML parses, and the pinned Debian image exists for both
  amd64 and arm64. The Debian workflow now checks installed notice contents;
  that remote packaging job has not been run for these local changes.
