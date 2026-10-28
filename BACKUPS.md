# Local backups and recovery

Create a backup in a new directory:

```sh
mjournal backup create "$HOME/Backups/mynajournal-2026-10-28"
```

The parent directory must already exist. Close MynaJournal before backing up
JSON, File or VJOURNAL storage. Backups preserve the selected backend's native files,
configuration, themes, and application state. Protected notes remain encrypted;
unprotected notes remain plaintext. A backup is not itself password protected.
Keep it in a private location, preferably on encrypted storage.

Backend and `--config` overrides work as they do when opening the app:

```sh
mjournal --config /path/to/config --file-dir-path /path/to/entries \
  backup create /path/to/new-backup
```

SQLite uses a consistent database snapshot, including committed changes still in
its write-ahead log. Other stores use their existing advisory locks and verify
that source files did not change while copying. A changed source or busy store
causes an error; retry after closing the app. Backup never creates missing source
journal files or initializes an unused backend.

The backup contains a versioned `manifest.json` with SHA-256 checksums. Its
presence marks a completed backup. These checksums detect accidental damage;
they do not authenticate a backup supplied by someone else. Restore only backups
you trust. Symbolic links and nonregular files inside source trees are rejected.
Files and directories created by these commands use private Unix permissions.
The manifest has a 16 MiB limit, determined by the number and length of source
paths rather than the size of journal content. Backup creation refuses an
oversized manifest and removes its incomplete output, so it cannot report success
for a backup that restore would reject for this limit.

## Restore into a separate profile

```sh
mjournal backup restore /path/to/backup --to /path/to/new-profile
mjournal --config /path/to/new-profile/config
```

The destination must not already exist, and its parent must exist. Restore
verifies every file and refuses missing, extra, modified, or unsafe paths. It
does not replace the current installation, journals, or settings.

The restored profile contains `config`, `state`, and `data` directories. All four
backend paths and the state path in its configuration point into that new
profile, so switching backends does not reopen the original stores. Other
preferences, unknown configuration fields, comments, and native journal metadata
are retained. The backup also keeps the original configuration separately as
`config/original.toml`.

Restore works even if your currently configured app profile is damaged, because
it runs before loading that profile. Use a build supporting the backup's backend.
The paths written into the restored configuration are absolute; if you move the
restored profile afterwards, update its storage and state paths before opening it.
