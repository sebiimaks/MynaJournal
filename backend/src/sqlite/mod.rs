use std::{collections::HashMap, fs::OpenOptions, path::PathBuf, str::FromStr};

use self::sqlite_helper::EntryIntermediate;

use super::*;
use anyhow::{Context, anyhow, ensure};
use path_absolutize::Absolutize;
use sqlx::{
    Row, Sqlite, SqliteConnection, SqlitePool,
    migrate::MigrateDatabase,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};
use zeroize::Zeroize;

mod sqlite_helper;

const SELECT_ENTRIES: &str = r"SELECT entries.id, entries.title, entries.date, entries.content,
    entries.priority, entries.protection,
    json_group_array(tags.tag) FILTER (WHERE tags.tag IS NOT NULL) AS tags
    FROM entries LEFT JOIN tags ON entries.id = tags.entry_id";

pub struct SqliteDataProvide {
    pool: SqlitePool,
    snapshots: HashMap<u32, Entry>,
}

impl SqliteDataProvide {
    pub async fn from_file(file_path: PathBuf) -> anyhow::Result<Self> {
        let file_full_path = file_path
            .absolutize()
            .with_context(|| format!("Failed to resolve database path: {}", file_path.display()))?;
        ensure!(
            file_full_path.to_str().is_some(),
            "SQLite filenames must be valid UTF-8"
        );
        let options = SqliteConnectOptions::new().filename(file_full_path.as_ref());
        Self::connect(options, true).await
    }

    pub async fn create(db_url: &str) -> anyhow::Result<Self> {
        let options = SqliteConnectOptions::from_str(db_url)
            .with_context(|| format!("Failed to parse database URL: {db_url}"))?;
        let create_file = !Sqlite::database_exists(db_url).await?;
        Self::connect(options, create_file).await
    }

    async fn connect(options: SqliteConnectOptions, create_file: bool) -> anyhow::Result<Self> {
        let filename = options.get_filename();
        if create_file && filename != std::path::Path::new(":memory:") {
            if let Some(parent) = filename
                .parent()
                .filter(|path| !path.as_os_str().is_empty())
            {
                crate::persistence::create_private_dirs(parent).with_context(|| {
                    format!("Failed to create database directory: {}", parent.display())
                })?;
            }
            let mut open = OpenOptions::new();
            open.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                open.mode(0o600);
            }
            match open.open(filename) {
                Ok(file) => file.sync_all()?,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error).context("Creating private database file"),
            }
        }
        let options = options
            .journal_mode(SqliteJournalMode::Delete)
            .synchronous(SqliteSynchronous::Full)
            .foreign_keys(true)
            .pragma("secure_delete", "ON");
        let pool = SqlitePoolOptions::new()
            .connect_with(options)
            .await
            .context("Failed to connect to database")?;
        sqlx::migrate!("backend/src/sqlite/migrations")
            .run(&pool)
            .await
            .map_err(|err| match err {
                sqlx::migrate::MigrateError::VersionMissing(id) => anyhow!("Database version mismatch: migration {id} was previously applied but is missing in the resolved migrations"),
                err => anyhow!(err),
            })
            .context("Failed to apply migrations on database")?;
        Ok(Self {
            pool,
            snapshots: HashMap::new(),
        })
    }

    async fn insert_entry(
        connection: &mut SqliteConnection,
        entry: &mut Entry,
        protection: Option<String>,
    ) -> anyhow::Result<()> {
        let row = sqlx::query(
            "INSERT INTO entries (title, date, content, priority, protection) VALUES ($1, $2, $3, $4, $5) RETURNING id",
        ).bind(&entry.title).bind(entry.date).bind(&entry.content).bind(entry.priority)
            .bind(protection).fetch_one(&mut *connection).await.context("Failed to add entry")?;
        let id: i64 = row.try_get(0).context("Reading the new entry ID")?;
        entry.id = u32::try_from(id).context("No entry IDs remain")?;
        Self::insert_tags(connection, entry.id, &entry.tags).await?;
        Ok(())
    }

    async fn insert_tags(
        connection: &mut SqliteConnection,
        entry_id: u32,
        tags: &[String],
    ) -> anyhow::Result<()> {
        for tag in tags {
            sqlx::query("INSERT INTO tags (entry_id, tag) VALUES ($1, $2)")
                .bind(entry_id)
                .bind(tag)
                .execute(&mut *connection)
                .await
                .with_context(|| format!("Failed to add tag to entry {entry_id}"))?;
        }
        Ok(())
    }

    async fn read_entries(
        connection: &mut SqliteConnection,
        id: Option<u32>,
    ) -> anyhow::Result<Vec<Entry>> {
        let sql = format!(
            "{SELECT_ENTRIES} WHERE ($1 IS NULL OR entries.id = $1) GROUP BY entries.id ORDER BY date DESC"
        );
        let entries: Vec<EntryIntermediate> = sqlx::query_as(&sql)
            .bind(id)
            .fetch_all(connection)
            .await
            .context("Failed to load entries from database")?;
        entries.into_iter().map(Entry::try_from).collect()
    }

    fn remember_entry(&mut self, entry: Entry) {
        if let Some(previous) = self.snapshots.insert(entry.id, entry) {
            wipe_snapshot(previous);
        }
    }

    fn check_snapshot(&self, entry_id: u32, current: Option<&Entry>) -> anyhow::Result<()> {
        if let Some(snapshot) = self.snapshots.get(&entry_id) {
            ensure!(
                current.is_some_and(|entry| {
                    entry.id == snapshot.id
                        && entry.date == snapshot.date
                        && entry.title == snapshot.title
                        && entry.content == snapshot.content
                        && entry.priority == snapshot.priority
                        && entry.protection == snapshot.protection
                        && entry.tags.len() == snapshot.tags.len()
                        && entry.tags.iter().all(|tag| snapshot.tags.contains(tag))
                }),
                "Entry {entry_id} changed outside the app; reload before writing"
            );
        }
        Ok(())
    }
}

