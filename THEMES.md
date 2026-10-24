# TUI-Journal Custom Themes

Themes control the app's colors and styles. Every installation includes **Tui
(default)** and ten additional themes, embedded in the application so they work
on macOS, Linux, and Debian. The first interactive launch creates the `themes`
folder inside the app's configuration directory and writes all eleven TOML files.
Later launches restore missing bundled files while preserving existing files and
edits. Tui remains the default. Custom themes are saved in the same folder.

## Table of Contents

- [Getting Started](#getting-started)
- [Selecting and editing themes](#selecting-and-editing-themes)
- [Theme files and compatibility](#theme-files-and-compatibility)
- [Themes structures](#themes-structures)
  - [Themes Groups](#themes-groups)
  - [Themes Types](#themes-types)
    - [Color Type](#color-type)
    - [Style Type](#style-type)
  - [Tag colors](#tag-colors)
  - [Linux text console](#linux-text-console)
- [Example](#example)

## Getting started

The following commands work without launching the interface:

| Command | Behavior |
| --- | --- |
| `tjournal theme print-path` | Prints the themes directory without creating it. Alias: `path`. |
| `tjournal theme print-default` | Prints the original built-in Tui values, even if Tui has been customized or another theme is active. Alias: `default`. |
| `tjournal theme write-new [NAME...]` | Creates a new theme containing the original Tui defaults. All following words form its name; an empty name uses the automatic custom naming sequence. Existing files and the active theme are unchanged. Alias: `write`. |

`write-new` replaces the old `write-defaults` command. For example:

```sh
tjournal theme write-new Night Blue
# Creates themes/theme_night_blue.toml
tjournal theme write-new
# Creates themes/theme_tui_custom.toml, then theme_tui_custom_01.toml, etc.
```

Quoted and unquoted multiword names both work. All arguments after `write-new`
are treated as name text, including words beginning with `-`; place launch options
before `theme`. A standalone `--` immediately after `write-new` is the standard
argument delimiter and is not part of the name. Use `tjournal theme --help` for command help. These commands do not
initialize the full theme collection. `write-new` always uses the original Tui
styles, even when Tui's file has been customized.

Use `--config /path/to/config-directory` before `theme` to use a custom
configuration directory. The directory itself must already exist. For example:

```sh
tjournal --config /path/to/config-directory theme print-path
```

## Selecting and editing themes

Press `!` in normal mode and select the **Appearance** tab. The theme list shows
**Tui (default)** first and marks the active theme. Use the arrow keys or `j` / `k`
to select a theme; `Page Up`, `Page Down`, `Home`, and `End` also navigate the list.

The following themes are included, with their files created at interactive startup:

| Theme | Configuration ID |
| --- | --- |
| Tui (default) | `tui` |
| Dark Abyssal | `dark_abyssal` |
| Dark Borealis | `dark_borealis` |
| Dark Fernwood | `dark_fernwood` |
| Dark Kiln | `dark_kiln` |
| Dark Night Market | `dark_night_market` |
| Light Cyanotype | `light_cyanotype` |
| Light Lido | `light_lido` |
| Light Matcha | `light_matcha` |
| Light Riso | `light_riso` |
| Light Sorbet | `light_sorbet` |

| Key | Action in the theme list |
| --- | --- |
| `Enter` or `a` | Activate the selected theme immediately and remember the selection. |
| `e` | Open the selected theme's full color and modifier editor. |
| `n` | Copy the selected theme. Enter a name, or leave it blank for automatic naming, and press `Enter` to open its unsaved editor. `Esc` cancels. |

To start a new theme with Tui's current styles, select Tui and press `n`. Creating
the draft does not write a file. A blank name chooses `tui_custom`, then
`tui_custom_01`, `tui_custom_02`, and so on, using the first available filename.
This copies the selected theme's current styles; the command-line `write-new`
instead starts with the original Tui defaults. In the theme editor, `Enter` or `Space` edits a
color or modifier, `d` restores the selected property's original Tui preset, and
`Ctrl-S` saves that theme. All existing style controls remain available. Saving
the active theme updates the interface immediately; saving another theme does not
activate it. Journals are not reloaded.

`Esc` cancels a field edit or returns from the theme editor to the list. If the
theme has unsaved changes, confirm with `y` to discard them or `n` to keep editing.
`Tab` / `Shift-Tab` move between configuration sections while preserving your theme
draft. Theme saves and activation preserve unrelated unsaved configuration changes.

Color values accept names, `#RRGGBB`, and palette indices from 0 to 255. For optional
style colors, `inherit` removes that color; `Reset` explicitly uses the terminal's
default color. Modifier names can be combined with `|`. Editing one property of a
previously implicit style preserves its other effective properties. The
`tags.palette` row holds all tag colors in one line; see [Tag colors](#tag-colors).

## Theme files and compatibility

The popup shows where the selected theme's customizations are saved. Theme files
follow the pattern `APPDEFAULT/themes/theme_THEMENAME.toml`, where `APPDEFAULT` is
the app's configuration directory. For example, the theme ID `night` uses
`themes/theme_night.toml`.

A local file with a bundled theme's ID replaces that bundled theme completely;
it is not merged with the bundled values. For example,
`themes/theme_dark_abyssal.toml` takes precedence over bundled Dark Abyssal.
Editing a bundled theme in the popup starts with its full values, so saving one
change preserves its other colors and styles in the local file. Removing the
local file recreates the bundled version on the next interactive launch. An invalid local
file is reported as an error instead of silently using the bundled version.

Names accept ASCII letters, digits, spaces, `_`, and `-`, with at least one letter
or digit. Leading and trailing whitespace is removed, spaces become underscores,
and the filename ID is lowercase, up to 64 characters. For example, `Night Blue`
uses `theme_night_blue.toml`. Duplicate names and filenames are rejected without
regard to case. Bundled theme names and IDs, including `Tui`, are reserved, as is
`Legacy`. To customize one under a different name, select it and press `n` to
create a copy.

Theme files can include a display name, which preserves capitalization and spaces:

```toml
[theme]
name = "Night Blue"
```

The active theme ID is stored in `config.toml`:

```toml
theme = "night"
```

Interactive startup writes any missing bundled theme files without replacing
existing files. Opening a new-theme draft or cancelling it does not create a
custom theme file; save it with `Ctrl-S`. `theme write-new` writes its new file
immediately. Activating a theme saves its selection in `config.toml`.
Saving preserves comments and unrelated fields;
external changes are detected before writing so they are not silently overwritten.

If an existing `themes.toml` is present and no theme has been selected, it remains
active as **Legacy**. Its first saved edit creates `themes/theme_legacy.toml` and
leaves the original `themes.toml` intact. The saved named version is used for future
Legacy selections. Legacy cursor-style aliases remain supported. With the legacy
`--config FILE` form, themes continue to use the default configuration directory;
use `--config DIRECTORY` to place both configuration and themes together.

Missing custom themes and invalid selected themes produce a warning and use
built-in Tui styles for that session. A bundled theme whose local file is absent
uses its embedded values normally. The app does not overwrite files or replace
the saved selection. Invalid theme files are identified in the list so they can
be repaired.
Fallback mode does not mark any saved theme active, including a customized Tui
theme. Editing and saving Tui in this mode does not apply it; select Tui and
activate it explicitly to use those styles now and on future launches. An
unavailable themes directory shows an Appearance error while the other
configuration sections remain usable.

## Themes structures:

### Themes Groups:

Themes are divided into the following groups:
- **general**: Styles for general controls in the app, including journal pop-ups, filter and sorting pop-ups, and more.
- **journals_list**: Styles for the main list of journals. These styles are differentiated from the general ones since they are more important and contain more information than general list items.
- **editor**: Styles for the built-in editor.
- **msgbox**: Colors for message-box prompts (Questions, Errors, Warnings, etc.).
- **tags**: Colors of the tag chips in the journals list. See [Tag colors](#tag-colors).

### Themes Types:

The main types used to define themes are `Color` and `Style`.

#### Color Type

Represents the color value of an item or field. It can be defined as a terminal color, such as `Black` or `Reset`, or as an RGB value in hexadecimal format `#RRGGBB`.

#### Style Type

Represents a complete style definition with the following fields:
- **fg**: Foreground color.
- **bg**: Background color.
- **modifiers**: Modifiers change the way a piece of text is displayed. They are bitflags, so they can be easily combined.
  The available modifiers are: `BOLD | DIM | ITALIC | UNDERLINED | SLOW_BLINK | RAPID_BLINK | REVERSED | HIDDEN | CROSSED_OUT`.
- **underline_color**: The color of the underline parts if the `UNDERLINED` modifier is active.

Here is an example of a style with all elements defined:

```toml
# Example of a style with all possible elements
[example_style]
fg = "#0AFA96" # Foreground Color. Colors can be in hex-decimal format "#RRGGBB"
bg = "Black" # Background Color. Also it can be one of terminal colors.
# Modifiers with all available flags. Flags can be combined as in example.
modifiers = "BOLD | DIM | ITALIC | UNDERLINED | SLOW_BLINK | RAPID_BLINK | REVERSED | HIDDEN | CROSSED_OUT"
underline_color = "Magenta" # Color for underline element if activated  
```
It's worth mentioning that not all fields must be defined. Missing parts will be filled with their default values.

### Tag colors:

When `colored_tags = true` in `config.toml` (the default), each tag in the journals
list is drawn as a chip with its own colors, taken from the `palette` of the `tags`
group. Each entry is a pair of [colors](#color-type): `fg` for the text and `bg` for
the chip. A missing `fg` or `bg` uses `Reset`, the terminal's default color.

```toml
[tags]
palette = [
    { fg = "Black", bg = "LightCyan" },
    { fg = "#FFFFFF", bg = "#3A5F8F" },
    { fg = "Black", bg = "LightYellow" },
]
```

- Tags are sorted alphabetically and take the pairs in order: the first tag gets the
  first pair, the second tag the second pair, and so on. Tags beyond the end of the
  list use `Reset` for both colors. An empty `palette = []` draws every tag that way.
- While the app runs, a tag keeps its colors. A new tag reuses a pair freed by a
  removed tag, or takes the next unused pair. Restarting, or changing the palette of
  the active theme, assigns the pairs in order again.
- A palette replaces the built-in one completely. Without a `palette`, a theme uses
  the nine built-in pairs. `theme print-default` writes them as `[[tags.palette]]`
  tables, which mean the same as the list form above.
- The ` | ` separators between tags, and every tag when `colored_tags = false`, use
  `journals_list.tags_default` instead.

In the theme editor, the `tags.palette` row edits the whole palette as one line of
`FG on BG` pairs separated by commas, for example `Black on LightCyan, White on #3A5F8F`.
Saving the active theme, or activating another one, recolors the tags immediately.

### Linux text console:

The Linux text console (`TERM=linux`) has only 16 colors. Hex and palette colors
are reduced to them, and the eight light colors (`LightRed`, `DarkGray`, `White`, and
so on) are drawn by turning on the console's bold intensity. Backgrounds use only the
eight normal colors. Named colors give the most predictable result there. The console
has no italic, underline, or dim, and draws `ITALIC`, `UNDERLINED`, and `DIM` text in
a fixed substitute color instead. The app turns bold intensity off before text in the
`Reset` color, so plain text keeps the console's normal color with any theme.

## Example:

Here is a small example of overriding some of the themes. For a full list of all available style fields, please use the CLI subcommands to print the default themes.

```toml
[general]
list_item_selected = { fg = "LightYellow", modifiers = "BOLD" }
input_block_active = { fg = "Blue" }

[general.input_block_invalid]
fg = "Red"
modifiers = "BOLD | SLOW_BLINK"

[general.input_cursor_active]
fg = "Black"
bg = "LightYellow"
modifiers = "RAPID_BLINK"

[general.list_highlight_active]
fg = "Black"
bg = "LightGreen"
modifiers = "UNDERLINED"
underline_color = "Magenta"

[journals_list.block_inactive]
fg = "Grey"
modifiers = ""

[journals_list.highlight_active]
fg = "Red"
bg = "LightGreen"
modifiers = "BOLD | ITALIC"

[journals_list.highlight_inactive]
fg = "Grey"
bg = "LightBlue"
modifiers = "BOLD"

[journals_list.title_active]
fg = "Reset"
modifiers = "BOLD | UNDERLINED"

[editor.block_insert]
fg = "LightGreen"
modifiers = "BOLD"

[editor.cursor_insert]
fg = "Green"
bg = "LightGreen"
modifiers = "RAPID_BLINK"

[msgbox]
error = "#105577"
question = "Magenta"

[tags]
palette = [
    { fg = "Black", bg = "LightGreen" },
    { fg = "White", bg = "#3A5F8F" },
]
```
