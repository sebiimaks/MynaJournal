//! VJOURNAL-backed journal persistence using iCalendar files.

mod calendar;

use std::collections::{HashMap, HashSet};
use std::fs::{self, File, Metadata, OpenOptions, Permissions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context, anyhow, ensure};
use uuid::Uuid;
use vobject::Component;
use vobject::component::write_component;

use self::calendar::{
    LOCKED_COMPONENT, apply_entry_to_component, build_vcalendar, component_to_entry,
    date_at_utc_midnight, get_uid, is_journal, normalize_entry_priority, parse_vcalendar,
    restore_entry_to_component, set_uid,
};
use super::*;

/// Persists journal entries as iCalendar `VJOURNAL` components in a directory.
pub struct VjournalDataProvide {
    /// The path to the directory containing `VJOURNAL` components in `.ics` files (CalDAV).
    directory: PathBuf,
    /// Internal state.
    state: VjournalState,
    baselines: HashMap<PathBuf, Snapshot>,
    deleted: HashMap<u32, RestoreMetadata>,
}

#[derive(Clone)]
struct RestoreMetadata {
    path: PathBuf,
    uid: String,
    family: Vec<Component>,
    calendar_shell: Component,
    permissions: Permissions,
}

struct EntryLocation {
    /// The path to the file the entry is in.
    file_path: PathBuf,
    /// The UID of the entry within the file.
    ///
    /// This is to account for the case when a single `.ics` file contains multiple `VJOURNAL`
    /// components.
    uid: String,
}

/// Bookkeeping between the `VJOURNAL` entries in a store and `tui-journal` state.
///
/// File snapshots reject changes made by another editor until an explicit reload.
struct VjournalState {
    /// The next available `tui-journal` ID.
    next_id: u64,
    /// Map of the journal UID to `tui-journal` ID.
    uid_to_id: HashMap<String, u32>,
    /// Map of `tui-journal` ID to location within the store.
    id_to_location: HashMap<u32, EntryLocation>,
}

impl VjournalDataProvide {
    /// Creates a provider backed by `directory`.
    ///
    /// The directory is created on the first write if it does not exist.
    pub fn new(directory: PathBuf) -> Self {
        Self {
            directory,
            state: VjournalState::new(),
            baselines: HashMap::new(),
            deleted: HashMap::new(),
        }
    }
}

impl DataProvider for VjournalDataProvide {
    async fn load_all_entries(&mut self) -> anyhow::Result<Vec<Entry>> {
        if !self.directory.try_exists()? {
            return Ok(Vec::new());
        }

        let files = scan_ics_files(&self.directory).await?;
        let mut entries = Vec::new();
        let mut seen_uids = HashSet::new();
        let mut baselines = HashMap::new();
        let mut locations = HashMap::new();
        let mut families = HashMap::<String, (usize, bool)>::new();

        for file_path in &files {
            let snapshot = Snapshot::read(file_path)?;
            let vcal = match snapshot.calendar() {
                Ok(c) => c,
                Err(_) => {
                    log::warn!("Skipping invalid calendar {}", file_path.display());
                    continue;
                }
            };

            for sub in &vcal.subcomponents {
                if sub.name == LOCKED_COMPONENT {
                    ensure!(
                        get_uid(sub).is_some() && sub.get_all("RECURRENCE-ID").is_empty(),
                        "Invalid encrypted journal identity in {}",
                        file_path.display()
                    );
                }
                if is_journal(sub)
                    && let Some(uid) = get_uid(sub)
                {
                    let family = families.entry(uid).or_default();
                    family.0 += 1;
                    family.1 |= sub.name == LOCKED_COMPONENT;
                }
                if !is_journal(sub) || !sub.get_all("RECURRENCE-ID").is_empty() {
                    continue;
                }
                let uid = match get_uid(sub) {
                    Some(uid) => uid,
                    None => {
                        log::warn!("Skipping VJOURNAL without UID in {}", file_path.display());
                        continue;
                    }
                };
                if !seen_uids.insert(uid.clone()) {
                    log::warn!(
                        "Skipping duplicate VJOURNAL master {uid} in {}",
                        file_path.display()
                    );
                    continue;
                }

                let id = self.state.assign_id(&uid, file_path, None)?;
                match component_to_entry(sub, id) {
                    Ok(entry) => {
                        entries.push(entry);
                        locations.insert(
                            id,
                            EntryLocation {
                                file_path: file_path.clone(),
                                uid,
                            },
                        );
                    }
                    Err(_) => {
                        ensure!(
                            sub.name != LOCKED_COMPONENT,
                            "Invalid encrypted journal in {}",
                            file_path.display()
                        );
                        log::warn!("Skipping invalid VJOURNAL in {}", file_path.display());
                    }
                }
            }
            baselines.insert(file_path.clone(), snapshot);
        }
        ensure!(
            families
                .values()
                .all(|(count, protected)| !protected || *count == 1),
            "A protected journal has duplicate or unencrypted recurrence components; resolve the conflict before opening it"
        );
        self.baselines = baselines;
        self.state.id_to_location = locations;
        Ok(entries)
    }