impl Drop for SqliteDataProvide {
    fn drop(&mut self) {
        for (_, entry) in self.snapshots.drain() {
            wipe_snapshot(entry);
        }
    }
}

fn wipe_snapshot(mut entry: Entry) {
    entry.content.zeroize();
    entry.tags.zeroize();
    entry.priority.zeroize();
}

impl DataProvider for SqliteDataProvide {
    async fn load_all_entries(&mut self) -> anyhow::Result<Vec<Entry>> {
        let mut connection = self.pool.acquire().await?;
        let entries = Self::read_entries(&mut connection, None).await?;
        for (_, entry) in self.snapshots.drain() {
            wipe_snapshot(entry);
        }
        self.snapshots = entries
            .iter()
            .map(|entry| (entry.id, entry.clone()))
            .collect();
        Ok(entries)
    }

    async fn add_entry(&mut self, entry: EntryDraft) -> Result<Entry, ModifyEntryError> {
        let mut entry = Entry::from_draft(0, entry);
        validate_entry(&entry)?;
        let protection = entry
            .protection
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .context("Serializing note protection")?;
        let mut transaction = self
            .pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .context("Starting entry transaction")?;
        Self::insert_entry(&mut transaction, &mut entry, protection).await?;
        transaction.commit().await.context("Committing new entry")?;
        self.remember_entry(entry.clone());
        Ok(entry)
    }

    async fn restore_entry(&mut self, entry: Entry) -> Result<Entry, ModifyEntryError> {
        validate_entry(&entry)?;
        let protection = entry
            .protection
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .context("Serializing note protection")?;
        let mut transaction = self
            .pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .context("Starting restore transaction")?;
        sqlx::query(
            "INSERT INTO entries (id, title, date, content, priority, protection) VALUES ($1, $2, $3, $4, $5, $6)",
        ).bind(entry.id).bind(&entry.title).bind(entry.date).bind(&entry.content)
            .bind(entry.priority).bind(protection).execute(&mut *transaction).await
            .with_context(|| format!("Failed to restore entry {}", entry.id))?;
        Self::insert_tags(&mut transaction, entry.id, &entry.tags).await?;
        transaction
            .commit()
            .await
            .context("Committing restored entry")?;
        self.remember_entry(entry.clone());
        Ok(entry)
    }

