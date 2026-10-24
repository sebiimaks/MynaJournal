<!----------------------------------------------------------------------------->

[ci]:  https://github.com/AmmarAbouZor/tui-journal/workflows/CI/badge.svg
[ci-url]:  https://github.com/AmmarAbouZor/tui-journal/workflows/CI
[crates-io]:  https://img.shields.io/crates/v/tui-journal
[crates-io-url]:  https://crates.io/crates/tui-journal
[docs-rs]:  https://docs.rs/tui-journal/badge.svg
[docs-rs-url]:  https://docs.rs/tui-journal
[downloads]:  https://img.shields.io/crates/d/tui-journal
[lcns]:  https://img.shields.io/github/license/AmmarAbouZor/tui-journal
[lst]:  https://img.shields.io/github/last-commit/AmmarAbouZor/tui-journal
[repository]:  https://github.com/AmmarAbouZor/tui-journal

<!----------------------------------------------------------------------------->

# TUI-Journal

[![][ci]][ci-url]
[![][crates-io]][crates-io-url]
[![][docs-rs]][docs-rs-url]
[![][downloads]][crates-io-url]
[![][lst]][repository]
[![][lcns]][repository]

TUI-Journal is a terminal-based application written in Rust that allows you to write and manage your journal/notes from within the comfort of your terminal. It provides a simple and efficient interface for creating and organizing your thoughts, ideas, and reflections. This fork supports four local back-ends: JSON, SQLite, an iCalendar `VJOURNAL` directory, and individual plain-text or Markdown journal files.