    async fn add_entry(&mut self, entry: EntryDraft) -> Result<Entry, ModifyEntryError> {
        if entry.title.is_empty() {
            return Err(ModifyEntryError::ValidationError(
                "Entry title can't be empty".into(),
            ));
        }

        self.write_entry(entry, None).await
    }

    async fn restore_entry(&mut self, entry: Entry) -> Result<Entry, ModifyEntryError> {
        if entry.title.is_empty() {
            return Err(ModifyEntryError::ValidationError(
                "Entry title can't be empty".into(),
            ));
        }

        if self.state.id_to_location.contains_key(&entry.id) {
            let message = format!("Entry id {} already exists", entry.id);
            return Err(ModifyEntryError::ValidationError(message));
        }

        entry.validate_protection()?;
        if let Some(metadata) = self.deleted.get(&entry.id).cloned() {
            return self
                .restore_native_entry(entry, metadata)
                .await
                .map_err(Into::into);
        }
        if self.state.uid_to_id.values().any(|id| *id == entry.id) {
            return Err(anyhow!("Restore metadata is no longer available for this journal").into());
        }
        let id = Some(entry.id);
        self.write_entry(EntryDraft::from_entry(entry), id).await
    }

    async fn remove_entry(&mut self, entry_id: u32) -> anyhow::Result<()> {
        let loc = self
            .state
            .id_to_location
            .get(&entry_id)
            .ok_or_else(|| anyhow!("No entry with id {entry_id}"))?;

        let file_path = loc.file_path.clone();
        let uid = loc.uid.clone();

        let mut vcal = self.current_calendar(&file_path)?;
        let mut calendar_shell = vcal.clone();
        calendar_shell.subcomponents.clear();
        let family = vcal
            .subcomponents
            .iter()
            .filter(|component| {
                is_journal(component) && get_uid(component).as_deref() == Some(&uid)
            })
            .cloned()
            .collect();
        let metadata = RestoreMetadata {
            path: file_path.clone(),
            uid: uid.clone(),
            family,
            calendar_shell,
            permissions: self
                .baselines
                .get(&file_path)
                .context("Missing calendar baseline")?
                .identity
                .permissions
                .clone(),
        };

        vcal.subcomponents.retain(|component| {
            !is_journal(component)
                || get_uid(component).is_none_or(|component_uid| component_uid != uid.as_str())
        });

        if vcal.subcomponents.is_empty() {
            let _lock = write_lock(&file_path)?;
            check_destination(&file_path, self.baselines.get(&file_path))?;
            fs::remove_file(&file_path)?;
            self.baselines.remove(&file_path);
            sync_parent(&file_path);
        } else {
            self.save_calendar(&file_path, &vcal, true)?;
        }

        self.state.remove_id(entry_id);
        self.deleted.insert(entry_id, metadata);

        Ok(())
    }

    async fn update_entry(&mut self, entry: Entry) -> Result<Entry, ModifyEntryError> {
        self.update_entry_with_metadata(entry, None).await
    }

