# Note locking

Select an unprotected note and press **L** in normal mode to open the password
field. Type a password, press **Enter**, then type it again and press **Enter**
to protect the note. Passwords appear as asterisks in the focused field.
The current editor text is
included, including unsaved changes. There is no master password, recovery
service, network connection, or saved password file.

The title and date remain visible. The body, tags, priority, and private native
metadata are encrypted. Use a strong passphrase: a forgotten password cannot be
recovered. Protecting a note does not change the `auto_title` setting for new
notes.

## Opening and editing

Press **Enter** on a locked note to enter its password. Press Enter again to
edit its body. While open, the note still has encrypted storage; saving encrypts
the new text before updating the backend. Title/date changes also require an
unlocked note because these fields are authenticated by the encrypted payload.

Press **L** to lock an open note, change its password, or remove protection.
Save or discard pending edits before changing a password or removing protection.
Removing protection explicitly stores plaintext again. A failed write keeps the
previous stored note and does not apply a new password.

Protected notes are marked `[Locked]` or `[Open]`. Their bodies, tags, and
priorities are excluded from list searches, tag suggestions, and bulk priority
assignment, even while open. Titles remain searchable. App-wide undo history
for a note is cleared when it is protected or locked; editor undo is available
while it is open. Deleting a saved protected note can be undone as ciphertext.

## Automatic locking

The **Security** section in the `!` configuration popup controls these settings:

```toml
[security]
auto_lock_seconds = 300
lock_on_leave = true
```

By default, moving to another note locks the previous protected note. Ordinary
save/discard prompts still apply. Set `lock_on_leave = false` to keep several
notes open during the session. Inactivity locks all open protected notes; set
`auto_lock_seconds = 0` to disable the timer. The timer checks elapsed wall time
as well as monotonic time so returning from sleep does not reveal an expired
session before checking the lock.

If automatic locking interrupts unsaved text or an unfinished entry metadata
form, it encrypts that draft **in memory**
and removes the password and open session. Unlocking restores the draft for you
to save or discard. The app prevents normal quitting, reloading, or deleting the
note until such drafts are resolved. An app crash, forced exit, or power loss
still loses unsaved drafts. If sealing a draft fails, the screen remains hidden
and the draft stays in memory until you authenticate and resolve it.

## Clipboard, editors, and exports

Automatic synchronization to the system clipboard is disabled for protected
notes. Explicit system clipboard copy/cut asks for confirmation; clipboard
history can retain copied text after locking. Internal copy/paste works while
the note is open and is discarded when it locks. External editors are blocked
for protected notes because their temporary files, backups, and swap files may
contain plaintext.

Exporting one protected note's body requires unlocking and confirming plaintext
export. Multi-note and CLI transfer exports retain ciphertext and do not require
passwords. Transfers between all four backends preserve protection. Private
metadata from the original file format travels inside the encrypted payload;
it is restored when protection is removed in that same backend. Other backends
restore the common title/date/body/tags/priority fields.

## Storage and compatibility

Protection uses the maintained Rust `age` implementation, ASCII-armored age
passphrase encryption, fresh random keys/salts, and scrypt with work factor 18
(approximately 256 MiB per password operation). The versioned encrypted payload
also authenticates a random note identity and its public title/date. Backend
numeric IDs may change during transfer without invalidating protection.

- **JSON:** the store becomes a version-2 object on its first protected note.
  Writes use an exclusive advisory lock and atomic file replacement.
- **SQLite:** a migrated protection column holds ciphertext; private plaintext
  columns and tag rows are cleared in the same transaction. Secure deletion and
  durable rollback journaling are enabled.
- **Files:** the same filename and extension remain, with minimal public
  version-2 front matter and armored ciphertext. Private/unknown original
  front matter is encrypted and restored when protection is removed.
- **VJOURNAL:** protected notes use a distinct `X-TUI-JOURNAL-LOCKED` component.
  The same-UID recurrence family is encrypted together. Ambiguous or cross-file
  families are rejected before writes; unrelated calendar components remain.

Protected transfer records use a new versioned shape that older importers reject.
Use this version or newer with protected stores; keep backups before migration.
Encrypted notes and their private bodies are limited to 32 MiB ciphertext and
16 MiB serialized plaintext respectively, including native metadata.

Locking protects current stored contents from someone without the password.
It cannot erase earlier backups, filesystem snapshots, Git history, editor
files, clipboard history, or copies already exported as plaintext. Secure
deletion is not a guarantee against filesystem or SSD recovery. Passwords and
owned sensitive buffers are cleared where supported, but a running application,
terminal recording, memory dump, or OS swap can still expose plaintext while a
note is open. Full-disk encryption remains useful alongside note locking.
