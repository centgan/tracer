use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use rusqlite::Connection;
use serde::Serialize;
use tauri::menu::{MenuBuilder, MenuItem, MenuItemBuilder, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_autostart::ManagerExt;

mod db;
mod engine;
mod identity;
mod watcher;

/// Dropped verbatim into every newly tracked folder. Plain language, no
/// jargon - the person reading it has never heard of a VCS.
const TRACER_README: &str = "This folder is being tracked by Tracer.

Every time you save a Word, Excel or PowerPoint file in this folder,
Tracer keeps a version of it that you can go back to.

To stop tracking this folder, delete this .tracer folder.
Your files are never touched, and nothing is uploaded anywhere.
";

/// Creates the `.tracer` folder that marks `root` as tracked.
///
/// The folder is the user's own on/off switch (see project_plan.md): deleting
/// it stops tracking, with no app interaction. Because it's hidden by the
/// leading dot, the README inside it is the only place someone browsing the
/// folder in Finder can learn what it is and how to turn it off.
///
/// Re-adding an already-tracked folder rewrites the README rather than failing,
/// so a folder whose README was deleted heals on the next add.
fn create_tracer_marker(root: &std::path::Path) -> std::io::Result<()> {
    let tracer_dir = root.join(".tracer");
    std::fs::create_dir_all(&tracer_dir)?;
    std::fs::write(tracer_dir.join("README.txt"), TRACER_README)
}

/// Everything the command handlers need. Held in Tauri's managed state.
///
/// The connection is shared with the watcher thread behind a mutex rather than
/// opened per-thread, so both sides see the same WAL and the same in-flight
/// transactions. Locks are held only for the duration of a single query.
struct AppState {
    conn: Arc<Mutex<Connection>>,
    objects_root: PathBuf,
    watch: watcher::WatchHandle,
    /// The disabled "Watching N folders" tray line, kept so it can be
    /// refreshed whenever vaults change instead of only reflecting startup.
    tray_status: MenuItem<tauri::Wry>,
    /// A tray icon is reference-counted and disappears when its last handle
    /// is dropped - this field exists purely to keep it alive for the app's
    /// lifetime, not to be read again.
    #[allow(dead_code)]
    tray: tauri::tray::TrayIcon,
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

fn watch_status_label(watched: usize) -> String {
    match watched {
        0 => "Not watching any folders".to_string(),
        1 => "Watching 1 folder".to_string(),
        n => format!("Watching {n} folders"),
    }
}

/// Recomputes the tray's status line. Called whenever vaults change, so the
/// count never shows stale data (e.g. a folder added, or one whose `.tracer`
/// marker was deleted out from under it).
fn refresh_watch_status(state: &AppState) -> Result<(), String> {
    let vaults = {
        let conn = lock(state)?;
        db::list_vaults(&conn).map_err(to_err)?
    };
    let watched = vaults
        .iter()
        .filter(|v| v.is_active && v.has_tracer_folder)
        .count();
    state
        .tray_status
        .set_text(watch_status_label(watched))
        .map_err(to_err)
}

/// Brings the (hidden, never closed) main window back. Shared by the tray's
/// "Open Tracer" item and clicking the Dock icon.
fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
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

    create_tracer_marker(&root).map_err(to_err)?;

    let vault = {
        let conn = lock(&state)?;
        db::add_vault(&conn, &root_str).map_err(to_err)?
    };

    // Start watching immediately — no restart required.
    state.watch.add_path(&root).map_err(to_err)?;

    refresh_watch_status(&state)?;

    Ok(vault)
}

/// Exits the process outright. Bound to the tray's "Quit Tracer" item, the
/// frontend's own Quit button, and (by the platform default we don't
/// override) Cmd+Q - quitting must actually stop the watcher, not hide it.
#[tauri::command]
fn quit_app(app: AppHandle) {
    app.exit(0);
}

#[tauri::command]
fn is_autostart_enabled(app: AppHandle) -> Result<bool, String> {
    app.autolaunch().is_enabled().map_err(to_err)
}

/// Must only ever be called in response to the user's own choice - launch at
/// login defaults to off and nothing in this codebase may turn it on itself.
#[tauri::command]
fn set_autostart(enabled: bool, app: AppHandle) -> Result<(), String> {
    let autolaunch = app.autolaunch();
    if enabled {
        autolaunch.enable().map_err(to_err)
    } else {
        autolaunch.disable().map_err(to_err)
    }
}

#[tauri::command]
fn list_tracked_files(
    vault_id: i64,
    state: State<'_, AppState>,
) -> Result<Vec<db::TrackedFile>, String> {
    let conn = lock(&state)?;
    db::list_tracked_files(&conn, vault_id).map_err(to_err)
}

/// Lists which parts of a file differ between two versions.
///
/// Cheap by construction: both versions already carry a hash per part, so this
/// is a set comparison rather than a diff of any file contents.
#[tauri::command]
fn compare_revisions(
    older_id: i64,
    newer_id: i64,
    state: State<'_, AppState>,
) -> Result<db::Comparison, String> {
    let conn = lock(&state)?;
    db::compare_revisions(&conn, older_id, newer_id).map_err(to_err)
}

/// Backs the line that makes the point of the whole app visible: how much
/// history there is, versus what it costs to keep.
#[tauri::command]
fn get_file_stats(file_id: i64, state: State<'_, AppState>) -> Result<db::FileStats, String> {
    let conn = lock(&state)?;
    db::file_stats(&conn, file_id).map_err(to_err)
}

#[tauri::command]
fn get_timeline(file_id: i64, state: State<'_, AppState>) -> Result<Vec<db::Revision>, String> {
    let conn = lock(&state)?;
    db::get_timeline(&conn, file_id).map_err(to_err)
}

/// Where this device's identity.json lives - beside tracer.db, never in it
/// (see `identity` module docs).
fn identity_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app.path().app_data_dir().map_err(to_err)?.join("identity.json"))
}