    async fn protection_metadata(&mut self, entry_id: u32) -> anyhow::Result<Option<String>> {
        let loc = self
            .state
            .id_to_location
            .get(&entry_id)
            .ok_or_else(|| anyhow!("No entry with id {entry_id}"))?;
        let vcal = self.current_calendar(&loc.file_path)?;
        validate_family_uids(&vcal)?;
        self.check_family_location(&loc.uid, &loc.file_path).await?;
        let family: Vec<_> = vcal
            .subcomponents
            .into_iter()
            .filter(|c| is_journal(c) && get_uid(c).as_deref() == Some(&loc.uid))
            .collect();
        ensure!(
            family.iter().all(|c| c.name == "VJOURNAL"),
            "Note is already protected"
        );
        ensure!(
            family
                .iter()
                .filter(|c| c.get_all("RECURRENCE-ID").is_empty())
                .count()
                == 1,
            "Cannot protect a journal with duplicate or missing recurrence masters"
        );
        let mut snapshot = build_vcalendar(family[0].clone());
        snapshot.subcomponents = family;
        Ok(Some(
            serde_json::json!({"format": "vjournal-v1", "document": write_component(&snapshot)})
                .to_string(),
        ))
    }

    async fn update_entry_with_metadata(
        &mut self,
        mut entry: Entry,
        metadata: Option<&str>,
    ) -> Result<Entry, ModifyEntryError> {
        if entry.title.is_empty() {
            return Err(ModifyEntryError::ValidationError(
                "Entry title can't be empty".into(),
            ));
        }
        entry.validate_protection()?;
        entry.priority = normalize_entry_priority(entry.priority)?;

        let loc = self.state.id_to_location.get(&entry.id).ok_or_else(|| {
            ModifyEntryError::ValidationError(format!("No entry with id {}", entry.id))
        })?;

        let file_path = loc.file_path.clone();
        let uid = loc.uid.clone();

        let mut vcal = self.current_calendar(&file_path)?;
        if entry.protection.is_some() {
            validate_family_uids(&vcal)?;
            self.check_family_location(&uid, &file_path).await?;
        }

        // Detached instances are hidden, so updates must target the recurrence master.
        let index = vcal
            .subcomponents
            .iter()
            .position(|c| {
                is_journal(c)
                    && get_uid(c).is_some_and(|component_uid| component_uid == uid.as_str())
                    && c.get_all("RECURRENCE-ID").is_empty()
            })
            .ok_or_else(|| {
                ModifyEntryError::DataError(anyhow!(
                    "VJOURNAL {uid} not found in {}",
                    file_path.display()
                ))
            })?;

        if entry.protection.is_none() {
            entry.date = date_at_utc_midnight(entry.date.date_naive());
        }
        let draft = EntryDraft::from_entry(entry.clone());
        let native = metadata
            .map(serde_json::from_str::<serde_json::Value>)
            .transpose()
            .context("Invalid native journal metadata")?;
        let native = native
            .as_ref()
            .filter(|value| value["format"] == "vjournal-v1");
        if entry.protection.is_some() {
            vcal.subcomponents
                .retain(|c| !is_journal(c) || get_uid(c).as_deref() != Some(&uid));
            vcal.subcomponents
                .push(apply_entry_to_component(&draft, &uid, None));
        } else if let Some(native) = native {
            let mut original = parse_vcalendar(
                native["document"]
                    .as_str()
                    .context("Invalid native calendar metadata")?,
            )?;
            let original_uid = original
                .subcomponents
                .first()
                .and_then(get_uid)
                .context("Missing native journal UID")?;
            if !original
                .subcomponents
                .iter()
                .all(|c| c.name == "VJOURNAL" && get_uid(c).as_deref() == Some(&original_uid))
            {
                return Err(anyhow!("Native metadata contains unrelated journals").into());
            }
            if original
                .subcomponents
                .iter()
                .filter(|c| c.get_all("RECURRENCE-ID").is_empty())
                .count()
                != 1
            {
                return Err(anyhow!("Native metadata has no unique recurrence master").into());
            }
            for component in &mut original.subcomponents {
                set_uid(component, &uid);
                if component.get_all("RECURRENCE-ID").is_empty() {
                    *component = restore_entry_to_component(&draft, &uid, component.clone())?;
                }
            }
            vcal.subcomponents
                .retain(|c| !is_journal(c) || get_uid(c).as_deref() != Some(&uid));
            vcal.subcomponents.extend(original.subcomponents);
        } else {
            let previous = (vcal.subcomponents[index].name == "VJOURNAL")
                .then(|| vcal.subcomponents[index].clone());
            vcal.subcomponents[index] = apply_entry_to_component(&draft, &uid, previous);
        }

        self.save_calendar(&file_path, &vcal, true)?;

        Ok(entry)
    }

