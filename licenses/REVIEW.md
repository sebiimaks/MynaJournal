# Licensing and attribution review — 27 October 2026

This review covers the current MynaJournal working tree, its inherited source
and assets, the 422 external packages in `Cargo.lock`, and the Mac/Debian/static
Linux distribution paths. It includes the uncommitted MynaJournal rename,
artwork, menu and Priority changes. It is a technical compliance review, not a
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
name the actual editor library, tui-textarea. Citation metadata distinguishes
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

These corrections are included in MynaJournal 0.19.0. They do **not** retroactively
change older tags, published source archives, workflow artifacts or existing
installations; old releases should not be described as having passed this review.
Following the maintainer's subsequent release request, 0.19.0 includes Mac and
Debian binaries with these notices, as well as source archives and workflow
artifacts.

Runtime coverage is version-specific: Rust 1.99.1 and Zig 0.15.2 where used.
See [the maintenance procedure](README.md) before redistributing builds made
with different toolchains, modified dependencies or additional native code.

## Validation

- 551 Rust tests and seven notice-integrity tests passed.
- Clippy with warnings denied, formatting, and all four individual-backend
  compile checks passed.
- Mac arm64 and static Linux amd64 release builds passed; both binaries contain
  the complete notice bundle, and the Mac command prints it byte-for-byte.
- Source-package contents include the license, attribution, notices and audit
  files. Workflow YAML parses, and the pinned Debian image exists for both
  amd64 and arm64. The Debian workflow now checks installed notice contents;
  that remote packaging job has not been run for these local changes.
