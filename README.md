# MynaJournal

[![CI](https://github.com/sebiimaks/MynaJournal/actions/workflows/main.yml/badge.svg)](https://github.com/sebiimaks/MynaJournal/actions/workflows/main.yml)
[![Debian packages](https://github.com/sebiimaks/MynaJournal/actions/workflows/deb.yml/badge.svg)](https://github.com/sebiimaks/MynaJournal/actions/workflows/deb.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

MynaJournal is a terminal journal and notes app. Write with the built-in editor or
your own terminal editor, organize entries with tags, and keep your data in local
files or a database. Launch it with **`mjournal`**.

This is a fork of [TUI-Journal by Ammar Abou Zor](https://github.com/AmmarAbouZor/tui-journal).
It retains the original project's editor, search, filtering, sorting, undo/redo,
transfer tools and `auto_title` support, with the additions below. The upstream
project and its contributors are credited in [ATTRIBUTION.md](ATTRIBUTION.md).

**This fork has been tested on macOS and Debian only.** Hands-on testing covers
Apple Silicon Macs and Debian 12, including the Linux text console (TTY).
Debian packages have automated installation checks for amd64 and arm64.
Windows, other Linux distributions, and Intel Macs have not been validated for
this fork; upstream platform support should not be read as a testing claim here.

## What differs from TUI-Journal?

These are the main additions and changes in MynaJournal relative to its upstream
base, rather than a claim about every future upstream version:

| Area | MynaJournal changes |
| --- | --- |
| Editing | Editable soft word wrap, with cursor movement and selection following displayed rows while saved text keeps its original line breaks. |
| Layout | A resizable divider, controlled by keyboard, mouse dragging or configuration, with its position remembered. |
| Configuration | An in-app `!` panel for settings, storage paths, editor preferences, security, themes and layout. |
| Themes | A named theme list with activation, editing and creation. Only **Tui (default)** is bundled. |
| Storage | A fourth backend: one plain-text or Markdown file per entry, with YAML metadata and date/time filenames. JSON, SQLite and VJOURNAL remain available. |
| Privacy | Password protection for individual entries across all four backends. Titles and dates stay visible; private contents are encrypted. |
| Entry dialogs | An option to hide the Priority field without removing existing values. New titles are blank unless `auto_title` is configured. |
| Interface | Myna artwork at startup and in empty note panes, clearer footer commands, an **Entries** heading, and period-separated list dates. |
| Linux console | Rendering, startup and exit fixes for Debian TTY sessions. |
| Distribution | The `mjournal` command, `myna-journal` default directories, Debian packages, and accessible bundled license notices. |

MynaJournal does not add network synchronization. All four backends operate on
local storage; an external program may manage a directory independently.

## Installation

Download binaries and `SHA256SUMS` from the
[latest release](https://github.com/sebiimaks/MynaJournal/releases/latest).
Release **0.19.0** provides a macOS Apple Silicon archive and Debian amd64/arm64
packages, alongside GitHub's source archives. Every binary includes all four
backends. The examples below use that version; use matching filenames from the
release you download.

If you already use TUI-Journal or an earlier `tjournal` build of this fork, read
[Upgrading from tjournal](#upgrading-from-tjournal) before first launch.

### macOS — Apple Silicon

Download `myna-journal_0.19.0_macos-arm64.tar.gz` and `SHA256SUMS` into the same
directory. In Terminal, change to that directory and verify the archive:

```sh
grep -F 'myna-journal_0.19.0_macos-arm64.tar.gz' SHA256SUMS | shasum -a 256 -c -
```

Close any running MynaJournal instance, then extract and install it for your user:

```sh
mkdir -p myna-journal-0.19.0
tar -xzf myna-journal_0.19.0_macos-arm64.tar.gz -C myna-journal-0.19.0
mkdir -p "$HOME/.local/bin" "$HOME/.local/share/doc/myna-journal"
install -m 755 myna-journal-0.19.0/mjournal "$HOME/.local/bin/mjournal"
cp myna-journal-0.19.0/*.md myna-journal-0.19.0/LICENSE \
  myna-journal-0.19.0/THIRD_PARTY_NOTICES.txt "$HOME/.local/share/doc/myna-journal/"
export PATH="$HOME/.local/bin:$PATH"
mjournal --version
```

Add `export PATH="$HOME/.local/bin:$PATH"` to `~/.zshrc` if that directory is
not already on your PATH. The release binary does not require Homebrew libraries.
This project does not provide a MynaJournal Homebrew formula; the upstream
TUI-Journal formula installs the upstream app. You can keep that installation while trying MynaJournal, but
do not run both against the same journal store.

For later MynaJournal updates, repeat the binary installation with the new
archive. Replacing the executable does not remove your journals or settings.

### Debian 12

Check your architecture:

```sh
dpkg --print-architecture
```

Download the matching `.deb` and `SHA256SUMS`: for example,
`myna-journal_0.19.0-1_amd64.deb` for amd64 or
`myna-journal_0.19.0-1_arm64.deb` for arm64. From the download directory:

```sh
sha256sum --ignore-missing --check SHA256SUMS
sudo apt install ./myna-journal_0.19.0-1_amd64.deb
mjournal --version
```

Use the arm64 filename in the install command when appropriate. APT installs
runtime dependencies and the `/usr/bin/mjournal` command. The package replaces
the old `tui-journal` package and its `tjournal` command; it does not move or
delete journals or settings in your home directory.

To update, download and install the next release's matching package in the same
way. This project does not provide an APT repository, so `apt update` and
`apt upgrade` alone will not discover new GitHub releases.

Development `.deb` builds are also available as artifacts of successful
[Debian workflow runs](https://github.com/sebiimaks/MynaJournal/actions/workflows/deb.yml).
Those artifacts expire after 30 days; tagged releases provide the release downloads.

## Upgrading from tjournal

The command changed from `tjournal` to `mjournal`, and default application
directories changed from `tui-journal` to `myna-journal`. **The app does not
automatically migrate those directories or merge two stores.**

1. Close the old app and back up its journals and configuration.
2. Copy each existing old default directory to the corresponding new location
   below, **only if the destination does not already exist**. Retain the old
   copies until you have checked the new installation. If both locations exist,
   choose which store to use instead of merging or overwriting them.
3. If your configuration contains explicit paths into an old default directory,
   update those paths to the new copies. Custom locations may remain unchanged.
4. Run `mjournal print-config` to inspect the effective paths, then launch
   `mjournal` and check your entries and settings.

| Contents | Debian default | macOS default |
| --- | --- | --- |
| Journal data | `<Documents>/myna-journal` | `~/Documents/myna-journal` |
| Settings and themes | `~/.config/myna-journal` | `~/Library/Application Support/myna-journal` |
| Saved pane/sort state | `~/.local/state/myna-journal` | `~/Library/Application Support/myna-journal` |
| Logs | `~/.cache/myna-journal` | `~/Library/Caches/myna-journal` |

The old location for each row uses `tui-journal` instead of `myna-journal`.
On macOS, settings and state share one directory, so one copy covers both.
Debian honors XDG directory overrides and the configured Documents folder; if
Documents is unavailable, the data default is `~/myna-journal`. Older Linux
installations may have `state.json` under `~/.local/share/tui-journal`; copy
that file to the new state directory only if there is no newer state file.
Logs are optional and may remain in the old cache directory.

Alternatively, keep the original data location and select it explicitly with
the configuration or backend path options described below. `--config` selects
settings only: it does not redirect unset backend paths to old journal data.

Stored formats and encryption remain compatible with the previous fork build.
Internal identifiers such as `tui_journal` and `X-TUI-JOURNAL-LOCKED` are retained
for compatibility; do not rename them inside journal files. Protected stores
require a note-locking-capable version of this fork.

## Getting started

```sh
mjournal
```

1. Press `n` in the Entries pane to create an entry. Type a title, use `Tab` to
   move through its date, optional priority and comma-separated tags, then press
   `Enter` to confirm.
2. Press `Enter` on the entry to edit its content. Type your note and press
   `Ctrl-S` to save it.
3. Press `Esc` to return to normal mode. Use `Tab` to move between panes.
4. Press `?` for the full help menu, `!` for configuration, or `q` to quit.
   The app prompts when an action would discard unsaved content.

The startup artwork disappears on your first action. It also appears when the
note pane is empty and has enough room to display it.

These shortcuts apply in normal mode, with the Entries pane focused where an
entry-list action is involved. In insertion mode, letters are ordinary text.

| Action | Key |
| --- | --- |
| Select previous/next entry | `Up` / `Down`, or `k` / `j` |
| Create an entry | `n` |
| Edit title, date, tags or priority | `e` |
| Edit content | `Enter` or `Ctrl-M` |
| Save content | `Ctrl-S` while editing; `s` in normal mode |
| Use an external terminal editor | `y` from the list, or `Ctrl-Y` in normal mode |
| Open / reset the filter | `f` / `x` |
| Fuzzy find | `a` or `Shift-F` |
| Sort options | `o` |
| Entry security | `L` |
| Export this entry's body | `>` |
| Select multiple entries | `v`, then `Space` to select; `>` exports the selection |
| Reload storage after external changes | `Ctrl-R` in normal mode |
| Toggle the full-screen pane | `Ctrl-F` in normal mode |
| Help / configuration / quit | `?` / `!` / `q` |

Content soft-wraps by default without inserting line breaks. Up/Down and page
navigation follow displayed rows; selections, copies and saved text keep the
original content. Disable **Word wrap** in Config → Editor to use horizontal
scrolling instead.

Use `Alt+,` to narrow the Entries pane and `Alt+.` to widen it, or drag the
divider with the left mouse button. Config → Layout provides the same setting.
The position is remembered on exit. Terminals may reserve some Alt/mouse
combinations; the configuration control remains available.

## Choosing storage

SQLite is the default. Choose a backend under `!` → **Storage**, save with
`Ctrl-S`, and restart when prompted. Changing a backend or path does not move
existing entries.

| Backend | Storage | Select for one launch |
| --- | --- | --- |
| SQLite | One `entries.db` database | `mjournal --sqlite-file-path /path/to/entries.db` |
| JSON | One `entries.json` file | `mjournal --json-file-path /path/to/entries.json` |
| File | One plain-text or `.md` file per entry | `mjournal --file-dir-path /path/to/entries` |
| VJOURNAL | iCalendar `.ics` files | `mjournal --vjournal-dir-path /path/to/journal` |

The File backend defaults to extensionless files named with local creation time,
for example `20261027_194218`. Select **Markdown** for new `.md` files. Each entry
has YAML front matter for its title, date, tags and identity; the editor shows
only the body. See [File backend](FILE-BACKEND.md) for the format and safeguards.

VJOURNAL storage can be placed in a directory managed by an external CalDAV
client. MynaJournal itself does not connect to a server or watch that directory.

To transfer entries between backends, use `v` to select entries and `>` to export
transfer JSON, then import it into the destination:

```sh
mjournal --file-dir-path /path/to/destination import-journals --path exported.json
```

Transfer JSON preserves common entry fields and protected-note ciphertext.
Single-entry body exports contain only the body; they are not a complete backup.
Keep copies of the actual backend files when you need all native metadata.

## Configuration, themes and entry protection

Press `!` to open configuration. Use `Tab` / `Shift-Tab` to change sections,
arrow keys to select an option, `Enter` to edit, and `Ctrl-S` to save. Settings
marked **restart required** apply on the next launch. See
[Configuration](CONFIGURATION.md) for the full reference and example TOML.

Under **Entries**, turn off **Show priority field** to hide Priority in the entry
dialog. Existing priorities and the configured default for new entries are
preserved. Titles start blank by default; `auto_title = "Daily Note"` supplies
literal text, while `auto_title = { kind = "date" }` supplies the current date.

Under **Appearance**, select a theme and press `Enter` or `a` to activate it,
`e` to edit it, or `n` to create a named copy. Only **Tui (default)** ships with
the app. The first interactive launch writes `themes/theme_tui.toml` beneath the
configuration directory and preserves existing files. The CLI also provides:

```sh
mjournal theme print-path
mjournal theme print-default
mjournal theme write-new Night Blue
```

Omit the name to create `theme_tui_custom.toml`, then `theme_tui_custom_01.toml`,
`theme_tui_custom_02.toml`, and so on. See [Themes](THEMES.md).

Press `L` to protect an entry with a password. Its title and date remain visible;
its body, tags, priority and private metadata are encrypted. Press `Enter` on a
locked entry to unlock it. There is no password recovery. Automatic locking,
clipboard restrictions and export behavior are described in
[Note locking](NOTE-LOCKING.md).

Useful terminal commands:

```sh
mjournal --help
mjournal print-config
mjournal --config /path/to/config-directory
mjournal licenses
```

## Building from source

Install a C compiler and [Rust through rustup](https://rustup.rs/). On macOS,
Apple's Command Line Tools provide the compiler (`xcode-select --install`). On
Debian, install `build-essential`, `git` and `pkg-config`.

Rust 1.92.0 is the minimum supported version. Release builds use **1.99.1**, which
matches the reviewed native-runtime notices:

```sh
rustup toolchain install 1.99.1
git clone --branch v0.19.0 --depth 1 https://github.com/sebiimaks/MynaJournal.git
cd MynaJournal
cargo +1.99.1 install --path . --locked
mjournal --version
```

Cargo normally installs the executable under `~/.cargo/bin`; ensure that
directory is on PATH. For a build without installation, run
`cargo +1.99.1 build --release --locked`; the binary is `target/release/mjournal`.
To compile one backend only, add `--no-default-features --features file`, using
`json`, `sqlite` or `vjournal` instead of `file` as needed.

On Debian, build a local package with:

```sh
sudo apt install dpkg-dev liblzma-dev
cargo +1.99.1 install cargo-deb --version '=3.8.0' --locked
cargo +1.99.1 deb --locked
sudo apt install ./target/debian/myna-journal_*.deb
```

For development, the main checks are `cargo test --locked`,
`cargo clippy --locked --all-targets --all-features -- -D warnings`, and
`cargo fmt --all -- --check`. Python 3.11 or newer runs the offline license check:
`python3 scripts/generate-notices.py --check`. See [license maintenance](licenses/README.md)
before distributing builds with changed dependencies or toolchains.

## Contributions and attribution

Open an issue to discuss a proposed change before implementing it. Review and
test contributions, including AI-assisted work, before submitting a pull request.
Contribution guidelines do not add conditions to the MIT license.

Thanks to Ammar Abou Zor and the TUI-Journal contributors, including Samuel K.
Norton's inherited `auto_title` and editor improvements. The interface uses
[Ratatui](https://github.com/ratatui/ratatui) and
[tui-textarea](https://github.com/rhysd/tui-textarea); SQLite support uses
[SQLx](https://github.com/launchbadge/sqlx). See [Attribution](ATTRIBUTION.md) for
the full provenance record, including the AI-generated Myna artwork.

MynaJournal retains TUI-Journal's [MIT license](LICENSE) and original copyright
notice. Dependencies retain their respective licenses. Keep `LICENSE`,
`ATTRIBUTION.md` and [THIRD_PARTY_NOTICES.txt](THIRD_PARTY_NOTICES.txt) with
redistributions. The executable embeds these documents (`mjournal licenses`),
and Debian packages install them under `/usr/share/doc/myna-journal/`.
