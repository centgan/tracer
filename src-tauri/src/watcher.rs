use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{new_debouncer, DebounceEventResult, Debouncer, RecommendedCache};
use rusqlite::Connection;

use crate::{db, engine, identity};

/// Office saves fire several create/rename events per file; this collapses a
/// single save into one batch instead of one version per event.
///
/// It is also almost all of the delay between saving and seeing the version
/// appear. Measured on real files (`snapshot_cost_on_real_files`), the work
/// itself takes 6-64ms on a repeat save - a 3.7MB .docx with 44 parts takes
/// 64ms, a 192-part .pptx takes 30ms - so the window, not the hashing, is what
/// anyone waiting is waiting for. One second keeps a save's events together
/// while staying short enough to feel immediate.
const DEBOUNCE_TIMEOUT: Duration = Duration::from_millis(1000);

/// How many times to re-attempt a snapshot that failed to read the file.
///
/// Office saves by writing a temp file and renaming it over the target, so the
/// file is complete the moment it appears. Not every editor does that - some
/// write in place - and with a short debounce we can arrive mid-write and find
/// an unreadable archive. Retrying costs nothing when nothing is wrong, and is
/// what makes a one second window safe.
const SNAPSHOT_ATTEMPTS: u32 = 3;
const SNAPSHOT_RETRY_DELAY: Duration = Duration::from_millis(400);

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

    pub fn remove_path(&self, path: &Path) -> anyhow::Result<()> {
        self.debouncer
            .lock()
            .map_err(|_| anyhow::anyhow!("watcher lock poisoned by an earlier panic"))?
            .unwatch(path)
            .with_context(|| format!("unwatching {}", path.display()))?;
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
    identity_path: PathBuf,
    on_revision: impl Fn(i64, i64) + Send + 'static,
) -> anyhow::Result<WatchHandle> {
    spawn_with_debounce(
        paths,
        conn,
        objects_root,
        identity_path,
        on_revision,
        DEBOUNCE_TIMEOUT,
    )
}

