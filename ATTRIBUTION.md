# Attribution and provenance

## TUI-Journal and contributors

MynaJournal is maintained by [sebiimaks](https://github.com/sebiimaks) and is a
fork of [TUI-Journal](https://github.com/AmmarAbouZor/tui-journal), created by
Ammar Abou Zor and developed with its contributors. The original copyright
notice and complete MIT license are retained unchanged in [LICENSE](LICENSE).
The project history retains the original contributors' commits and authorship.

[Samuel K. Norton / kotachisam](https://github.com/kotachisam/tui-journal)
contributed the `auto_title` option and editor navigation improvements inherited
through upstream. His MIT-licensed fork was also reviewed as a reference for
editable word wrap and a resizable divider. MynaJournal's implementations and
subsequent changes are recorded in this repository's history.

The upstream project and its contributors do not imply endorsement of this fork.

## Artwork

`assets/demo.gif` and `assets/fuzzy_find.gif` are inherited, unchanged, from
TUI-Journal and are covered by its retained MIT license.

The maintainer supplied the Myna artwork used in `assets/myna_journal.txt` and
the earlier Myna artwork variants, including `myna_notes.txt` and
`myna_round_88.txt`. On 27 October 2026, the maintainer clarified that the artwork
was AI-generated using ChatGPT/OpenAI and approved its distribution under the
project's MIT license to the extent they hold rights in it. This records its
provenance without claiming human authorship or exclusive copyright in generated
output.

## Libraries and adapted code

The terminal interface uses [Ratatui](https://github.com/ratatui/ratatui),
which continues the work of [tui-rs](https://github.com/fdehau/tui-rs), and
[tui-textarea](https://github.com/rhysd/tui-textarea). SQLite support uses
[SQLx](https://github.com/launchbadge/sqlx). These are examples, not the complete
dependency inventory.

The Linux-console drawing adapter in `src/console_backend.rs` adapts the
Ratatui Crossterm backend. Ratatui's copyright and MIT license are included in
[THIRD_PARTY_NOTICES.txt](THIRD_PARTY_NOTICES.txt), together with notices for the
other dependencies and native components covered by that inventory.
The editor's clipboard key mappings also follow tui-textarea; its MIT notice
is included in that file.

The former `string_or_struct` helper in `src/settings/mod.rs` was inherited
from TUI-Journal and copied from Serde's
["Deserialize either a string or a struct" example](https://serde.rs/string-or-struct.html).
The documentation repository uses
[CC BY-SA 4.0](https://github.com/serde-rs/serde-rs.github.io/blob/master/LICENSE-CC-BY-SA),
unlike the separately MIT/Apache-2.0-licensed Serde crate. During the 27 October
2026 review, the copied visitor and comments were removed and replaced with an
external-editor-specific, derived untagged enum in
`src/settings/external_editor.rs`. This is a record of the historical source;
it does not apply the documentation license to the replacement implementation.

## Distribution

Keep [LICENSE](LICENSE), this attribution file, and
[THIRD_PARTY_NOTICES.txt](THIRD_PARTY_NOTICES.txt) with redistributions.
The executable embeds these notices and displays them with `mjournal licenses`.
Debian packages also install them under `/usr/share/doc/myna-journal/`.
Dependency licenses apply to their respective components; the project's MIT
license does not replace those licenses.
