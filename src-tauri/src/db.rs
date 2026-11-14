use rusqlite::{params, Connection, OptionalExtension, Result};
use std::path::{Path, PathBuf};

pub const SCHEMA_VERSION: i64 = 2;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Vault {
    pub id: i64,
    pub root_path: String,
    pub is_active: bool,
    /// Whether `root_path/.tracer` still exists. Not a column - the folder is
    /// the user-facing on/off switch for tracking (see project_plan.md), so
    /// this is computed from disk on every read rather than cached in SQLite,
    /// where it could go stale the moment someone deletes the folder.
    pub has_tracer_folder: bool,
}

fn has_tracer_folder(root_path: &str) -> bool {
    Path::new(root_path).join(".tracer").is_dir()
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackedFile {
    pub id: i64,
    pub vault_id: i64,
    pub relative_path: String,
    pub extension: String,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PartRecord {
    pub entry_order: i64,
    pub entry_name: String,
    pub compression_method: String,
    pub object_hash: String,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectRecord {
    pub hash: String,
    pub size: i64,
    pub stored_size: i64,
    pub compressed: bool,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Revision {
    pub id: i64,
    pub file_id: i64,
    pub timestamp: i64,
    pub byte_size: i64,
    pub manifest_hash: String,
    pub part_count: i64,
    pub milestone: Option<String>,
}

/// Initializes the local database, running any pending schema migrations.
pub fn init_db(db_path: &Path) -> Result<Connection> {
    let conn = Connection::open(db_path)?;

    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "journal_mode", "WAL")?;

    run_migrations(&conn)?;

    Ok(conn)
}

fn run_migrations(conn: &Connection) -> Result<()> {
    let user_version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;

    if user_version < 2 {
        migrate_to_v2(conn)?;
    }

    Ok(())
}

fn migrate_to_v2(conn: &Connection) -> Result<()> {
    // v1 only ever held scaffold data - wiping and recreating is acceptable.
    conn.execute_batch(
        "
        DROP TABLE IF EXISTS revision_parts;
        DROP TABLE IF EXISTS milestones;
        DROP TABLE IF EXISTS revisions;
        DROP TABLE IF EXISTS objects;
        DROP TABLE IF EXISTS files;
        DROP TABLE IF EXISTS vaults;

        CREATE TABLE vaults (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            root_path TEXT NOT NULL UNIQUE,
            is_active INTEGER NOT NULL
        );

        CREATE TABLE files (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            vault_id INTEGER NOT NULL,
            relative_path TEXT NOT NULL,
            extension TEXT NOT NULL,
            UNIQUE(vault_id, relative_path),
            FOREIGN KEY(vault_id) REFERENCES vaults(id)
        );

        CREATE TABLE objects (
            hash TEXT PRIMARY KEY,
            size INTEGER NOT NULL,
            stored_size INTEGER NOT NULL,
            compressed INTEGER NOT NULL
        );

        CREATE TABLE revisions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            file_id INTEGER NOT NULL,
            timestamp INTEGER NOT NULL,
            byte_size INTEGER NOT NULL,
            manifest_hash TEXT NOT NULL,
            FOREIGN KEY(file_id) REFERENCES files(id)
        );

        CREATE TABLE revision_parts (
            revision_id INTEGER NOT NULL,
            entry_order INTEGER NOT NULL,
            entry_name TEXT NOT NULL,
            compression_method TEXT NOT NULL,
            object_hash TEXT NOT NULL,
            PRIMARY KEY(revision_id, entry_order),
            FOREIGN KEY(revision_id) REFERENCES revisions(id),
            FOREIGN KEY(object_hash) REFERENCES objects(hash)
        );

        CREATE TABLE milestones (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            revision_id INTEGER NOT NULL UNIQUE,
            label TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            FOREIGN KEY(revision_id) REFERENCES revisions(id)
        );

        CREATE INDEX idx_revisions_file_id ON revisions(file_id);
        CREATE INDEX idx_revision_parts_object_hash ON revision_parts(object_hash);
        CREATE INDEX idx_files_vault_id ON files(vault_id);
        ",
    )?;

    conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;

    Ok(())
}

/// Inserts a new vault or, if `root_path` already exists, marks it active.
/// Returns the resulting row.
pub fn add_vault(conn: &Connection, root_path: &str) -> Result<Vault> {
    conn.execute(
        "INSERT INTO vaults (root_path, is_active) VALUES (?1, 1)
         ON CONFLICT(root_path) DO UPDATE SET is_active = 1",
        params![root_path],
    )?;

    let id: i64 = conn.query_row(
        "SELECT id FROM vaults WHERE root_path = ?1",
        params![root_path],
        |row| row.get(0),
    )?;

    Ok(Vault {
        id,
        has_tracer_folder: has_tracer_folder(root_path),
        root_path: root_path.to_string(),
        is_active: true,
    })
}

pub fn list_vaults(conn: &Connection) -> Result<Vec<Vault>> {
    let mut stmt = conn.prepare("SELECT id, root_path, is_active FROM vaults ORDER BY id ASC")?;

    let rows = stmt.query_map([], |row| {
        let root_path: String = row.get(1)?;
        Ok(Vault {
            id: row.get(0)?,
            is_active: row.get::<_, i64>(2)? != 0,
            has_tracer_folder: has_tracer_folder(&root_path),
            root_path,
        })
    })?;

    rows.collect()
}

/// Stops or resumes watching a folder without forgetting its history.
// Called by the Folders screen, which doesn't exist yet.
#[allow(dead_code)]
pub fn set_vault_active(conn: &Connection, vault_id: i64, active: bool) -> Result<()> {
    conn.execute(
        "UPDATE vaults SET is_active = ?1 WHERE id = ?2",
        params![active as i64, vault_id],
    )?;

    Ok(())
}

/// Returns the active vault whose root_path is the longest prefix (by path
/// components) of `path`, if any.
pub fn find_vault_for_path(conn: &Connection, path: &Path) -> Result<Option<Vault>> {
    let mut stmt = conn.prepare("SELECT id, root_path, is_active FROM vaults WHERE is_active = 1")?;

    let vaults = stmt.query_map([], |row| {
        let root_path: String = row.get(1)?;
        Ok(Vault {
            id: row.get(0)?,
            is_active: row.get::<_, i64>(2)? != 0,
            has_tracer_folder: has_tracer_folder(&root_path),
            root_path,
        })
    })?;

    let mut best: Option<Vault> = None;
    let mut best_len = 0usize;

    for vault in vaults {
        let vault = vault?;
        let root = PathBuf::from(&vault.root_path);
        let root_components: Vec<_> = root.components().collect();

        if root_components.len() <= path.components().count()
            && path
                .components()
                .zip(root_components.iter())
                .all(|(a, b)| &a == b)
        {
            if root_components.len() > best_len {
                best_len = root_components.len();
                best = Some(vault);
            }
        }
    }

    Ok(best)
}

/// Inserts a file record if it doesn't exist yet, otherwise returns the
/// existing row's id.
pub fn upsert_file(
    conn: &Connection,
    vault_id: i64,
    relative_path: &str,
    extension: &str,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO files (vault_id, relative_path, extension) VALUES (?1, ?2, ?3)
         ON CONFLICT(vault_id, relative_path) DO UPDATE SET extension = excluded.extension",
        params![vault_id, relative_path, extension],
    )?;

    conn.query_row(
        "SELECT id FROM files WHERE vault_id = ?1 AND relative_path = ?2",
        params![vault_id, relative_path],
        |row| row.get(0),
    )
}

pub fn list_tracked_files(conn: &Connection, vault_id: i64) -> Result<Vec<TrackedFile>> {
    let mut stmt = conn.prepare(
        "SELECT id, vault_id, relative_path, extension FROM files WHERE vault_id = ?1 ORDER BY id ASC",
    )?;

    let rows = stmt.query_map(params![vault_id], |row| {
        Ok(TrackedFile {
            id: row.get(0)?,
            vault_id: row.get(1)?,
            relative_path: row.get(2)?,
            extension: row.get(3)?,
        })
    })?;

    rows.collect()
}

/// Returns the manifest_hash of the newest revision (by timestamp, then id)
/// for the given file, if any revisions exist.
pub fn latest_manifest_hash(conn: &Connection, file_id: i64) -> Result<Option<String>> {
    conn.query_row(
        "SELECT manifest_hash FROM revisions WHERE file_id = ?1
         ORDER BY timestamp DESC, id DESC LIMIT 1",
        params![file_id],
        |row| row.get(0),
    )
    .optional()
}

/// Inserts a new revision along with its parts and any not-yet-known
/// objects, all within a single transaction.
pub fn insert_revision(
    conn: &mut Connection,
    file_id: i64,
    timestamp: i64,
    byte_size: i64,
    manifest_hash: &str,
    parts: &[PartRecord],
    objects: &[ObjectRecord],
) -> Result<i64> {
    let tx = conn.transaction()?;

    tx.execute(
        "INSERT INTO revisions (file_id, timestamp, byte_size, manifest_hash) VALUES (?1, ?2, ?3, ?4)",
        params![file_id, timestamp, byte_size, manifest_hash],
    )?;

    let revision_id = tx.last_insert_rowid();

    for object in objects {
        tx.execute(
            "INSERT OR IGNORE INTO objects (hash, size, stored_size, compressed) VALUES (?1, ?2, ?3, ?4)",
            params![object.hash, object.size, object.stored_size, object.compressed as i64],
        )?;
    }

    for part in parts {
        tx.execute(
            "INSERT INTO revision_parts (revision_id, entry_order, entry_name, compression_method, object_hash)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                revision_id,
                part.entry_order,
                part.entry_name,
                part.compression_method,
                part.object_hash
            ],
        )?;
    }

    tx.commit()?;

    Ok(revision_id)
}

