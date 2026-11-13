use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use rusqlite::Connection;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

mod db;
mod engine;
mod watcher;

/// Everything the command handlers need. Held in Tauri's managed state.
///
/// The connection is shared with the watcher thread behind a mutex rather than
/// opened per-thread, so both sides see the same WAL and the same in-flight
/// transactions. Locks are held only for the duration of a single query.
struct AppState {
    conn: Arc<Mutex<Connection>>,
    objects_root: PathBuf,
    watch: watcher::WatchHandle,
}

/// Emitted to the UI whenever the watcher commits a revision, so a save made in
/// PowerPoint shows up in the timeline without the user clicking anything.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RevisionCreated {
    file_id: i64,
    revision_id: i64,
}

fn to_err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

/// The mutex is only ever poisoned if a thread panicked while holding it, which
/// would mean the database is in an unknown state. Surface that rather than
/// unwrapping into a second panic.
fn lock(state: &AppState) -> Result<std::sync::MutexGuard<'_, Connection>, String> {
    state
        .conn
        .lock()
        .map_err(|_| "database lock poisoned by an earlier panic".to_string())
}

#[tauri::command]
fn list_vaults(state: State<'_, AppState>) -> Result<Vec<db::Vault>, String> {
    let conn = lock(&state)?;
    db::list_vaults(&conn).map_err(to_err)
}

#[tauri::command]
fn add_vault(path: String, state: State<'_, AppState>) -> Result<db::Vault, String> {
    let picked = PathBuf::from(&path);
    if !picked.is_dir() {
        return Err(format!("not a directory: {path}"));
    }

    // Canonicalize before storing. macOS hands out /tmp but `notify` reports
    // /private/tmp, and symlinked folders differ the same way; if the stored
    // root and the observed event path disagree, the vault prefix match in
    // find_vault_for_path silently never fires and nothing is ever tracked.
    let root = picked.canonicalize().map_err(to_err)?;
    let root_str = root.to_string_lossy().to_string();

    let vault = {
        let conn = lock(&state)?;
        db::add_vault(&conn, &root_str).map_err(to_err)?
    };

    // Start watching immediately — no restart required.
    state.watch.add_path(&root).map_err(to_err)?;

    Ok(vault)
}

#[tauri::command]
fn list_tracked_files(
    vault_id: i64,
    state: State<'_, AppState>,
) -> Result<Vec<db::TrackedFile>, String> {
    let conn = lock(&state)?;
    db::list_tracked_files(&conn, vault_id).map_err(to_err)
}

#[tauri::command]
fn get_timeline(file_id: i64, state: State<'_, AppState>) -> Result<Vec<db::Revision>, String> {
    let conn = lock(&state)?;
    db::get_timeline(&conn, file_id).map_err(to_err)
}

#[tauri::command]
fn set_milestone(
    revision_id: i64,
    label: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let conn = lock(&state)?;
    db::set_milestone(&conn, revision_id, &label).map_err(to_err)
}

/// Rebuild a historical revision and drop a copy of it on the Desktop.
///
/// Named `save_copy`, not "export": the product vocabulary reserves "export"
/// for zipping up a file's whole history to send someone, and this is the
/// restore feature instead. Copies are named `<stem>_rev<id>.<ext>` and marked
/// read-only. The revision id rather than a formatted timestamp keeps the name
/// stable and collision-free without pulling in a date-formatting dependency;
/// read-only is there so a recovered copy can't be mistaken for the live file
/// and edited by accident.
#[tauri::command]
fn save_copy(
    revision_id: i64,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let (target, parts) = {
        let conn = lock(&state)?;
        let target = db::get_revision_target(&conn, revision_id)
            .map_err(to_err)?
            .ok_or_else(|| format!("no such revision: {revision_id}"))?;
        let parts = db::get_revision_parts(&conn, revision_id).map_err(to_err)?;
        (target, parts)
    };

    if parts.is_empty() {
        return Err(format!("revision {revision_id} has no stored parts"));
    }

    let desktop = app.path().desktop_dir().map_err(to_err)?;

    let stem = PathBuf::from(&target.relative_path)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "export".to_string());

    let dest = desktop.join(format!("{stem}_rev{revision_id}.{}", target.extension));

    engine::restore(&parts, &state.objects_root, &dest).map_err(to_err)?;

    // Best-effort: a failure to flip the permission bit shouldn't fail an
    // otherwise-successful export.
    if let Ok(meta) = std::fs::metadata(&dest) {
        let mut perms = meta.permissions();
        perms.set_readonly(true);
        let _ = std::fs::set_permissions(&dest, perms);
    }

    Ok(dest.to_string_lossy().to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // Storage lives in the OS app-data directory, not the process's
            // working directory. Under `tauri dev` the CWD is src-tauri/, but a
            // bundled .app launched from Finder gets `/` — the old relative
            // "./tracer-db.db" would have pointed at two different files.
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;

            let objects_root = data_dir.join("objects");
            std::fs::create_dir_all(&objects_root)?;

            let conn = db::init_db(&data_dir.join("tracer.db"))?;
            let active_paths = db::get_active_vault_paths(&conn)?;
            let shared_conn = Arc::new(Mutex::new(conn));

            let emitter = app.handle().clone();
            let watch = watcher::spawn(
                active_paths,
                Arc::clone(&shared_conn),
                objects_root.clone(),
                move |file_id, revision_id| {
                    let _ = emitter.emit(
                        "revision-created",
                        RevisionCreated {
                            file_id,
                            revision_id,
                        },
                    );
                },
            )?;

            app.manage(AppState {
                conn: shared_conn,
                objects_root,
                watch,
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_vaults,
            add_vault,
            list_tracked_files,
            get_timeline,
            set_milestone,
            save_copy
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
