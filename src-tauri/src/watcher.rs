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
    let (tx, rx) = mpsc::channel::<DebounceEventResult>();

    let mut debouncer = new_debouncer(DEBOUNCE_TIMEOUT, None, tx)
        .context("creating file watcher debouncer")?;

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
}