pub fn get_timeline(conn: &Connection, file_id: i64) -> Result<Vec<Revision>> {
    let mut stmt = conn.prepare(
        "SELECT r.id, r.file_id, r.timestamp, r.byte_size, r.manifest_hash,
                (SELECT COUNT(*) FROM revision_parts rp WHERE rp.revision_id = r.id) AS part_count,
                m.label
         FROM revisions r
         LEFT JOIN milestones m ON m.revision_id = r.id
         WHERE r.file_id = ?1
         ORDER BY r.timestamp DESC, r.id DESC",
    )?;

    let rows = stmt.query_map(params![file_id], |row| {
        Ok(Revision {
            id: row.get(0)?,
            file_id: row.get(1)?,
            timestamp: row.get(2)?,
            byte_size: row.get(3)?,
            manifest_hash: row.get(4)?,
            part_count: row.get(5)?,
            milestone: row.get(6)?,
        })
    })?;

    rows.collect()
}

/// Returns the file a revision belongs to, for naming a restored copy.
///
/// `relative_path` and `extension` are the two fields the save-a-copy command
/// needs; `TrackedFile` already carries exactly those, so this returns one
/// rather than introducing a near-identical struct.
pub fn get_revision_target(conn: &Connection, revision_id: i64) -> Result<Option<TrackedFile>> {
    conn.query_row(
        "SELECT f.id, f.vault_id, f.relative_path, f.extension
         FROM revisions r JOIN files f ON f.id = r.file_id
         WHERE r.id = ?1",
        params![revision_id],
        |row| {
            Ok(TrackedFile {
                id: row.get(0)?,
                vault_id: row.get(1)?,
                relative_path: row.get(2)?,
                extension: row.get(3)?,
            })
        },
    )
    .optional()
}

