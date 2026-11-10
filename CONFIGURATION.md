# Configuration

Whole-journal protection is available under **Security → Journal protection** for
the open journal. Its state comes from the encrypted storage, rather than a
plain configuration flag. See [Journal locking](JOURNAL-LOCKING.md) for passwords,
automatic locking on close, and **Hide details: Ctrl-O**.

[Back to MynaJournal](README.md)

MynaJournal reads `config.toml` from its configuration directory. See the
[default locations](README.md#upgrading-from-tjournal), or use
`mjournal print-config` to inspect the effective settings and paths.

Press `!` in normal mode to open **Config**, or use it while selecting multiple
journals. In text insertion mode `!` remains ordinary punctuation. The popup has
sections for general preferences, new entries and exports, the editor, security,
storage, appearance, and the divider. Each option includes a description and its
saved and effective values. Command-line overrides and the configuration
locations are shown for reference.

Use `Tab` / `Shift-Tab` to change tabs, the arrow keys to select an option, and
`Enter` to edit it. Press `d` outside a text field to restore that option's default.
`Ctrl-S` saves the current configuration document: General, Entries, Editor,
Security and Storage share `config.toml`; Appearance lists named themes and opens a separate
theme editor; Layout saves the divider to the existing app-state file. `Esc`
cancels a field edit or closes the popup, with a discard prompt when configuration
edits remain unsaved.

The **Journal protection** and **Change journal password** actions in Security
apply immediately after their password and confirmation steps. They do not need
`Ctrl-S`, and `d` does not remove protection. Turning journal protection off uses
the same **Journal protection** row and requires confirmation plus the current password.

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

To relocate the open journal, use **Storage → Move journal**. Save or discard
entry and configuration edits first. Enter the absolute path of a **new folder**
whose parent already exists. Review the source and resulting storage path, then
press **Y** to confirm; **Esc** edits the destination and **N** cancels.

The journal closes before relocation. Its native data and any whole-journal
encryption header are copied together and verified before the saved path changes.
The storage keeps its current basename inside the new folder: for example, moving
`/old/entries` to a new folder `/new/Personal` uses `/new/Personal/entries`.
Encryption and passwords are preserved. Reopen the journal after the move;
protected journals ask for their password again.

The original data is retained in a private recovery folder beside its old location.
The result panel shows both paths and any recovery warning. Keep that copy until
you have checked the moved journal. Existing destinations are never overwritten,
and unrelated regular files in the journal folder are preserved. The move refuses
unsafe paths, unfinished encryption conversions and conflicting changes. Reopen
without command-line storage overrides before using this action.

Only the active backend storage moves. Configuration, themes, exports, app state,
and other backend locations remain in place, so the journal stays in its existing
selector collection. Moving an added journal's storage outside its managed profile
also means **Delete** in the selector cannot archive it automatically; it refuses
to leave external storage behind.

The individual path rows select which storage to open after restarting; an empty
location starts an empty, unprotected journal.
Use **Move journal** when you want the current entries and protection to follow.

In General, **Show entry time** toggles the time beside each entry's date, and
**Time format** selects `24h` (for example, `30.10.2026 22:09`) or `12h`
(`30.10.2026 10:09 pm`). Time is shown in 24-hour format by default. Save with
`Ctrl-S` to apply these preferences immediately. **Entry dates** hides both the
date and time in the entry list when set to `hide` or `empty_line`.

With **Show entry time** enabled, new and edited entry dialogs show a **Date and
Time** field, for example `01-11-2026 22:09` or `01-11-2026 10:09 pm`. You can edit
both values using the selected format. With time display off, the field shows
only **Date**. These preferences apply to the next entry dialog you open.

Dates and times are displayed in your computer's local timezone. New entries
retain the time when their dialog was opened. Editing metadata preserves the
full saved timestamp, including seconds, when the displayed date and time are
unchanged. Editing the date or time in **Date and Time** saves the entered local
minute with seconds set to zero. In date-only mode, choosing another calendar
date uses the first valid local time on that day (usually midnight). A date or
time skipped by a timezone change is rejected without losing the draft. A newly
entered time that occurs twice during a clock change uses the earlier occurrence;
an unchanged value keeps its original occurrence. Older entries may show a time
derived from midnight UTC because earlier versions saved only a calendar date.

Priority is hidden by default. In Entries, enable **Show priority field** to show it
in the entry dialog.
This applies to the next dialog you open. Existing priority values are preserved,
and the configured new-entry priority still applies when creating entries.

In **Entries**, **New-entry title mode** cycles through **None**, **Date** and
**Custom text**. None starts with a blank title field; Date fills it with today's
date as `DD-MM-YYYY`; Custom text fills it with the text in the **Custom text**
row. Editing that row selects Custom text mode. The text is remembered when you
switch to None or Date, including after saving with **Ctrl-S** and restarting.
These settings only prefill new entries: you can edit the suggested title, and
existing entries keep their titles. An empty default is allowed, but you must
enter a title before creating the entry.

Pressing **d** on the mode resets it to None without forgetting your text.
Pressing **d** on Custom text clears the remembered text and leaves the current
mode selected. These changes also need Ctrl-S to save.

In **Entries**, **Show mood** enables the optional Low, Average and High checkboxes
and the `[-]`, `[~]` and `[+]` markers in the list. Use `Tab` to focus Mood,
`Left`/`Right` to move between choices, and `Space` to select one. Pressing Space
on the selected choice clears it. A highlighted **Space** hint appears while the
checkboxes have focus. Existing entries start without a mood.

With **Journal prompts** enabled, new-entry dialogs also ask **Would you like a
journal prompt?** beside the mood question (stacked on narrow terminals).
**Yes** is selected by default for each new entry. Use Tab to focus the question,
Left/Right to focus Yes or No, and Space to select. No skips the prompt for that
entry without changing its mood or your preferences. A prompt requires a selected
mood; existing entries keep their original prompt when edited.

Choosing Yes creates one of the 120 bundled prompts matching the entry's mood.
The prompt appears in a centred box at the top inside the content
pane, occupying 60% of its inner width; **Ctrl-G**
hides or shows it while browsing or typing. The chosen prompt is saved with the
entry and remains unchanged when its mood changes. Reopening an entry shows its
saved prompt again. Editing an older entry does not create a prompt for it.

Both preferences default to on. Turning off Show mood also disables prompt
creation and display; it preserves saved moods, prompts and the Journal prompts
preference. Turning off Journal prompts alone preserves mood controls. These
changes apply immediately after saving. Prompts work entirely offline, are not
inserted into the note body, and are included in structured transfers and backups.
Protected entries encrypt mood and the saved prompt; both are hidden while locked.

## Separate journals

Normal startup opens **Select Journal** centred beneath the MynaJournal wordmark.
**Personal** uses your existing configuration and notes without moving them.
Use Up/Down and **Enter** to open a journal, **n** to create one, **d** to delete
an added journal, and **q** or **Esc** to exit. Inside a journal, **q** in normal
mode closes it and returns to the selector, with that journal highlighted.
Unsaved changes still offer
Save, Discard and Cancel; unresolved encrypted drafts must be saved or discarded
before leaving. Protected notes require unlocking again when reopened. The
entries pane shows the open journal's name. Smaller terminals show plain title
text, or just the selector when there is not enough room for a heading.

New journals inherit Personal's current preferences, including `auto_title`,
but receive separate backend files, exports and app state. Config edits apply to
the open journal. Theme files are shared by the collection; each journal can
select a different theme. Resetting a storage setting to its default keeps that
path inside the selected journal.

The collection is stored in `journals/` beside the original `config.toml`. Each
added journal has an internal identifier and a folder containing `journal.toml`
(its name and collection), `config.toml`, `data/`, `state/` and `exports/`. Names
do not become filesystem paths. When using a custom configuration file with a
different filename, the collection has a separate `journals-…` directory. Config's
Context tab shows the collection path after selecting a journal.

Deletion asks for confirmation and moves the entire added journal folder to
`journals/Deleted Journals/`; it does not erase its files. Personal cannot be
deleted through this selector. A journal open in another window, or configured
to use storage outside its managed folder, cannot be deleted here. To recover a
deleted journal, close MynaJournal and move its identifier folder from
`Deleted Journals/` back into the collection directory, keeping its name and
contents. If you have since created another journal with the same display name,
archive that one first. Reopen the app to see the recovered journal.

Explicit backend/path flags and command-line operations retain their existing
destinations and bypass the selector; quitting these direct sessions closes the
app. To target an added journal from
the command line, pass its profile directory to `--config`, for example:

```sh
mjournal --config "/path/to/journals/j_IDENTIFIER" backup create /path/to/backup
```

This backs up the selected profile, not the whole collection. Opening a managed
profile directly with `--config` skips the initial selector; closing that journal
returns to its collection's selector. Archives and backups
preserve existing note encryption; creating a separate journal does not encrypt
its unprotected entries.

## Settings reference

Viewing settings does not create files. Saving preserves unrelated configuration,
comments, and backend sections, including those unavailable in the current build.
CLI backend/path overrides are not copied into saved preferences. If a file changes
outside the app while the popup is open, saving reports a conflict and keeps the
configuration draft; close and reopen the popup to load the external changes.
Journal drafts remain separate from configuration drafts throughout.

This sample illustrates available settings rather than factory defaults. Replace
the placeholder paths before using it; omit `auto_title` to keep new titles blank.

```toml
backend_type = "Sqlite"   # Available options: Json, Sqlite, Vjournal, File. Default value: Sqlite.

theme = "tui"  # Active theme ID; reads themes/theme_tui.toml when present, otherwise built-in Tui defaults.
# Omit this key to retain an existing legacy themes.toml, or use Tui when none exists.

default_journal_priority = 3  # Sets the suggested priority while creating a new journal
show_entry_priority = false  # Hidden by default; true shows Priority without changing saved values.
show_entry_mood = true       # Optional Low / Average / High selector and list markers.
journal_prompts = true       # Saved prompts for new entries with a mood; requires show_entry_mood.

auto_title = { kind = "date" } # Pre-populates the title field when creating a new entry. Two forms are accepted:
#   - Custom text: `auto_title = "Daily Note"` inserts the text as-is.
#   - Computed value: `auto_title = { kind = "date" }` inserts today's date as DD-MM-YYYY.
# Omit the key entirely (default) to leave the title field empty.
# Note: the string form is always literal. `auto_title = "date"` inserts the literal word "date",
# not today's date — use the struct form `{ kind = "date" }` for computed values.
custom_title = "Daily Note" # Remembers custom text while None or Date is selected; does not activate it.
# If auto_title is a string, it takes precedence over custom_title. The config panel
# updates both when editing Custom text and remembers auto_title when changing modes.

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
show_entry_time = true   # Show the stored time in the entry list, using the local timezone.
entry_time_format = "24h" # "24h" (22:09) or "12h" (10:09 pm). Only affects display.

# Sets the directory where the application persists its state between sessions.
# Defaults: ~/.local/state/myna-journal/ on Debian; ~/Library/Application Support/myna-journal/ on macOS.
app_state_dir = "<STATE_DIRECTORY>/myna-journal/"

[security]
auto_lock_seconds = 300 # Automatically lock protected notes after inactivity; 0 disables the timer.
lock_on_leave = true    # Lock a protected note when selecting another entry.

[export]
default_path = "<Absolute_path_to_export_directory>"   # Existing directory, or end with '/' to create a directory. Other paths name a file. Falls back to the current directory if omitted.
show_confirmation = true   # Show confirmation after successful export. Replacing an existing file always requires confirmation.

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
file_path = "<Documents-folder>/myna-journal/entries.json"

[sqlite_backend]
file_path = "<Documents-folder>/myna-journal/entries.db"

[vjournal_backend]
directory = "<Documents-folder>/myna-journal/journal"

[file_backend]
directory = "<Documents-folder>/myna-journal/entries"
format = "plain" # "plain" (no extension) or "markdown" (.md); applies to new files only.
```

See [File storage](FILE-BACKEND.md), [Themes](THEMES.md), and
[Note locking](NOTE-LOCKING.md) for their detailed behavior.
