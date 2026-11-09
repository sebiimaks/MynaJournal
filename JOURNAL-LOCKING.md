# Journal locking

Open a journal, press `!`, and choose **Security → Journal protection**. Press
`Enter`, confirm with `Y`, then enter and confirm a new password. Protection belongs to that journal's storage;
new journals inherit preferences but start without a journal password.

The selector shows **Locked** at the right of a protected journal's row. Opening
it asks for its password before displaying entries. Pressing `q` from the journal
returns to the selector and closes its unlocked session. Normal Save, Discard and
Cancel choices still apply to unsaved edits. Exiting the process also discards the
session key. Saved entries remain encrypted while the journal is open.

Security also offers **Change journal password** while protection is enabled.
To remove protection, select **Journal protection** again and press `Enter`.
Both actions require the current password. Removing protection first asks for
confirmation because the native store becomes readable without a password.
There is no password recovery service.

Changing the password rewraps the existing journal key. It does not revoke old
key-header copies: someone with an earlier header and its password can still use
that journal key. Keep old headers and backups private, including after a password
change.

## Existing locked entries

Enabling journal protection explicitly confirms replacement of individual entry
passwords with the journal password. If necessary, setup asks for each locked
entry's password, showing its title. Cancelling does not change stored entries
and relocks entries opened during setup. Save or discard any
unsaved edits and encrypted drafts before starting this conversion.

## Hide details

Inside a protected journal, **Hide details: Ctrl-O** replaces Entry security.
Press `Ctrl-O` again to reveal the same editor, selection and dialog state.
The shortcut works in normal, insert and visual modes, multi-selection and popups.
Hidden views use generated scrambled text, not transformed copies of real text.
Entry titles, dates, tags, content, mood, prompts and open dialogs are concealed.
Editing, navigation, mouse actions and clipboard/export commands pause while
hidden. Quitting still offers generic Save, Discard and Cancel choices.

This is a reversible screen-concealment control; it does not require the password
to reveal details. Close the journal when you need password protection. `Ctrl-H`
and Backspace keep their existing navigation and deletion behavior.

## Storage and privacy

All four backends remain supported: JSON, SQLite, plain-text/Markdown files and
iCalendar VJOURNAL. Protected native records contain age-encrypted payloads under
a randomly generated journal key. The password protects that key using age's
scrypt password encryption. Entries include their full title, date, body, tags,
priority, mood, prompt and supported native metadata inside the encrypted payload.
An HMAC-SHA-256 check bound to the private journal key authenticates each record,
so knowing the public encryption recipient does not allow forged entries.
Native fields outside the payload are placeholders; file names no longer encode
the original entry date. Journal names, entry counts, filesystem sizes and file
modification times are not hidden.

The key header is stored beside the backend path with the suffix
`.myna-lock.json`. **Keep it with the journal data.** The native backup/restore
commands include it automatically, and moving a whole managed journal into
Deleted Journals retains it. An encrypted backup still needs the journal password.
Copying only a native JSON/database file or only the entry directory is insufficient.

Conversion stages and verifies the native replacement before publication. An
interrupted conversion is detected rather than opened as an ordinary store.
Reopening attempts recovery; preserve all associated files if recovery reports
damage. Mixed or unsupported external directory contents are rejected rather than
silently removed. Older app versions cannot read protected journal records.

Automatic system clipboard synchronization is disabled in protected journals.
Explicit clipboard copy/cut and plaintext exports ask for confirmation. External
editors are unavailable because they require a plaintext temporary file. Internal
editing, undo/redo, auto-title, word wrap and journal prompts remain available.

Protection cannot remove previously exported plaintext, older backups, filesystem
snapshots or recoverable disk blocks. Passwords and owned buffers are cleared where
the libraries permit, but protection does not promise resistance to process-memory
inspection, swap, terminal recording or a compromised operating system.