    async fn get_export_object(&mut self, entries_ids: &[u32]) -> anyhow::Result<EntriesDTO> {
        if self.baselines.is_empty() && self.state.id_to_location.is_empty() {
            self.load_all_entries().await?;
        }
        let mut entries = Vec::new();
        for id in entries_ids {
            let location = self
                .state
                .id_to_location
                .get(id)
                .context("Entry no longer exists")?;
            let calendar = self.current_calendar(&location.file_path)?;
            let component = calendar
                .subcomponents
                .iter()
                .find(|component| {
                    is_journal(component)
                        && get_uid(component).as_deref() == Some(&location.uid)
                        && component.get_all("RECURRENCE-ID").is_empty()
                })
                .context("Journal no longer exists")?;
            entries.push(EntryDraft::from_entry(component_to_entry(component, *id)?));
        }

        Ok(EntriesDTO::new(entries))
    }

    async fn import_entries(&mut self, entries_dto: EntriesDTO) -> anyhow::Result<()> {
        entries_dto.validate()?;
        if entries_dto.entries.is_empty() {
            return Ok(());
        }
        let count = u64::try_from(entries_dto.entries.len())?;
        ensure!(
            self.state.next_id + count <= u64::from(u32::MAX) + 1,
            "No journal IDs remain"
        );
        let lock = write_lock(&self.directory.join("import.ics"))?;
        let canonical = fs::canonicalize(&self.directory)?;
        let directory = File::open(&canonical)?;
        let mut staged = Vec::new();
        let mut imported = Vec::new();
        for (offset, mut draft) in entries_dto.entries.into_iter().enumerate() {
            if draft.protection.is_none() {
                draft.date = date_at_utc_midnight(draft.date.date_naive());
            }
            draft.priority = normalize_entry_priority(draft.priority)?;
            let uid = generate_uid();
            let path = self.directory.join(format!("{uid}.ics"));
            let calendar = build_vcalendar(apply_entry_to_component(&draft, &uid, None));
            let bytes = write_component(&calendar).into_bytes();
            let file = crate::persistence::StagedFile::new(path.clone(), &bytes)?;
            let snapshot = Snapshot {
                bytes,
                identity: FileIdentity::new(&file.temporary.as_file().metadata()?),
            };
            staged.push(file);
            imported.push((
                uid,
                path,
                snapshot,
                (self.state.next_id + offset as u64) as u32,
            ));
        }
        crate::persistence::publish_new_files(staged, || {
            ensure!(
                fs::canonicalize(&self.directory)? == canonical,
                "Import directory changed"
            );
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                let current = fs::metadata(&canonical)?;
                let held = directory.metadata()?;
                ensure!(
                    current.dev() == held.dev() && current.ino() == held.ino(),
                    "Import directory was replaced"
                );
            }
            #[cfg(not(unix))]
            ensure!(
                fs::metadata(&canonical)?.created().ok() == directory.metadata()?.created().ok(),
                "Import directory was replaced"
            );
            let current_lock = fs::symlink_metadata(canonical.join(".tui-journal.lock"))?;
            ensure!(
                current_lock.is_file()
                    && FileIdentity::new(&current_lock) == FileIdentity::new(&lock.metadata()?),
                "Calendar lock was replaced"
            );
            Ok(())
        })?;
        for (uid, path, snapshot, id) in imported {
            self.state.assign_id(&uid, &path, Some(id))?;
            self.baselines.insert(path, snapshot);
        }
        sync_parent(&self.directory.join("import.ics"));
        Ok(())
    }

    fn retain_restore_metadata(&mut self, ids: &[u32]) {
        let ids: HashSet<_> = ids.iter().copied().collect();
        self.deleted.retain(|id, _| ids.contains(id));
    }

    async fn assign_priority_to_entries(&mut self, priority: u32) -> anyhow::Result<()> {
        let Some(priority) = normalize_entry_priority(Some(priority))? else {
            return Ok(());
        };

        let entries = self.load_all_entries().await?;

        for mut entry in entries {
            if entry.priority.is_none() && entry.protection.is_none() {
                entry.priority = Some(priority);
                self.update_entry(entry).await.map_err(|e| anyhow!("{e}"))?;
            }
        }

        Ok(())
    }
}

