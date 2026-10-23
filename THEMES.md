# TUI-Journal Custom Themes

Themes control the app's colors and styles. Each theme is stored in its own TOML
file in the `themes` folder inside the app's configuration directory. The built-in
**Tui (default)** theme works without a file; its customizations are saved to
`themes/theme_tui.toml` only when you save changes or explicitly write defaults.

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
- [Example](#example)

## Getting started

The following commands work without launching the interface:

| Command | Behavior |
| --- | --- |
| `tjournal theme print-path` | Prints the themes directory without creating it. Alias: `path`. |
| `tjournal theme print-default` | Prints the original built-in Tui values, even if Tui has been customized or another theme is active. Alias: `default`. |
| `tjournal theme write-defaults` | Creates `themes/theme_tui.toml` containing the built-in defaults; refuses to overwrite an existing file and leaves the active theme unchanged. Alias: `write`. |

Use `--config /path/to/config-directory` before `theme` to use a custom
configuration directory. The directory itself must already exist. For example:

```sh
tjournal --config /path/to/config-directory theme print-path
```

## Selecting and editing themes

Press `!` in normal mode and select the **Appearance** tab. The theme list shows
**Tui (default)** first and marks the active theme. Use the arrow keys or `j` / `k`
to select a theme; `Page Up`, `Page Down`, `Home`, and `End` also navigate the list.

| Key | Action in the theme list |
| --- | --- |
| `Enter` or `a` | Activate the selected theme immediately and remember the selection. |
| `e` | Open the selected theme's full color and modifier editor. |
| `n` | Create a named copy of the selected theme. Enter a name and press `Enter` to open its unsaved editor. |

To start a new theme with Tui's current styles, select Tui and press `n`. Creating
the draft does not write a file. In the theme editor, `Enter` or `Space` edits a
color or modifier, `d` restores the selected property's built-in preset, and
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

The popup shows the selected theme's actual file location. Theme files follow the
pattern `APPDEFAULT/themes/theme_THEMENAME.toml`, where `APPDEFAULT` is the app's
configuration directory. For example, the theme ID `night` uses
`themes/theme_night.toml`.

Names accept ASCII letters, digits, spaces, `_`, and `-`, with at least one letter
or digit. Leading and trailing whitespace is removed, spaces become underscores,
and the filename ID is lowercase, up to 64 characters. For example, `Night Blue`
uses `theme_night_blue.toml`. Duplicate names and filenames are rejected without
regard to case. `Tui` and `Legacy` are reserved for their existing themes.

Theme files can include a display name, which preserves capitalization and spaces:

```toml
[theme]
name = "Night Blue"
```

The active theme ID is stored in `config.toml`:

```toml
theme = "night"
```

Starting the app, listing themes, activating built-in Tui, opening an editor, and
cancelling changes do not create theme files. Files are written only when you save
a new or modified theme or run `theme write-defaults`. Activating a theme saves
its selection in `config.toml`. Saving preserves comments and unrelated fields;
external changes are detected before writing so they are not silently overwritten.

If an existing `themes.toml` is present and no theme has been selected, it remains
active as **Legacy**. Its first saved edit creates `themes/theme_legacy.toml` and
leaves the original `themes.toml` intact. The saved named version is used for future
Legacy selections. Legacy cursor-style aliases remain supported. With the legacy
`--config FILE` form, themes continue to use the default configuration directory;
use `--config DIRECTORY` to place both configuration and themes together.

Missing or invalid selected themes produce a warning and use built-in Tui styles
for that session. The app does not overwrite the file or replace the saved
selection. Invalid theme files are identified in the list so they can be repaired.
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
