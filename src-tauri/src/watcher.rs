use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{new_debouncer, DebounceEventResult, Debouncer, RecommendedCache};
use rusqlite::Connection;

use crate::{db, engine};

/// Office saves fire several create/rename events per file; 5s collapses a
/// single save into one batch instead of one revision per event.
const DEBOUNCE_TIMEOUT: Duration = Duration::from_secs(5);

/// Owns the debouncer for the app's lifetime. `Debouncer` stops watching as
/// soon as it's dropped (`impl Drop` calls `set_stop`), so this handle must be
/// kept alive in `AppState` rather than discarded after `spawn` returns.
///
/// `watch()` needs `&mut self`; `add_path` only gets `&self` from callers, so
/// the mutex supplies the mutability.
pub struct WatchHandle {
    debouncer: Mutex<Debouncer<RecommendedWatcher, RecommendedCache>>,
}

impl WatchHandle {
    pub fn add_path(&self, path: &Path) -> anyhow::Result<()> {
        self.debouncer
            .lock()
            .map_err(|_| anyhow::anyhow!("watcher lock poisoned by an earlier panic"))?
            .watch(path, RecursiveMode::Recursive)
            .with_context(|| format!("watching {}", path.display()))?;
        Ok(())
    }
}

fn is_trackable(path: &Path) -> bool {
    // The watcher must never record its own bookkeeping - without this a
    // save under `.tracer/` would eventually get treated as a tracked file.
    if path.components().any(|c| c.as_os_str() == ".tracer") {
        return false;
    }

    let Some(extension) = path.extension() else {
        return false;
    };
    let extension = extension.to_string_lossy().to_lowercase();
    if extension != "pptx" && extension != "docx" && extension != "xlsx" {
        return false;
    }

    let Some(file_name) = path.file_name() else {
        return false;
    };
    !file_name.to_string_lossy().starts_with("~$")
}

/// Starts watching `paths` and commits a revision for every real save.
///
/// The debouncer runs its own internal thread and sends each coalesced batch
/// down a channel; this drains that channel on one more thread, so `spawn`
/// returns immediately and `?` works inside Tauri's `setup` closure.
pub fn spawn(
    paths: Vec<PathBuf>,
    conn: Arc<Mutex<Connection>>,
    objects_root: PathBuf,
    on_revision: impl Fn(i64, i64) + Send + 'static,
) -> anyhow::Result<WatchHandle> {
    spawn_with_debounce(paths, conn, objects_root, on_revision, DEBOUNCE_TIMEOUT)
}

/// `spawn`, with the debounce window exposed so tests don't wait five seconds
/// per save.
fn spawn_with_debounce(
    paths: Vec<PathBuf>,
    conn: Arc<Mutex<Connection>>,
    objects_root: PathBuf,
    on_revision: impl Fn(i64, i64) + Send + 'static,
    debounce: Duration,
) -> anyhow::Result<WatchHandle> {
    let (tx, rx) = mpsc::channel::<DebounceEventResult>();

    let mut debouncer =
        new_debouncer(debounce, None, tx).context("creating file watcher debouncer")?;

    for path in &paths {
        debouncer
            .watch(path, RecursiveMode::Recursive)
            .with_context(|| format!("watching {}", path.display()))?;
    }

    std::thread::spawn(move || {
        for result in rx {
            match result {
                Ok(events) => {
                    let mut paths: Vec<PathBuf> =
                        events.into_iter().flat_map(|e| e.event.paths).collect();
                    paths.sort();
                    paths.dedup();

                    for path in paths {
                        if let Err(e) =
                            process_path(&path, &conn, &objects_root, &on_revision)
                        {
                            eprintln!("tracer: failed to process {}: {e:#}", path.display());
                        }
                    }
                }
                Err(errors) => {
                    for e in errors {
                        eprintln!("tracer: watch error: {e:?}");
                    }
                }
            }
        }
    });

    Ok(WatchHandle {
        debouncer: Mutex::new(debouncer),
    })
}

fn lock(conn: &Arc<Mutex<Connection>>) -> Result<std::sync::MutexGuard<'_, Connection>> {
    conn.lock()
        .map_err(|_| anyhow::anyhow!("database lock poisoned by an earlier panic"))
}