impl VjournalDataProvide {
    async fn restore_native_entry(
        &mut self,
        mut entry: Entry,
        metadata: RestoreMetadata,
    ) -> anyhow::Result<Entry> {
        for path in scan_ics_files(&self.directory).await? {
            if path == metadata.path {
                continue;
            }
            let calendar = Snapshot::read(&path)?.calendar()?;
            validate_family_uids(&calendar)?;
            for component in calendar.subcomponents.iter().filter(|component| {
                is_journal(component) && get_uid(component).as_deref() == Some(&metadata.uid)
            }) {
                ensure!(
                    entry.protection.is_none()
                        && component.name == "VJOURNAL"
                        && !component.get_all("RECURRENCE-ID").is_empty(),
                    "A conflicting journal family already exists; reload before restoring"
                );
            }
        }
        let mut calendar = if self.baselines.contains_key(&metadata.path) {
            self.current_calendar(&metadata.path)?
        } else {
            check_destination(&metadata.path, None)?;
            metadata.calendar_shell
        };
        ensure!(
            !calendar.subcomponents.iter().any(|component| {
                is_journal(component) && get_uid(component).as_deref() == Some(&metadata.uid)
            }),
            "Journal recurrence family already exists; reload before restoring"
        );
        let mut family = metadata.family;
        let master = family
            .iter_mut()
            .find(|component| component.get_all("RECURRENCE-ID").is_empty())
            .context("Deleted calendar has no recurrence master")?;
        let previous = component_to_entry(master, entry.id)?;
        ensure!(
            previous.protection.is_some() == entry.protection.is_some(),
            "Restore cannot change a note's protection state"
        );
        entry.priority = normalize_entry_priority(entry.priority)?;
        if entry.protection.is_none() {
            entry.date = date_at_utc_midnight(entry.date.date_naive());
        }
        if entry != previous {
            let draft = EntryDraft::from_entry(entry.clone());
            *master = if entry.protection.is_some() {
                apply_entry_to_component(&draft, &metadata.uid, None)
            } else {
                restore_entry_to_component(&draft, &metadata.uid, master.clone())?
            };
        }
        calendar.subcomponents.extend(family);
        let snapshot = write_vcalendar_file(
            &metadata.path,
            &calendar,
            self.baselines.get(&metadata.path),
            Some(&metadata.permissions),
        )?;
        self.baselines.insert(metadata.path.clone(), snapshot);
        self.state
            .assign_id(&metadata.uid, &metadata.path, Some(entry.id))?;
        self.deleted.remove(&entry.id);
        Ok(entry)
    }

    fn current_calendar(&self, path: &Path) -> anyhow::Result<Component> {
        let baseline = self
            .baselines
            .get(path)
            .context("Reload the calendar before writing")?;
        check_destination(path, Some(baseline))?;
        baseline.calendar()
    }

    fn save_calendar(
        &mut self,
        path: &Path,
        calendar: &Component,
        existing: bool,
    ) -> anyhow::Result<()> {
        let baseline = if existing {
            Some(
                self.baselines
                    .get(path)
                    .context("Reload the calendar before writing")?,
            )
        } else {
            None
        };
        let snapshot = write_vcalendar_file(path, calendar, baseline, None)?;
        self.baselines.insert(path.to_owned(), snapshot);
        Ok(())
    }