    async fn remove_entry(&mut self, entry_id: u32) -> anyhow::Result<()> {
        let mut transaction = self
            .pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .context("Starting delete transaction")?;
        let current = Self::read_entries(&mut transaction, Some(entry_id)).await?;
        self.check_snapshot(entry_id, current.first())?;
        sqlx::query("DELETE FROM entries WHERE id = $1")
            .bind(entry_id)
            .execute(&mut *transaction)
            .await
            .with_context(|| format!("Failed to delete entry {entry_id}"))?;
        transaction
            .commit()
            .await
            .context("Committing entry deletion")?;
        if let Some(entry) = self.snapshots.remove(&entry_id) {
            wipe_snapshot(entry);
        }
        Ok(())
    }

    async fn update_entry(&mut self, entry: Entry) -> Result<Entry, ModifyEntryError> {
        validate_entry(&entry)?;
        let protection = entry
            .protection
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .context("Serializing note protection")?;
        let mut transaction = self
            .pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .context("Starting update transaction")?;
        let current = Self::read_entries(&mut transaction, Some(entry.id)).await?;
        self.check_snapshot(entry.id, current.first())?;
        if current.len() != 1 {
            return Err(ModifyEntryError::ValidationError(format!(
                "Entry {} does not exist",
                entry.id
            )));
        }
        sqlx::query("DELETE FROM tags WHERE entry_id = $1")
            .bind(entry.id)
            .execute(&mut *transaction)
            .await
            .context("Replacing entry tags")?;
        sqlx::query(
            "UPDATE entries SET title = $1, date = $2, content = $3, priority = $4, protection = $5 WHERE id = $6",
        ).bind(&entry.title).bind(entry.date).bind(&entry.content).bind(entry.priority)
            .bind(protection).bind(entry.id).execute(&mut *transaction).await
            .with_context(|| format!("Failed to update entry {}", entry.id))?;
        Self::insert_tags(&mut transaction, entry.id, &entry.tags).await?;
        transaction
            .commit()
            .await
            .context("Committing updated entry")?;
        self.remember_entry(entry.clone());
        Ok(entry)
    }

    async fn get_export_object(&mut self, entries_ids: &[u32]) -> anyhow::Result<EntriesDTO> {
        let mut connection = self.pool.acquire().await?;
        let entries = Self::read_entries(&mut connection, None)
            .await?
            .into_iter()
            .filter(|entry| entries_ids.contains(&entry.id))
            .map(EntryDraft::from_entry)
            .collect();
        Ok(EntriesDTO::new(entries))
    }

    async fn import_entries(&mut self, entries_dto: EntriesDTO) -> anyhow::Result<()> {
        entries_dto.validate()?;
        if entries_dto.entries.is_empty() {
            return Ok(());
        }
        let mut transaction = self
            .pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .context("Starting import transaction")?;
        let mut imported = Vec::new();
        for draft in entries_dto.entries {
            let mut entry = Entry::from_draft(0, draft);
            validate_entry(&entry)?;
            let protection = entry
                .protection
                .as_ref()
                .map(serde_json::to_string)
                .transpose()?;
            Self::insert_entry(&mut transaction, &mut entry, protection).await?;
            imported.push(entry);
        }
        transaction
            .commit()
            .await
            .context("Committing imported entries")?;
        for entry in imported {
            self.remember_entry(entry);
        }
        Ok(())
    }

    async fn assign_priority_to_entries(&mut self, priority: u32) -> anyhow::Result<()> {
        let mut transaction = self
            .pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .context("Starting priority transaction")?;
        let entries = Self::read_entries(&mut transaction, None).await?;
        for entry in entries
            .iter()
            .filter(|entry| entry.priority.is_none() && entry.protection.is_none())
        {
            self.check_snapshot(entry.id, Some(entry))?;
        }
        sqlx::query(
            "UPDATE entries SET priority = $1 WHERE priority IS NULL AND protection IS NULL",
        )
        .bind(priority)
        .execute(&mut *transaction)
        .await
        .context("Failed to assign entry priorities")?;
        transaction
            .commit()
            .await
            .context("Committing entry priorities")?;
        for mut entry in entries
            .into_iter()
            .filter(|entry| entry.priority.is_none() && entry.protection.is_none())
        {
            entry.priority = Some(priority);
            self.remember_entry(entry);
        }
        Ok(())
    }
}

fn validate_entry(entry: &Entry) -> Result<(), ModifyEntryError> {
    if entry.title.is_empty() {
        return Err(ModifyEntryError::ValidationError(
            "Entry title can't be empty".into(),
        ));
    }
    entry.validate_protection()?;
    Ok(())
}