Before opening a PR, please read the [Contribution and AI Policy](#contribution-and-ai-policy).

## Demo
<p align="center">
   <img src="assets/demo.gif">
</p>

<details>
  <summary><h3>Fuzzy Finder</h3></summary>

<p align="center">
   <img src="assets/fuzzy_find.gif">
</p>

</details>

## Table of Contents

- [Features](#features)
- [Roadmap](#roadmap)
- [Installation](#installation)
  - [Debian and Ubuntu](#debian-and-ubuntu)
  - [Arch Linux](#arch-linux)
  - [Alpine Linux](#alpine-linux)
  - [FreeBSD](#freebsd)
  - [NetBSD](#netbsd)
  - [Nix](#nix)
  - [Homebrew](@homebrew)
  - [Build & Install via Cargo](#build--install-via-cargo)
- [Usage](#usage)
- [Configuration](#configuration)
  - [File back-end](#file-back-end)
- [Themes](#themes)
- [Documentation](#documentation)
- [Contribution and AI Policy](#contribution-and-ai-policy)
- [Acknowledgments](#acknowledgments)
- [License](#license)

## Features

- Write and manage journal/notes entries from the terminal.
- Store your entries as JSON, in a SQLite database, as iCalendar `VJOURNAL` files, or as plain-text/Markdown files with YAML front matter.
- Intuitive, responsive and user-friendly text-based user interface (TUI).
- Create, edit, and delete entries easily.
- Edit journal content with the built-in editor or use your favourite terminal text editor from within the app.
- Add custom colored tags to the journals and use them in the built-in filter.
- Fuzzy Finder: Locate your desired journal with lightning-fast speed using smart-case search.
- Smart search functions for journals title and content in the built-in filter.
- Sort the journals based on their date, priority and title.
- Control many journals at once via the multi-select mode
- History management with Undo and Redo actions to easily revert or reapply changes in your entries
- Keybindings is a combination of VIM and Emacs motions (VIM for navigation and Emacs for editing texts in edit-mode).
- Utilize Editor's Visual Mode for VIM-style text selection, copying, and deletion.
- Export and Import journals between different back-end files.
- Export the current journal's content to a predefined export path or the current directory
- Transfer text between the built-in editor and the system clipboard using Cut, Copy, and Paste.
- Optionally sync the clipboard between the built-in editor and the operating system, with vim and emacs keybindings.
- Sorting and full-screen preferences in the App State will be retained.
- Easily cycle through tags in the main view with a single command \<Ctrl-t\>, applying the current filter for quick navigation.
- See the keybindings from inside the app
- Cross-platform compatibility (Windows, macOS, Linux, NetBSD).

## Roadmap

#### Back-ends:
- [x]  Plain text JSON back-end.
- [x]  Database back-end using SQLite.
- [x]  `VJOURNAL` directory back-end for use with external CalDAV sync clients.
- [x]  Individual plain-text or Markdown files with portable metadata.
#### Application:
- [x]  Edit journals content with external text editor from within the app.
- [x]  Filter & Search functionalities.
- [x]  Customize themes.
- [ ]  Preview mode for journals supporting Markdown highlighting and word wrapping.
- [ ]  Add mouse support
- [ ]  Improve app input and rending cycle using app events to support real concurrency within the app.

## Installation

This fork publishes versioned source code through its [latest release](https://github.com/sebiimaks/tui-journal-deb-plus/releases/latest). Debian packages are available as workflow artifacts, or you can build them locally using the instructions below. The other package-manager instructions below and the [upstream releases](https://github.com/AmmarAbouZor/tui-journal/releases) install the upstream application, whose features may differ from this fork.

### Debian and Ubuntu

Release pages contain **source code only**, with no attached binaries. The
[Debian workflow](https://github.com/sebiimaks/tui-journal-deb-plus/actions/workflows/deb.yml)
builds and installs packages for **amd64** and **arm64** on Debian 12 (Bookworm),
then saves the verified `.deb` files as workflow artifacts for **30 days**.
The packages target Debian 12 or newer and Ubuntu 24.04 or newer.

To install or upgrade from a workflow build:

1. Open the [source release](https://github.com/sebiimaks/tui-journal-deb-plus/releases/latest) and note its tag and commit.
2. Open [Build Debian packages](https://github.com/sebiimaks/tui-journal-deb-plus/actions/workflows/deb.yml) and select a successful run matching that release's commit.
3. Download `tui-journal-deb-amd64` or `tui-journal-deb-arm64`, matching the output of `dpkg --print-architecture`, and extract the ZIP file.

From the extracted directory, install the package. For this release:

```bash
version='0.18.0+debplus.4'
arch="$(dpkg --print-architecture)"
sudo apt install "./tui-journal_${version}-1_${arch}.deb"
tjournal --version
```

The package includes the JSON, SQLite, `VJOURNAL`, and File back-ends and installs
the `tjournal` command into `/usr/bin`. APT upgrades an existing `tui-journal`
Debian installation and installs any required runtime dependencies. It replaces
the application without removing your configuration, journals, or saved themes.
Close the app before upgrading, then restart it normally.

If the artifacts have expired, use **Run workflow** with the release tag to
rebuild them, or build locally. For a local build, install Rust 1.92.0 or newer,
then run:

```bash
sudo apt-get update
sudo apt-get install build-essential dpkg-dev git liblzma-dev pkg-config
version='0.18.0+debplus.4'
git clone --depth 1 --branch "v$version" \
  https://github.com/sebiimaks/tui-journal-deb-plus.git \
  "tui-journal-deb-plus-$version"
cd "tui-journal-deb-plus-$version"
cargo install cargo-deb --version '=3.8.0' --locked
cargo deb --locked
arch="$(dpkg --print-architecture)"
sudo apt install "./target/debian/tui-journal_${version}-1_${arch}.deb"
tjournal --version
```

The locally built package remains in `target/debian/`, with its version taken
from `Cargo.toml`.

Fork releases use `0.18.0+debplus.N`, incrementing `N` for each release based on
upstream 0.18.0. This release's Debian version is
`0.18.0+debplus.4-1`, which sorts after `0.18.0+debplus.3-1`, `0.18.0+debplus.2-1`,
`0.18.0+debplus.1-1`, and `0.18.0-1`. The final `-1` is the Debian packaging
revision. Future releases will retain increasing Debian versions so APT can
recognize upgrades.

This fork does not provide an APT repository: `apt update` and `apt upgrade`
alone cannot discover new GitHub releases. For each update, check the
[latest release](https://github.com/sebiimaks/tui-journal-deb-plus/releases/latest),
download the matching workflow artifact or build from its source tag, and
install that `.deb` file with APT. Set `version` in the examples to the release's
version without the leading `v`.

### Arch Linux

On Arch Linux, you can install TUI-Journal by using [pacman](https://wiki.archlinux.org/title/Pacman):

```bash
pacman -S tui-journal
```

### Alpine Linux

TUI-Journal is available for [Alpine Edge](https://pkgs.alpinelinux.org/packages?name=tui-journal&branch=edge). It can be installed via [apk](https://wiki.alpinelinux.org/wiki/Alpine_Package_Keeper) after enabling the [testing repository](https://wiki.alpinelinux.org/wiki/Repositories).

```bash
apk add tui-journal
```

### FreeBSD

On FreeBSD, you can install TUI-Journal by using [pkg](https://www.freshports.org/textproc/tui-journal):

```bash
pkg install -y tui-journal
```

### NetBSD

On NetBSD a pre-compiled binary built with default features is available from the official repositories. To install it, simply run:

```bash
pkgin install tui-journal
```

### Nix

TUI-Journal is available on the [Nix package manager](https://nixos.org/). You can install it using the following commands:

```bash
# On NixOS:
nix-env -iA nixos.tui-journal

# On Non NixOS:
nix-env -iA nixpkgs.tui-journal
```
For more information, visit the [TUI-Journal package page on Nix](https://search.nixos.org/packages?show=tui-journal).

### Homebrew (macOS and Linux)

For macOS and Linux users, TUI-Journal can be installed via Homebrew using a direct installation method:

```bash
brew install AmmarAbouZor/homebrew-tui-journal/tui-journal
```

### Build & Install via Cargo

Ensure you have [Rust](https://www.rust-lang.org/tools/install) installed on your system.

#### Install with default features:

To install TUI-Journal with the default JSON, SQLite, and `VJOURNAL` back-ends, you can use `cargo` to install directly from [crates.io](https://crates.io/crates/tui-journal):

```bash
cargo install tui-journal --locked
```

#### Install nightly version:
To use the current nightly version, you can install it directly from the GitHub repository

```bash
cargo install --git https://github.com/AmmarAbouZor/tui-journal
```

#### Install with Specific Features:

You can choose to install TUI-Journal with specific features enabled or disabled by customizing the cargo installation command. To install TUI-Journal with only the JSON back-end feature, use the following command:

```bash
cargo install tui-journal --locked --no-default-features --features json
```

To install TUI-Journal with only the SQLite back-end feature, use the following command:

```bash
cargo install tui-journal --locked --no-default-features --features sqlite
```

To install TUI-Journal with only the `VJOURNAL` back-end feature, use the following command:

```bash
cargo install tui-journal --locked --no-default-features --features vjournal
```

The File back-end is included in this fork's default source build. Published
upstream and package-manager versions may differ. From a checkout of this fork,
build all four back-ends with `cargo build --release --locked`, or only File with:

```bash
cargo build --release --locked --no-default-features --features file
```

## Usage

Once installed, you can run TUI-Journal by typing `tjournal` in your terminal:

```bash
$ tjournal
```

To view the available arguments and commands, you can use the `--help` or `-h` flag:

```bash
$ tjournal --help

Usage: tjournal [OPTIONS] [COMMAND]

Commands:
  print-config     Print the current settings including the paths for the backend files [aliases: pc]
  import-journals  Import journals from the given transfer JSON file to the current back-end file [aliases: imj]
  assign-priority  Assign priority for all the entries with empty priority field [aliases: ap]
  theme            Provides commands regarding changing themes and styles of the app [aliases: style]
  help             Print this message or the help of the given subcommand(s)

Options:
  -j, --json-file-path <FILE PATH>    Sets the entries Json file path and starts using it
  -s, --sqlite-file-path <FILE PATH>  Sets the entries sqlite file path and starts using it
      --vjournal-dir-path <DIR PATH>  Sets the entries VJOURNAL directory path and starts using it
      --file-dir-path <DIR PATH>     Sets the plain-text/Markdown journal directory path and starts using it
  -b, --backend-type <BACKEND_TYPE>   Sets the backend type and starts using it [possible values: json, sqlite, vjournal, file]
  -c, --config <DIR PATH>             Specifies the path for the configuration directory.
                                      The configuration directory is considered as the root for themes files too.
                                      It still accepts the path for configuration file for backward compatibility.
                                      (default path: <config-dir>/tui-journal/)
  -v, --verbose...                    Increases logging verbosity each use for up to 3 times
  -l, --log <FILE PATH>               Specifies a file to use for logging
                                      (default file: <cache_dir>/tui-journal/tui-journal.log)
  -h, --help                          Print help
  -V, --version                       Print version
```

Journal content soft-wraps while editing. Up/Down and Page Up/Page Down follow displayed
rows; selecting, copying, saving, and undoing still use the original text. Wrapping
does not insert line breaks. Set `editor_word_wrap = false` to use horizontal scrolling.

Use **Alt+,** to narrow the journals list and **Alt+.** to widen it, or drag the border
between the list and the editor with the left mouse button. The list starts at 30%
of the window and adjusts between 10% and 60%, with minimum pane widths on small
terminals. The divider position is remembered on exit. These controls also work
while editing, without moving the cursor or changing a draft. In terminals that
support it, hold Shift to use the terminal's own mouse selection instead of dragging.

## Configuration

The configuration for TUI-Journal can be found in the `config.toml` file located in the configuration folder within the TUI-Journal directory.

Press `!` in normal mode to open **Show config**, or use it while selecting multiple
journals. In text insertion mode `!` remains ordinary punctuation. The popup has
tabs for general preferences, new entries and exports, the editor, storage,
appearance, and the divider. Each option includes a description and its saved and
effective values. Command-line overrides and the configuration locations are
shown for reference.

Use `Tab` / `Shift-Tab` to change tabs, the arrow keys to select an option, and
`Enter` to edit it. Press `d` outside a text field to restore that option's default.
`Ctrl-S` saves the current configuration document: General, Entries, Editor and
Storage share `config.toml`; Appearance lists named themes and opens a separate
theme editor; Layout saves the divider to the existing app-state file. `Esc`
cancels a field edit or closes the popup, with a discard prompt when configuration
edits remain unsaved.

In Appearance, `Enter` or `a` activates the selected theme, `e` edits it, and `n`
prompts for the name of a new copy. Submit an empty name to use `tui_custom`, then
`tui_custom_01`, `tui_custom_02`, and so on. The editor retains every color and modifier control, with
`Ctrl-S` saving only that theme. `Esc` returns to the theme list, asking before
discarding unsaved theme edits. Saving an inactive theme does not activate it.
Theme selection is remembered separately in `config.toml` without saving unrelated
configuration drafts.

Display preferences apply after a successful save. Entry defaults, exports, and
external-editor preferences apply on the next relevant operation. Backend and
storage paths, the File format preference, the app-state directory, and the journal undo limit are marked
**restart required**; changing them preserves the current journal and undo history.
Changing storage paths does not move or copy entries. Relative paths retain their
existing meaning: they resolve from the application's launch directory.

Viewing settings does not create files. Saving preserves unrelated configuration,
comments, and backend sections, including those unavailable in the current build.
CLI backend/path overrides are not copied into saved preferences. If a file changes
outside the app while the popup is open, saving reports a conflict and keeps the
configuration draft; close and reopen the popup to load the external changes.
Journal drafts remain separate from configuration drafts throughout.

Here is a sample of the settings in the `config.toml` file:

```toml
backend_type = "Sqlite"   # Available options: Json, Sqlite, Vjournal, File. Default value: Sqlite.

theme = "tui"  # Active theme ID; reads themes/theme_tui.toml when present, otherwise built-in Tui defaults.
# Omit this key to retain an existing legacy themes.toml, or use Tui when none exists.

default_journal_priority = 3  # Sets the suggested priority while creating a new journal

auto_title = { kind = "date" } # Pre-populates the title field when creating a new entry. Two forms are accepted:
#   - Literal string: `auto_title = "Daily Note"` inserts the text as-is.
#   - Computed value: `auto_title = { kind = "date" }` inserts today's date as DD-MM-YYYY.
# Omit the key entirely (default) to leave the title field empty.
# Note: the string form is always literal. `auto_title = "date"` inserts the literal word "date",
# not today's date — use the struct form `{ kind = "date" }` for computed values.

scroll_per_page = 5  # Sets how many journals will be scrolled when using page up/down commands

sync_os_clipboard = false  # Syncs editor clipboard actions with operating system clipboard

editor_word_wrap = true  # Soft-wrap the editable journal content to the pane width without changing saved text.

history_limit = 10  # Sets the maximum changes limit for the undo & redo stacks. Use 0 to disable it.

colored_tags = true   # Sets if automatically coloring for tags is enabled.

# Sets the visibility option for the datum of journals. Available options:
#  - `show`: Render datum in journals list.
#  - `hide`: Hide datum without providing an extra empty line for journal without `priority` value.
#  - `empty_line`: Hide datum providing an extra empty line for journal without `priority` value.
datum_visibility = "show"

# Sets the directory where the application persists its state between sessions.
# Default are "~/<HOME>/.local/state/tui-journal/" on Linux and "C:\Users\Alice\AppData\Roaming\tui-journal\" on Windows
app_state_dir = "<STATE_DIRECTORY>/tui-journal/"

[export]
default_path = "<Absolute_path_to_export_directory>"   # Optional default path to export multiple journals or a single journal's content. Falls back to the current directory if not specified.
show_confirmation = true   # Show confirmation after successful export.

[external_editor]
# Set the external terminal editor to use from within the app.
# If the value isn't set the app will try to retrieve the editor from git global configurations then It'll try with the environment variables VISUAL, EDITOR then it'll fallback to vi.
command = "nvim"
# Enabling this save the journal content automatically after closing the external editor
auto_save = false
# Set the extension of the temporary file used with the external editor.
# This influences syntax highlighting in external editor (e.g., "md" for Markdown support).
temp_file_extension = "txt"

# Note: external_editor can still be configured in one line to set the command. In that case, the default values for the other fields will be used
# external_editor = "nvim"

[json_backend]
file_path = "<Documents-folder>/tui-journal/entries.json"

[sqlite_backend]
file_path = "<Documents-folder>/tui-journal/entries.db"

[vjournal_backend]
directory = "<Documents-folder>/tui-journal/journal"

[file_backend]
directory = "<Documents-folder>/tui-journal/entries"
format = "plain" # "plain" (no extension) or "markdown" (.md); applies to new files only.
```

### File back-end

The File back-end stores one UTF-8 file per entry. Select **File** under
`!` → **Storage**, set **File directory** and **File format**, save, and restart.
Alternatively, select a directory for one launch:

```bash
tjournal --file-dir-path /path/to/journals
```

Without a directory override, files live in `<Documents-folder>/tui-journal/entries`
(the home directory replaces Documents if unavailable). The normal default backend
remains SQLite. Custom builds choose SQLite, then JSON, then VJOURNAL, then File,
according to which features are compiled in. Switching backends or directories
does not move existing entries.

New filenames use local creation time in 24-hour `YYYYMMdd_HHmmss` form, such as
`20261023_194218`. The default `plain` format has no extension; `markdown` appends
`.md`. Collisions add `_2`, `_3`, and so on before the extension. Editing an entry's
title or journal date keeps its filename. Changing the format affects only new
files; both extensionless and `.md` journals are always read from the directory's
top level. The body is stored unchanged in either format.

Each file starts with YAML front matter. The editor shows only the body; the entry
dialog manages title, journal date, tags, and priority. For example:

```text
---
title: "Evening notes"
date: "2026-10-23T00:00:00Z"
created: "2026-10-23T19:42:18+10:00"
tags: ["personal", "walking"]
tui_journal:
  version: 1
  id: "8f86c7ae-c1a0-41ea-ad9e-86f3acd90fd3"
  priority: 2
---
Went for a walk after dinner.
```

`date` is the chosen journal date; `created` records actual creation time with its
UTC offset. `title`, `date`, `created`, and the version/UUID under `tui_journal` are
required; tags and priority may be omitted. The UUID is the permanent identity,
independent of the filename. The app's numeric entry IDs are session identifiers,
not values to copy into this header. Renamed files retain their identity on reload.
Copying a file into the same directory requires a new UUID to create a separate entry.
Titles must be nonempty, and both timestamps use RFC 3339 with a timezone.

Unknown JSON-compatible metadata values are preserved. Body-only saves also retain
header comments and formatting; editing metadata may reformat YAML and remove
comments. Headers are limited to 1 MiB and bounded nesting and item counts;
the same limits are checked before saving. Duplicate keys,
anchors, aliases, merge keys, and custom YAML tags are rejected. Arbitrary text and
unrelated files are not automatically adopted; malformed recognized journals and
duplicate UUIDs must be resolved before writes can continue.

This backend is local-only, with no network access or file watcher. Use **Reload all**
(`Ctrl-R` in normal mode) to pick up external edits; unsaved journal drafts prompt
before reloading. Updates and deletions check for external changes and report a
conflict instead of accepting a stale edit. A failed save keeps the editor draft.
Saves use flushed temporary files and atomic replacement; new entries never replace
an existing file. An exclusive directory lock coordinates app instances. External
editors do not share that lock, so avoid simultaneous writes to the same journal.
Journal-file symlinks are not supported.

To transfer entries from another backend, export a selection to the shared transfer
JSON format, then run:

```bash
tjournal --file-dir-path /path/to/journals import-journals --path exported.json
```

Transfer JSON preserves journal dates, titles, bodies, tags, and priorities. It does
not carry filenames, UUIDs, creation timestamps, or custom front matter; imported
entries receive new UUIDs and creation filenames. Copy the original journal files
to preserve their complete metadata. Exporting only a journal's content produces
the body without its metadata. Imports save entries individually; if an import
fails partway through, entries already saved remain in the directory.

### VJOURNAL back-end

The `VJOURNAL` back-end stores journal entries as iCalendar `.ics` files in the configured directory. To use it with CalDAV, point the directory to one managed by an external synchronization client. TUI-Journal does not connect to a CalDAV server or monitor external file changes while running, so restart it after the directory is changed externally.

## Themes

Press `!` and choose Appearance to activate, edit, or duplicate a theme. Every
installation includes **Tui (default)** as its only built-in theme. The first
interactive launch creates `themes/theme_tui.toml` under the app's configuration
directory. Later launches restore that file if missing and preserve existing
files and edits. Previously generated Dark and Light theme files remain usable
as custom themes; remove unwanted files from the themes directory manually.
An existing `themes.toml` remains available as **Legacy** and is kept intact when
its edits are saved to `themes/theme_legacy.toml`.

`tjournal theme print-path` prints the themes directory, `theme print-default`
prints the original built-in Tui styles, and `theme write-new Night Blue` creates
`theme_night_blue.toml` using those original defaults. All words after `write-new`
form the name. Without a name, it creates `theme_tui_custom.toml`, then
`theme_tui_custom_01.toml`, `theme_tui_custom_02.toml`, and so on. Existing files are
never overwritten, and creation does not change the active theme. These commands
do not create `theme_tui.toml`. See [Themes](THEMES.md) for naming, keyboard
controls, and the complete file format.

## Documentation

For detailed information about the TUI Journal app, including usage guide, keymaps, and configuration details, please refer to the [Wiki](https://github.com/AmmarAbouZor/tui-journal/wiki).


## Acknowledgments

TUI-Journal would not have been possible without the following open-source crates:

- [tui-rs](https://crates.io/crates/tui-rs) and its revival [ratatui](https://github.com/tui-rs-revival/ratatui): TUI-Journal utilizes the `tui-rs` and `ratatui` crates, which provide a framework for building terminal user interfaces in Rust. They offer a wide range of components and utilities to create interactive and responsive TUI applications.

- [tui-texteditor](https://crates.io/crates/tui-texteditor): TUI-Journal leverages the `tui-texteditor` crate, which offers a text editor widget specifically designed for terminal-based applications. It provides functionalities such as text manipulation, cursor movement, and scrolling within the TUI environment.

- [sqlx](https://crates.io/crates/sqlx): TUI-Journal benefits from the `sqlx` crate, which is a Rust library for interacting with databases. It enables seamless integration with SQLite, allowing TUI-Journal to store journal entries and notes in a reliable and efficient manner.

These crates have greatly contributed to the development of TUI-Journal, and the project extends its gratitude to the maintainers and contributors of these fantastic open-source libraries.

## Contribution and AI Policy

Thank you for considering contributing to TUI-Journal. Your support is highly appreciated!

In the age of powerful AI agents, we need to specify clear rules for contributions in this repository to save my time and yours:

* Always start by **opening an issue** before starting implementation. Please **write the issue yourself**. I would rather read an issue full of typos and mistakes than a wall of AI-generated text.
* I don’t mind accepting code generated by AI agents, I do that myself. But you **must review** the code carefully before submitting it. If I review your PR and spot code that is clearly generated by AI and not reviewed by a human developer, you will be **banned from contributing to this repository**. If reviewing code generated by your agents is not worth your time, **do not expect me to waste my time** reviewing it for you.

## License

[MIT](https://choosealicense.com/licenses/mit/)