    async fn check_family_location(&self, uid: &str, own_path: &Path) -> anyhow::Result<()> {
        for path in scan_ics_files(&self.directory).await? {
            if path == own_path {
                continue;
            }
            let calendar = Snapshot::read(&path)?
                .calendar()
                .context("Cannot verify journal family while another calendar is invalid")?;
            validate_family_uids(&calendar)?;
            ensure!(
                !calendar
                    .subcomponents
                    .iter()
                    .any(|c| is_journal(c) && get_uid(c).as_deref() == Some(uid)),
                "Cannot protect a journal whose recurrence family spans multiple files"
            );
        }
        Ok(())
    }

    async fn write_entry(
        &mut self,
        mut entry: EntryDraft,
        id: Option<u32>,
    ) -> Result<Entry, ModifyEntryError> {
        Entry::from_draft(0, entry.clone()).validate_protection()?;
        if entry.protection.is_none() {
            entry.date = date_at_utc_midnight(entry.date.date_naive());
        }
        entry.priority = normalize_entry_priority(entry.priority)?;
        let uid = generate_uid();
        let vjournal = apply_entry_to_component(&entry, &uid, None);
        let vcal = build_vcalendar(vjournal);

        // Each new entry gets its own file, named after the UID.
        let file_name = format!("{uid}.ics");
        let file_path = self.directory.join(&file_name);

        if id.is_none() {
            u32::try_from(self.state.next_id).context("No journal IDs remain")?;
        }
        self.save_calendar(&file_path, &vcal, false)?;

        let id = self.state.assign_id(&uid, &file_path, id)?;

        Ok(Entry::from_draft(id, entry))
    }
}

/// Collect all `.ics` files in `dir`, sorted for deterministic ordering.
async fn scan_ics_files(dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut read_dir = tokio::fs::read_dir(dir)
        .await
        .with_context(|| format!("reading directory {}", dir.display()))?;

    let mut files = Vec::new();
    while let Some(entry) = read_dir
        .next_entry()
        .await
        .with_context(|| format!("iterating directory {}", dir.display()))?
    {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("ics") {
            let metadata = fs::symlink_metadata(&path)?;
            ensure!(
                !metadata.file_type().is_symlink(),
                "Calendar symlinks are not supported: {}",
                path.display()
            );
            if metadata.is_file() {
                files.push(path);
            }
        }
    }

    files.sort();
    Ok(files)
}

/// Serialise a VCALENDAR [`Component`] and write it to `path`, creating
/// parent directories as needed.
fn write_vcalendar_file(
    path: &Path,
    vcal: &Component,
    baseline: Option<&Snapshot>,
    restored_permissions: Option<&Permissions>,
) -> anyhow::Result<Snapshot> {
    let _lock = write_lock(path)?;
    check_destination(path, baseline)?;
    let content = write_component(vcal);
    let mut temporary = tempfile::Builder::new()
        .prefix(".tui-journal-")
        .suffix(".tmp")
        .tempfile_in(path.parent().context("Missing calendar directory")?)?;
    temporary.write_all(content.as_bytes())?;
    if let Some(permissions) = baseline
        .map(|snapshot| &snapshot.identity.permissions)
        .or(restored_permissions)
    {
        temporary.as_file().set_permissions(permissions.clone())?;
    }
    temporary.as_file().sync_all()?;
    let snapshot = Snapshot {
        bytes: content.into_bytes(),
        identity: FileIdentity::new(&temporary.as_file().metadata()?),
    };
    check_destination(path, baseline)?;
    if baseline.is_some() {
        temporary.persist(path)
    } else {
        temporary.persist_noclobber(path)
    }
    .context("Saving calendar atomically")?;
    sync_parent(path);
    Ok(snapshot)
}

#[derive(Clone, PartialEq, Eq)]
struct FileIdentity {
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    length: u64,
    modified: Option<SystemTime>,
    permissions: Permissions,
}

