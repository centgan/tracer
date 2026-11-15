use serde::{Deserialize, Serialize};
use std::path::Path;

/// The operator's name and email, written once per device. Per
/// project_plan.md's journal model, identity is never synced and `tracer.db`
/// is a rebuildable cache - so this is not a database row, it's a plain JSON
/// file living beside it in the app-data directory.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Identity {
    pub name: String,
    pub email: String,
}

/// Missing or unreadable file -> an empty identity, never an error. A first
/// run must not fail just because the user hasn't filled this in yet.
pub fn read(path: &Path) -> Identity {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|contents| serde_json::from_str(&contents).ok())
        .unwrap_or_default()
}

/// Validates and writes atomically (temp file, then rename) so a crash
/// mid-write can't leave truncated JSON that reads as corrupt forever.
pub fn write(path: &Path, name: &str, email: &str) -> Result<(), String> {
    let name = name.trim().to_string();
    let email = email.trim().to_string();
    if !email.is_empty() && !email.contains('@') {
        return Err("email must contain @".to_string());
    }

    let json = serde_json::to_string_pretty(&Identity { name, email }).map_err(|e| e.to_string())?;
    let tmp_path = path.with_extension("json.tmp");
    std::fs::write(&tmp_path, json).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp_path, path).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_path(name: &str) -> std::path::PathBuf {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/test-tmp");
        std::fs::create_dir_all(&root).expect("creating test scratch directory");
        root.join(format!("identity-test-{name}-{}.json", std::process::id()))
    }

    #[test]
    fn missing_file_yields_empty_identity_not_an_error() {
        let path = scratch_path("missing");
        let _ = std::fs::remove_file(&path);

        let identity = read(&path);
        assert_eq!(identity.name, "");
        assert_eq!(identity.email, "");
    }

    #[test]
    fn identity_round_trips_through_the_file() {
        let path = scratch_path("roundtrip");

        write(&path, "  Eric Feng  ", "  eric@example.com  ").unwrap();

        let identity = read(&path);
        assert_eq!(identity.name, "Eric Feng");
        assert_eq!(identity.email, "eric@example.com");
    }

    #[test]
    fn set_identity_rejects_a_non_empty_email_with_no_at_sign() {
        let path = scratch_path("bad-email");

        let err = write(&path, "Eric", "not-an-email").unwrap_err();
        assert!(err.contains('@'));
        assert!(!path.exists(), "a rejected write must not touch the file");
    }

    #[test]
    fn set_identity_accepts_an_empty_email() {
        let path = scratch_path("empty-email");

        write(&path, "Eric", "").unwrap();
        assert_eq!(read(&path).email, "");
    }
}