pub fn get_revision_parts(conn: &Connection, revision_id: i64) -> Result<Vec<PartRecord>> {
    let mut stmt = conn.prepare(
        "SELECT entry_order, entry_name, compression_method, object_hash
         FROM revision_parts WHERE revision_id = ?1 ORDER BY entry_order ASC",
    )?;

    let rows = stmt.query_map(params![revision_id], |row| {
        Ok(PartRecord {
            entry_order: row.get(0)?,
            entry_name: row.get(1)?,
            compression_method: row.get(2)?,
            object_hash: row.get(3)?,
        })
    })?;

    rows.collect()
}

/// Upserts the milestone label for a revision.
pub fn set_milestone(conn: &Connection, revision_id: i64, label: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO milestones (revision_id, label, created_at) VALUES (?1, ?2, strftime('%s','now'))
         ON CONFLICT(revision_id) DO UPDATE SET label = excluded.label",
        params![revision_id, label],
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    /// Scratch space for tests, kept inside the project under `target/` rather
    /// than the OS temp directory, so a test run leaves nothing outside the repo.
    fn temp_file() -> NamedTempFile {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/test-tmp");
        std::fs::create_dir_all(&root).expect("creating test scratch directory");
        NamedTempFile::new_in(&root).expect("create temp file")
    }

    fn temp_db() -> (NamedTempFile, Connection) {
        let file = temp_file();
        let conn = init_db(file.path()).expect("init db");
        (file, conn)
    }

    #[test]
    fn migration_runs_and_is_idempotent_on_reopen() {
        let file = temp_file();

        {
            let conn = init_db(file.path()).expect("init db");
            let version: i64 = conn
                .query_row("PRAGMA user_version", [], |row| row.get(0))
                .unwrap();
            assert_eq!(version, SCHEMA_VERSION);

            add_vault(&conn, "/a/foo").unwrap();
        }

        // Reopening should not wipe data and should not error.
        let conn = init_db(file.path()).expect("reopen db");
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);

        let vaults = list_vaults(&conn).unwrap();
        assert_eq!(vaults.len(), 1);
        assert_eq!(vaults[0].root_path, "/a/foo");
    }

    #[test]
    fn add_vault_upserts() {
        let (_file, conn) = temp_db();

        let v1 = add_vault(&conn, "/a/foo").unwrap();
        set_vault_active(&conn, v1.id, false).unwrap();

        let v2 = add_vault(&conn, "/a/foo").unwrap();

        assert_eq!(v1.id, v2.id);
        assert!(v2.is_active);

        let vaults = list_vaults(&conn).unwrap();
        assert_eq!(vaults.len(), 1);
    }

    #[test]
    fn find_vault_for_path_uses_longest_prefix_by_components() {
        let (_file, conn) = temp_db();

        add_vault(&conn, "/a/foo").unwrap();
        add_vault(&conn, "/a/foo/bar").unwrap();

        // Must NOT match "/a/foo" via raw string prefix.
        let result = find_vault_for_path(&conn, Path::new("/a/foobar")).unwrap();
        assert!(result.is_none());

        // Nested vault: longest matching prefix wins.
        let result = find_vault_for_path(&conn, Path::new("/a/foo/bar/baz.pptx")).unwrap();
        assert_eq!(result.unwrap().root_path, "/a/foo/bar");

        // Falls back to the shorter vault when the nested one doesn't match.
        let result = find_vault_for_path(&conn, Path::new("/a/foo/baz.pptx")).unwrap();
        assert_eq!(result.unwrap().root_path, "/a/foo");
    }

    #[test]
    fn find_vault_for_path_ignores_inactive_vaults() {
        let (_file, conn) = temp_db();

        let vault = add_vault(&conn, "/a/foo").unwrap();
        set_vault_active(&conn, vault.id, false).unwrap();

        let result = find_vault_for_path(&conn, Path::new("/a/foo/baz.pptx")).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn insert_revision_round_trips_parts_in_order() {
        let (_file, mut conn) = temp_db();

        let vault = add_vault(&conn, "/a/foo").unwrap();
        let file_id = upsert_file(&conn, vault.id, "deck.pptx", "pptx").unwrap();

        let parts = vec![
            PartRecord {
                entry_order: 0,
                entry_name: "[Content_Types].xml".to_string(),
                compression_method: "deflate".to_string(),
                object_hash: "hash_a".to_string(),
            },
            PartRecord {
                entry_order: 1,
                entry_name: "ppt/presentation.xml".to_string(),
                compression_method: "deflate".to_string(),
                object_hash: "hash_b".to_string(),
            },
            PartRecord {
                entry_order: 2,
                entry_name: "ppt/slides/slide1.xml".to_string(),
                compression_method: "stored".to_string(),
                object_hash: "hash_a".to_string(),
            },
        ];

        let objects = vec![
            ObjectRecord {
                hash: "hash_a".to_string(),
                size: 100,
                stored_size: 40,
                compressed: true,
            },
            ObjectRecord {
                hash: "hash_b".to_string(),
                size: 200,
                stored_size: 200,
                compressed: false,
            },
        ];

        let revision_id = insert_revision(&mut conn, file_id, 1000, 240, "manifest_1", &parts, &objects)
            .unwrap();

        let fetched = get_revision_parts(&conn, revision_id).unwrap();
        assert_eq!(fetched.len(), 3);
        assert_eq!(
            fetched.iter().map(|p| p.entry_order).collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
        assert_eq!(fetched[1].entry_name, "ppt/presentation.xml");

        let stored_hashes: Vec<String> = conn
            .prepare("SELECT hash FROM objects ORDER BY hash ASC")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_>>()
            .unwrap();
        assert_eq!(stored_hashes, vec!["hash_a", "hash_b"]);

        let timeline = get_timeline(&conn, file_id).unwrap();
        assert_eq!(timeline.len(), 1);
        assert_eq!(timeline[0].part_count, 3);
        assert_eq!(timeline[0].milestone, None);

        set_milestone(&conn, revision_id, "v1.0").unwrap();
        let timeline = get_timeline(&conn, file_id).unwrap();
        assert_eq!(timeline[0].milestone, Some("v1.0".to_string()));
    }

    #[test]
    fn two_files_in_different_vaults_can_share_relative_path() {
        let (_file, conn) = temp_db();

        let vault_a = add_vault(&conn, "/a/foo").unwrap();
        let vault_b = add_vault(&conn, "/b/bar").unwrap();

        let file_a = upsert_file(&conn, vault_a.id, "deck.pptx", "pptx").unwrap();
        let file_b = upsert_file(&conn, vault_b.id, "deck.pptx", "pptx").unwrap();

        assert_ne!(file_a, file_b);

        let files_a = list_tracked_files(&conn, vault_a.id).unwrap();
        let files_b = list_tracked_files(&conn, vault_b.id).unwrap();
        assert_eq!(files_a.len(), 1);
        assert_eq!(files_b.len(), 1);
        assert_eq!(files_a[0].relative_path, files_b[0].relative_path);
    }

    #[test]
    fn get_revision_target_resolves_the_file_and_is_none_when_missing() {
        let (_file, mut conn) = temp_db();

        let vault = add_vault(&conn, "/a/foo").unwrap();
        let file_id = upsert_file(&conn, vault.id, "decks/deck.pptx", "pptx").unwrap();
        let revision_id = insert_revision(&mut conn, file_id, 1000, 10, "m1", &[], &[]).unwrap();

        let target = get_revision_target(&conn, revision_id).unwrap().unwrap();
        assert_eq!(target.id, file_id);
        assert_eq!(target.vault_id, vault.id);
        assert_eq!(target.relative_path, "decks/deck.pptx");
        assert_eq!(target.extension, "pptx");

        assert!(get_revision_target(&conn, revision_id + 999)
            .unwrap()
            .is_none());
    }

    #[test]
    fn latest_manifest_hash_picks_newest_by_timestamp_then_id() {
        let (_file, mut conn) = temp_db();

        let vault = add_vault(&conn, "/a/foo").unwrap();
        let file_id = upsert_file(&conn, vault.id, "deck.pptx", "pptx").unwrap();

        assert_eq!(latest_manifest_hash(&conn, file_id).unwrap(), None);

        insert_revision(&mut conn, file_id, 1000, 10, "m1", &[], &[]).unwrap();
        insert_revision(&mut conn, file_id, 2000, 10, "m2", &[], &[]).unwrap();
        insert_revision(&mut conn, file_id, 2000, 10, "m3", &[], &[]).unwrap();

        assert_eq!(latest_manifest_hash(&conn, file_id).unwrap(), Some("m3".to_string()));
    }
}