fn process_path(
    path: &Path,
    conn: &Arc<Mutex<Connection>>,
    objects_root: &Path,
    on_revision: &impl Fn(i64, i64),
) -> Result<()> {
    if !is_trackable(path) || !path.is_file() {
        return Ok(());
    }

    // Vault roots are canonicalized when added, but notify reports whatever
    // path the OS gave it - on macOS /tmp for /private/tmp, and the raw path
    // through any symlinked folder. Without matching that, find_vault_for_path
    // never fires and the file is silently never tracked.
    let path = &path
        .canonicalize()
        .with_context(|| format!("canonicalizing {}", path.display()))?;

    let file_id = {
        let conn = lock(conn)?;

        let Some(vault) = db::find_vault_for_path(&conn, path)? else {
            return Ok(());
        };

        // `.tracer` is the user-facing on/off switch (see project_plan.md):
        // deleting it must stop tracking immediately, with no restart and no
        // app interaction, so this is checked on every save rather than once
        // at startup.
        if !Path::new(&vault.root_path).join(".tracer").is_dir() {
            return Ok(());
        }

        let Ok(relative_path) = path.strip_prefix(&vault.root_path) else {
            return Ok(());
        };

        let extension = path
            .extension()
            .expect("is_trackable guarantees an extension")
            .to_string_lossy()
            .to_lowercase();
        let relative_path = relative_path.to_string_lossy().replace('\\', "/");

        db::upsert_file(&conn, vault.id, &relative_path, &extension)?
    };

    // Never hold the DB lock while snapshotting - hashing a multi-megabyte
    // deck must not block every other UI command for its duration.
    let snap = engine::snapshot(path, objects_root)?;

    let revision_id = {
        let mut conn = lock(conn)?;

        if db::latest_manifest_hash(&conn, file_id)?.as_deref() == Some(&snap.manifest_hash) {
            return Ok(());
        }

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("system clock is before the Unix epoch")?
            .as_secs() as i64;

        db::insert_revision(
            &mut conn,
            file_id,
            timestamp,
            snap.byte_size,
            &snap.manifest_hash,
            &snap.parts,
            &snap.objects,
        )?
    };

    // Deliberately outside the lock: the callback emits to the UI, which reacts
    // by querying the timeline. Notifying while still holding the connection
    // invites a stall now and a deadlock the moment the callback touches the DB.
    on_revision(file_id, revision_id);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    /// Scratch space inside the project under `target/`, never the OS temp
    /// directory, so a test run leaves nothing outside this repo.
    fn scratch() -> TempDir {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/test-tmp");
        std::fs::create_dir_all(&root).expect("creating test scratch directory");
        TempDir::new_in(root).expect("creating scratch directory")
    }

    /// A minimal but genuine zip, so the engine has something real to snapshot.
    fn write_office_file(path: &Path, body: &[u8]) {
        let file = std::fs::File::create(path).expect("creating office file");
        let mut writer = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::DEFAULT
            .compression_method(zip::CompressionMethod::Deflated);
        writer.start_file("[Content_Types].xml", options).unwrap();
        writer.write_all(b"<Types/>").unwrap();
        writer.start_file("ppt/slides/slide1.xml", options).unwrap();
        writer.write_all(body).unwrap();
        writer.finish().unwrap();
    }

    /// Like `write_office_file`, but with `docProps/core.xml` set to
    /// `metadata`, to simulate Office rewriting its own save metadata (revision
    /// counter, modified timestamp) independent of whatever the user changed.
    fn write_office_file_with_metadata(path: &Path, body: &[u8], metadata: &[u8]) {
        let file = std::fs::File::create(path).expect("creating office file");
        let mut writer = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::DEFAULT
            .compression_method(zip::CompressionMethod::Deflated);
        writer.start_file("[Content_Types].xml", options).unwrap();
        writer.write_all(b"<Types/>").unwrap();
        writer.start_file("docProps/core.xml", options).unwrap();
        writer.write_all(metadata).unwrap();
        writer.start_file("ppt/slides/slide1.xml", options).unwrap();
        writer.write_all(body).unwrap();
        writer.finish().unwrap();
    }

    /// The product's whole promise: save a file in a watched folder and a
    /// version appears on its own. Exercises the real debouncer, the real
    /// filesystem and the real database - the only thing stubbed is the five
    /// second debounce window.
    #[test]
    fn saving_a_file_records_a_version_without_being_asked() {
        let vault_dir = scratch();
        let objects_dir = scratch();
        let db_dir = scratch();

        let root = vault_dir.path().canonicalize().expect("canonicalizing vault");
        std::fs::create_dir_all(root.join(".tracer")).expect("creating .tracer");
        let conn = db::init_db(&db_dir.path().join("tracer.db")).expect("init db");
        db::add_vault(&conn, root.to_str().unwrap()).expect("adding vault");
        let conn = Arc::new(Mutex::new(conn));

        let (tx, rx) = mpsc::channel();
        // Bound, not dropped: dropping the handle stops the debouncer silently.
        let _handle = spawn_with_debounce(
            vec![root.clone()],
            Arc::clone(&conn),
            objects_dir.path().to_path_buf(),
            move |file_id, revision_id| {
                let _ = tx.send((file_id, revision_id));
            },
            Duration::from_millis(150),
        )
        .expect("spawning watcher");

        let deck = root.join("deck.pptx");

        write_office_file(&deck, b"<p>first draft</p>");
        let (file_id, first) = rx
            .recv_timeout(Duration::from_secs(20))
            .expect("saving a file should record a version");

        write_office_file(&deck, b"<p>second draft</p>");
        let (_, second) = rx
            .recv_timeout(Duration::from_secs(20))
            .expect("changing the file should record another version");
        assert_ne!(first, second);

        // Saving without changing anything must not pile up a duplicate - this
        // is what latest_manifest_hash is for, and later what stops a sync
        // tool's own writes creating phantom versions.
        write_office_file(&deck, b"<p>second draft</p>");
        assert!(
            rx.recv_timeout(Duration::from_secs(3)).is_err(),
            "an unchanged save must not record a version"
        );

        let conn = conn.lock().unwrap();
        let timeline = db::get_timeline(&conn, file_id).expect("reading timeline");
        assert_eq!(timeline.len(), 2, "two real edits, two versions");
        assert!(timeline[0].part_count > 0);
        assert_eq!(
            db::get_revision_target(&conn, timeline[0].id)
                .unwrap()
                .unwrap()
                .relative_path,
            "deck.pptx"
        );
    }

    /// The product's other hard requirement: deleting `.tracer` turns
    /// tracking off immediately, and creating it turns tracking on, with no
    /// app restart in between.
    #[test]
    fn tracer_marker_folder_gates_whether_saves_are_recorded() {
        let vault_dir = scratch();
        let objects_dir = scratch();
        let db_dir = scratch();

        let root = vault_dir.path().canonicalize().expect("canonicalizing vault");
        let conn = db::init_db(&db_dir.path().join("tracer.db")).expect("init db");
        db::add_vault(&conn, root.to_str().unwrap()).expect("adding vault");
        let conn = Arc::new(Mutex::new(conn));

        let (tx, rx) = mpsc::channel();
        let _handle = spawn_with_debounce(
            vec![root.clone()],
            Arc::clone(&conn),
            objects_dir.path().to_path_buf(),
            move |file_id, revision_id| {
                let _ = tx.send((file_id, revision_id));
            },
            Duration::from_millis(150),
        )
        .expect("spawning watcher");

        let deck = root.join("deck.pptx");

        write_office_file(&deck, b"<p>first draft</p>");
        assert!(
            rx.recv_timeout(Duration::from_secs(3)).is_err(),
            "a save must not be recorded while .tracer is absent"
        );

        std::fs::create_dir_all(root.join(".tracer")).expect("creating .tracer");
        write_office_file(&deck, b"<p>second draft</p>");
        rx.recv_timeout(Duration::from_secs(20))
            .expect("saving a file should record a version once .tracer exists");
    }

    #[test]
    fn paths_inside_the_tracer_folder_are_never_trackable() {
        assert!(!is_trackable(Path::new("/vault/.tracer/deck.pptx")));
        assert!(!is_trackable(Path::new("/vault/sub/.tracer/nested/deck.pptx")));
    }

    #[test]
    fn trackable_extensions_pass_case_insensitively() {
        assert!(is_trackable(Path::new("deck.pptx")));
        assert!(is_trackable(Path::new("doc.docx")));
        assert!(is_trackable(Path::new("sheet.xlsx")));
        assert!(is_trackable(Path::new("DECK.PPTX")));
    }

    #[test]
    fn office_lock_files_are_rejected() {
        assert!(!is_trackable(Path::new("~$deck.pptx")));
    }

    #[test]
    fn untracked_extensions_are_rejected() {
        assert!(!is_trackable(Path::new("notes.txt")));
        assert!(!is_trackable(Path::new("no_extension")));
    }

    /// The user-visible reason for the whole `manifest_hash` change: Office
    /// rewrites `docProps/core.xml` on every save, even one that changes
    /// nothing a user would recognize, and that must not produce a version.
    #[test]
    fn metadata_only_resave_does_not_record_a_second_version() {
        let vault_dir = scratch();
        let objects_dir = scratch();
        let db_dir = scratch();

        let root = vault_dir.path().canonicalize().expect("canonicalizing vault");
        std::fs::create_dir_all(root.join(".tracer")).expect("creating .tracer");
        let conn = db::init_db(&db_dir.path().join("tracer.db")).expect("init db");
        db::add_vault(&conn, root.to_str().unwrap()).expect("adding vault");
        let conn = Arc::new(Mutex::new(conn));

        let (tx, rx) = mpsc::channel();
        let _handle = spawn_with_debounce(
            vec![root.clone()],
            Arc::clone(&conn),
            objects_dir.path().to_path_buf(),
            move |file_id, revision_id| {
                let _ = tx.send((file_id, revision_id));
            },
            Duration::from_millis(150),
        )
        .expect("spawning watcher");

        let deck = root.join("deck.pptx");

        write_office_file_with_metadata(&deck, b"<p>draft</p>", b"<cp:revision>1</cp:revision>");
        let (file_id, _first) = rx
            .recv_timeout(Duration::from_secs(20))
            .expect("saving a file should record a version");

        write_office_file_with_metadata(&deck, b"<p>draft</p>", b"<cp:revision>2</cp:revision>");
        assert!(
            rx.recv_timeout(Duration::from_secs(3)).is_err(),
            "a metadata-only resave must not record a version"
        );

        let conn = conn.lock().unwrap();
        let timeline = db::get_timeline(&conn, file_id).expect("reading timeline");
        assert_eq!(timeline.len(), 1, "one real edit, one version");
    }
}
