# File backend

[Back to MynaJournal](README.md)

The File back-end stores one UTF-8 file per entry. Select **File** under
`!` → **Storage**, set **File directory** and **File format**, save, and restart.
Alternatively, select a directory for one launch:

```bash
mjournal --file-dir-path /path/to/journals
```

Without a directory override, files live in `<Documents-folder>/myna-journal/entries`
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
mjournal --file-dir-path /path/to/journals import-journals --path exported.json
```

Transfer JSON preserves journal dates, titles, bodies, tags, and priorities. It does
not carry filenames, UUIDs, creation timestamps, or custom front matter; imported
entries receive new UUIDs and creation filenames. Copy the original journal files
to preserve their complete metadata. Exporting only a journal's content produces
the body without its metadata. Imports save entries individually; if an import
fails partway through, entries already saved remain in the directory.