/// `spawn`, with the debounce window exposed so tests don't wait five seconds
/// per save.
fn spawn_with_debounce(
    paths: Vec<PathBuf>,
    conn: Arc<Mutex<Connection>>,
    objects_root: PathBuf,
    identity_path: PathBuf,
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
                            process_path(&path, &conn, &objects_root, &identity_path, &on_revision)
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

/// Snapshots a file, re-attempting if it can't be read as an archive.
///
/// A file caught mid-write isn't an error worth reporting - it's a race we
/// expect to lose occasionally and win a moment later. Only the final attempt's
/// failure propagates, so a genuinely corrupt file still surfaces rather than
/// being retried into silence.
fn snapshot_with_retry(path: &Path, objects_root: &Path) -> Result<engine::Snapshot> {
    let mut attempt = 1;
    loop {
        match engine::snapshot(path, objects_root) {
            Ok(snapshot) => return Ok(snapshot),
            Err(e) if attempt < SNAPSHOT_ATTEMPTS => {
                eprintln!(
                    "tracer: {} not readable yet (attempt {attempt}/{SNAPSHOT_ATTEMPTS}), retrying: {e:#}",
                    path.display()
                );
                std::thread::sleep(SNAPSHOT_RETRY_DELAY);
                attempt += 1;
            }
            Err(e) => return Err(e),
        }
    }
}

fn process_path(
    path: &Path,
    conn: &Arc<Mutex<Connection>>,
    objects_root: &Path,
    identity_path: &Path,
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
    let snap = snapshot_with_retry(path, objects_root)?;

    // Reading identity is a plain file read, not a database query - it must
    // not serialize against every other command just because it happens to
    // run on the watcher thread.
    let who = identity::read(identity_path);
    let author_name = (!who.name.is_empty()).then_some(who.name.as_str());
    let author_email = (!who.email.is_empty()).then_some(who.email.as_str());

    let revision_id = {
        let mut conn = lock(conn)?;

        let latest = db::latest_revision(&conn, file_id)?;
        if latest.as_ref().map(|(_, hash)| hash.as_str()) == Some(snap.manifest_hash.as_str()) {
            return Ok(());
        }
        let parent_id = latest.map(|(id, _)| id);

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("system clock is before the Unix epoch")?
            .as_secs() as i64;

        db::insert_revision(
            &mut conn,
            db::NewRevision {
                file_id,
                timestamp,
                byte_size: snap.byte_size,
                manifest_hash: &snap.manifest_hash,
                parts: &snap.parts,
                objects: &snap.objects,
                author_name,
                author_email,
                parent_id,
            },
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
            db_dir.path().join("identity.json"),
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
            db_dir.path().join("identity.json"),
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
    fn a_file_still_being_written_is_retried_until_it_is_readable() {
        let dir = scratch();
        let objects = scratch();
        let deck = dir.path().join("deck.pptx");

        // Arrives as something that is not yet a readable archive, the way an
        // editor writing in place would leave it.
        std::fs::write(&deck, b"not a zip yet").unwrap();

        let finisher = {
            let deck = deck.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(500));
                write_office_file(&deck, b"<p>finished</p>");
            })
        };

        let snapshot = snapshot_with_retry(&deck, objects.path())
            .expect("a file that becomes readable must not be dropped");
        finisher.join().unwrap();

        assert!(snapshot.parts.iter().any(|p| p.entry_name.contains("slide1")));
    }

    #[test]
    fn a_file_that_never_becomes_readable_eventually_gives_up() {
        let dir = scratch();
        let objects = scratch();
        let deck = dir.path().join("broken.pptx");
        std::fs::write(&deck, b"never a zip").unwrap();

        let started = std::time::Instant::now();
        let result = snapshot_with_retry(&deck, objects.path());

        assert!(result.is_err(), "a genuinely corrupt file must surface, not retry forever");
        assert!(
            started.elapsed() >= SNAPSHOT_RETRY_DELAY,
            "it should have waited between attempts rather than failing instantly"
        );
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
            db_dir.path().join("identity.json"),
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

    /// End to end: an identity set on this device shows up on the version the
    /// watcher records, and the first version of a file has no parent.
    #[test]
    fn a_recorded_version_carries_the_set_identity() {
        let vault_dir = scratch();
        let objects_dir = scratch();
        let db_dir = scratch();

        let root = vault_dir.path().canonicalize().expect("canonicalizing vault");
        std::fs::create_dir_all(root.join(".tracer")).expect("creating .tracer");
        let conn = db::init_db(&db_dir.path().join("tracer.db")).expect("init db");
        db::add_vault(&conn, root.to_str().unwrap()).expect("adding vault");
        let conn = Arc::new(Mutex::new(conn));

        let identity_path = db_dir.path().join("identity.json");
        identity::write(&identity_path, "Eric Feng", "eric@example.com").expect("setting identity");

        let (tx, rx) = mpsc::channel();
        let _handle = spawn_with_debounce(
            vec![root.clone()],
            Arc::clone(&conn),
            objects_dir.path().to_path_buf(),
            identity_path,
            move |file_id, revision_id| {
                let _ = tx.send((file_id, revision_id));
            },
            Duration::from_millis(150),
        )
        .expect("spawning watcher");

        let deck = root.join("deck.pptx");

        write_office_file(&deck, b"<p>first draft</p>");
        let (file_id, _) = rx
            .recv_timeout(Duration::from_secs(20))
            .expect("saving a file should record a version");

        let conn = conn.lock().unwrap();
        let timeline = db::get_timeline(&conn, file_id).expect("reading timeline");
        assert_eq!(timeline.len(), 1);
        assert_eq!(timeline[0].author_name, Some("Eric Feng".to_string()));
        assert_eq!(timeline[0].author_email, Some("eric@example.com".to_string()));
        assert_eq!(timeline[0].parent_id, None, "a file's first version has no parent");
    }
}