#[tauri::command]
fn get_identity(app: AppHandle) -> Result<identity::Identity, String> {
    Ok(identity::read(&identity_path(&app)?))
}

#[tauri::command]
fn set_identity(name: String, email: String, app: AppHandle) -> Result<(), String> {
    identity::write(&identity_path(&app)?, &name, &email)
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
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        // Registering this never turns autostart on - it only makes the
        // is_autostart_enabled/set_autostart commands available. It stays
        // off until the user flips it on themselves.
        .plugin(tauri_plugin_autostart::Builder::new().build())
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // Closing the window must keep the watcher running in the
                // background - only the tray's Quit item, Cmd+Q, or the
                // frontend's Quit button may actually end the process.
                api.prevent_close();
                let _ = window.hide();
            }
        })
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
            let vaults = db::list_vaults(&conn)?;

            // A vault whose `.tracer` folder is missing at startup must not be
            // watched until it exists again - deleting it is how tracking stops.
            let active_paths: Vec<PathBuf> = vaults
                .iter()
                .filter(|v| v.is_active && v.has_tracer_folder)
                .map(|v| PathBuf::from(&v.root_path))
                .collect();
            let watched_count = active_paths.len();

            let shared_conn = Arc::new(Mutex::new(conn));

            let emitter = app.handle().clone();
            let watch = watcher::spawn(
                active_paths,
                Arc::clone(&shared_conn),
                objects_root.clone(),
                data_dir.join("identity.json"),
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

            let status_item = MenuItemBuilder::with_id("status", watch_status_label(watched_count))
                .enabled(false)
                .build(app)?;
            let open_item = MenuItemBuilder::with_id("open", "Open Tracer").build(app)?;
            // The label states the consequence - this app is deliberately not
            // hard to quit, and the wording says so up front.
            let quit_item =
                MenuItemBuilder::with_id("quit", "Quit Tracer (stops watching)").build(app)?;
            let separator = PredefinedMenuItem::separator(app)?;

            let tray_menu = MenuBuilder::new(app)
                .items(&[&status_item, &open_item, &separator, &quit_item])
                .build()?;

            let tray_icon = app
                .default_window_icon()
                .cloned()
                .expect("tauri.conf.json must configure a bundle icon for the tray to reuse");

            let tray = TrayIconBuilder::new()
                .icon(tray_icon)
                .menu(&tray_menu)
                .show_menu_on_left_click(true)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "open" => show_main_window(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;

            app.manage(AppState {
                conn: shared_conn,
                objects_root,
                watch,
                tray_status: status_item,
                tray,
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_vaults,
            add_vault,
            list_tracked_files,
            get_timeline,
            get_file_stats,
            compare_revisions,
            set_milestone,
            save_copy,
            quit_app,
            is_autostart_enabled,
            set_autostart,
            get_identity,
            set_identity
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app_handle, event| match event {
        // The Dock icon stays on purpose (no ActivationPolicy::Accessory) so
        // it's obvious Tracer is alive; clicking it must bring the window back.
        tauri::RunEvent::Reopen { .. } => show_main_window(app_handle),
        // Deliberately a no-op: a real quit (Cmd+Q, the tray's Quit item, or
        // quit_app) must still exit the process. Only a window close is
        // intercepted, and that happens in on_window_event above.
        tauri::RunEvent::ExitRequested { .. } => {}
        _ => {}
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch() -> tempfile::TempDir {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/test-tmp");
        std::fs::create_dir_all(&root).expect("creating test scratch directory");
        tempfile::TempDir::new_in(root).expect("creating scratch directory")
    }

    #[test]
    fn the_tray_status_line_counts_folders_grammatically() {
        assert_eq!(watch_status_label(0), "Not watching any folders");
        assert_eq!(watch_status_label(1), "Watching 1 folder");
        assert_eq!(watch_status_label(2), "Watching 2 folders");
    }

    #[test]
    fn adding_a_folder_leaves_an_explanation_of_how_to_stop() {
        let folder = scratch();

        create_tracer_marker(folder.path()).expect("creating the marker");

        let marker = folder.path().join(".tracer");
        assert!(marker.is_dir(), "the marker folder gates all tracking");

        let readme = std::fs::read_to_string(marker.join("README.txt")).expect("reading README");
        assert!(
            readme.contains("delete this .tracer folder"),
            "the README is the only place someone browsing the folder learns how to opt out"
        );
        assert!(
            !readme.to_lowercase().contains("commit") && !readme.to_lowercase().contains("repositor"),
            "no version-control jargon: {readme}"
        );
    }

    #[test]
    fn re_adding_a_folder_restores_a_deleted_readme() {
        let folder = scratch();

        create_tracer_marker(folder.path()).expect("first add");
        std::fs::remove_file(folder.path().join(".tracer/README.txt")).expect("removing README");

        create_tracer_marker(folder.path()).expect("re-adding must not fail on an existing folder");
        assert!(folder.path().join(".tracer/README.txt").is_file());
    }
}