impl FileIdentity {
    fn new(metadata: &Metadata) -> Self {
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        Self {
            #[cfg(unix)]
            device: metadata.dev(),
            #[cfg(unix)]
            inode: metadata.ino(),
            length: metadata.len(),
            modified: metadata.modified().ok(),
            permissions: metadata.permissions(),
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
struct Snapshot {
    bytes: Vec<u8>,
    identity: FileIdentity,
}

impl Snapshot {
    fn read(path: &Path) -> anyhow::Result<Self> {
        let metadata = fs::symlink_metadata(path)?;
        ensure!(metadata.is_file(), "Calendar must be a regular file");
        let identity = FileIdentity::new(&metadata);
        let mut file = File::open(path)?;
        ensure!(
            identity == FileIdentity::new(&file.metadata()?),
            "Calendar changed while opening"
        );
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        let metadata = fs::symlink_metadata(path)?;
        ensure!(
            metadata.is_file()
                && identity == FileIdentity::new(&metadata)
                && identity == FileIdentity::new(&file.metadata()?),
            "Calendar changed while reading"
        );
        Ok(Self { bytes, identity })
    }

    fn calendar(&self) -> anyhow::Result<Component> {
        parse_vcalendar(std::str::from_utf8(&self.bytes)?)
            .map_err(|_| anyhow!("Invalid calendar document"))
    }
}

fn check_destination(path: &Path, baseline: Option<&Snapshot>) -> anyhow::Result<()> {
    if let Some(baseline) = baseline {
        ensure!(
            &Snapshot::read(path)? == baseline,
            "Calendar changed outside the app; reload before writing"
        );
    } else {
        match fs::symlink_metadata(path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
            Ok(_) => anyhow::bail!("Calendar destination already exists"),
        }
    }
    Ok(())
}

fn write_lock(path: &Path) -> anyhow::Result<File> {
    let parent = path.parent().context("Missing calendar directory")?;
    crate::persistence::create_private_dirs(parent)?;
    let path = parent.join(".tui-journal.lock");
    if let Ok(metadata) = fs::symlink_metadata(&path) {
        ensure!(metadata.is_file(), "Calendar lock must be a regular file");
    }
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(&path)?;
    file.try_lock()
        .map_err(|_| anyhow!("Another app is writing the calendar; retry the operation"))?;
    ensure!(
        FileIdentity::new(&file.metadata()?) == FileIdentity::new(&fs::symlink_metadata(&path)?),
        "Calendar lock was replaced"
    );
    Ok(file)
}

fn sync_parent(path: &Path) {
    #[cfg(unix)]
    if let Some(parent) = path.parent()
        && File::open(parent).and_then(|file| file.sync_all()).is_err()
    {
        log::warn!("Could not flush calendar directory after saving");
    }
}

fn generate_uid() -> String {
    Uuid::new_v4().to_string()
}

fn validate_family_uids(calendar: &Component) -> anyhow::Result<()> {
    ensure!(
        calendar
            .subcomponents
            .iter()
            .filter(|c| is_journal(c))
            .all(|c| get_uid(c).is_some()),
        "Cannot verify a calendar with missing or duplicate journal UID properties"
    );
    Ok(())
}

impl VjournalState {
    fn new() -> Self {
        Self {
            next_id: 0,
            uid_to_id: HashMap::new(),
            id_to_location: HashMap::new(),
        }
    }

    /// Return the existing id for `uid`, or assign a fresh one.  Either way
    /// the id-to-location mapping is updated to point at `file_path`.
    fn assign_id(
        &mut self,
        uid: &str,
        file_path: &Path,
        given_id: Option<u32>,
    ) -> anyhow::Result<u32> {
        let id = if let Some(id) = self.uid_to_id.get(uid) {
            *id
        } else {
            let id = given_id
                .map(Ok)
                .unwrap_or_else(|| u32::try_from(self.next_id).context("No journal IDs remain"))?;
            self.uid_to_id.insert(uid.to_owned(), id);
            id
        };
        self.next_id = self.next_id.max(u64::from(id) + 1);
        let location = EntryLocation {
            file_path: file_path.to_path_buf(),
            uid: uid.to_string(),
        };
        self.id_to_location.insert(id, location);
        Ok(id)
    }

    fn remove_id(&mut self, id: u32) {
        self.id_to_location.remove(&id);
    }
}
