use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::db::{ObjectRecord, PartRecord};

/// First byte of every object file on disk: `0` means the rest of the file is
/// the raw entry bytes, `1` means the rest is zlib-compressed. `restore` only
/// has the `PartRecord`/`ObjectRecord` the DB hands back, neither of which is
/// guaranteed to be the one produced alongside the object (the DB is a
/// rebuildable cache), so the object file has to describe its own encoding
/// rather than relying on a `compressed` flag passed in from outside.
/// `ObjectRecord::stored_size` includes this byte.
const RAW_PREFIX: u8 = 0;
const ZLIB_PREFIX: u8 = 1;

pub struct Snapshot {
    pub parts: Vec<PartRecord>,
    pub objects: Vec<ObjectRecord>,
    pub manifest_hash: String,
    pub byte_size: i64,
}

/// Snapshots an Office file into content-addressed parts.
///
/// Each zip entry's uncompressed bytes are hashed and stored once under that
/// hash; `parts` records how to reassemble the archive, `objects` describes
/// every distinct blob this revision references (whether or not this call was
/// the one that wrote it, so the DB's `objects` table can always satisfy
/// `revision_parts`' foreign key), and `manifest_hash` identifies this exact
/// set of parts so callers can skip storing a revision that didn't change.
pub fn snapshot(path: &Path, objects_root: &Path) -> Result<Snapshot> {
    let byte_size = fs::metadata(path)
        .with_context(|| format!("reading metadata for {}", path.display()))?
        .len() as i64;

    let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut archive =
        ZipArchive::new(file).with_context(|| format!("reading zip archive {}", path.display()))?;

    let mut parts = Vec::new();
    let mut objects = Vec::new();
    let mut seen_objects = HashSet::new();
    let mut manifest = blake3::Hasher::new();
    let mut entry_order = 0i64;

    for i in 0..archive.len() {
        let mut f = archive
            .by_index(i)
            .with_context(|| format!("reading entry {i} of {}", path.display()))?;

        // shortcut: directory entries are dropped, Office writes none and ignores them on open; revisit if we ever track hand-zipped archives
        if f.is_dir() {
            continue;
        }

        let name = f.name().to_string();
        let compression_method = compression_to_name(f.compression())?.to_string();

        let mut bytes = Vec::new();
        f.read_to_end(&mut bytes)
            .with_context(|| format!("reading entry {name} of {}", path.display()))?;

        let object_hash = blake3::hash(&bytes).to_hex().to_string();

        let record = store_object(objects_root, &object_hash, &bytes)?;
        if seen_objects.insert(object_hash.clone()) {
            objects.push(record);
        }

        manifest.update(entry_order.to_string().as_bytes());
        manifest.update(b"\0");
        manifest.update(name.as_bytes());
        manifest.update(b"\0");
        manifest.update(object_hash.as_bytes());
        manifest.update(b"\n");

        parts.push(PartRecord {
            entry_order,
            entry_name: name,
            compression_method,
            object_hash,
        });

        entry_order += 1;
    }

    Ok(Snapshot {
        parts,
        objects,
        manifest_hash: manifest.finalize().to_hex().to_string(),
        byte_size,
    })
}

/// Rebuilds a zip archive from stored parts, in `entry_order`.
pub fn restore(parts: &[PartRecord], objects_root: &Path, dest: &Path) -> Result<()> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating parent directory for {}", dest.display()))?;
    }

    let mut ordered: Vec<&PartRecord> = parts.iter().collect();
    ordered.sort_by_key(|p| p.entry_order);

    let mut seen_names = HashSet::new();
    for part in &ordered {
        if !seen_names.insert(part.entry_name.as_str()) {
            bail!("revision has duplicate entry name: {}", part.entry_name);
        }
    }

    let file = File::create(dest).with_context(|| format!("creating {}", dest.display()))?;
    // Never new_stream (sets the data-descriptor bit) and never large_file(true)
    // (forces zip64 extra fields) - both make PowerPoint show a repair dialog.
    let mut writer = ZipWriter::new(file);

    for part in ordered {
        let bytes = load_object(objects_root, &part.object_hash).with_context(|| {
            format!(
                "loading object {} for entry {}",
                part.object_hash, part.entry_name
            )
        })?;

        let method = compression_from_name(&part.compression_method)?;
        // DEFAULT (the const), not default(): default() stamps the current time
        // and makes restores non-deterministic. DEFAULT is 1980-01-01, matching
        // real Office files.
        let options = SimpleFileOptions::DEFAULT.compression_method(method);

        writer
            .start_file(&part.entry_name, options)
            .with_context(|| format!("starting zip entry {}", part.entry_name))?;
        writer
            .write_all(&bytes)
            .with_context(|| format!("writing zip entry {}", part.entry_name))?;
    }

    writer.finish().context("finishing zip archive")?;

    Ok(())
}

fn object_path(objects_root: &Path, hash: &str) -> PathBuf {
    objects_root.join(&hash[..2]).join(&hash[2..])
}

fn compression_to_name(method: CompressionMethod) -> Result<&'static str> {
    match method {
        CompressionMethod::Stored => Ok("stored"),
        CompressionMethod::Deflated => Ok("deflated"),
        other => bail!("unsupported compression method: {other:?}"),
    }
}

fn compression_from_name(name: &str) -> Result<CompressionMethod> {
    match name {
        "stored" => Ok(CompressionMethod::Stored),
        "deflated" => Ok(CompressionMethod::Deflated),
        other => bail!("unsupported compression method in stored part: {other}"),
    }
}

/// Writes `bytes` under `hash` unless it's already on disk, and describes the
/// object either way.
///
/// File existence is the dedupe check rather than a DB lookup, because the DB
/// is a derived cache that can be deleted and rebuilt from the object store.
/// But the record is returned even when nothing was written: `revision_parts`
/// has a foreign key onto `objects`, so a revision whose parts are all already
/// stored would otherwise insert rows pointing at absent `objects` rows and
/// fail the constraint. That happens whenever the DB is newer than the object
/// store - a deleted `tracer.db`, a fresh install over an existing store, or an
/// `insert_revision` that rolled back after the blobs were already on disk.
/// `insert_revision` uses INSERT OR IGNORE, so re-describing a known object is
/// free.
fn store_object(objects_root: &Path, hash: &str, bytes: &[u8]) -> Result<ObjectRecord> {
    let dest = object_path(objects_root, hash);
    if dest.exists() {
        return describe_stored_object(&dest, hash, bytes.len() as i64);
    }

    let shard_dir = dest.parent().expect("object path always has a shard parent");
    fs::create_dir_all(shard_dir)
        .with_context(|| format!("creating object shard directory {}", shard_dir.display()))?;

    let mut compressed = Vec::new();
    {
        let mut encoder = ZlibEncoder::new(&mut compressed, Compression::default());
        encoder
            .write_all(bytes)
            .context("zlib-compressing object")?;
        encoder.finish().context("finishing zlib stream")?;
    }

    let (prefix, payload, is_compressed) = if compressed.len() < bytes.len() {
        (ZLIB_PREFIX, compressed.as_slice(), true)
    } else {
        (RAW_PREFIX, bytes, false)
    };

    // Write to a temp file in the same directory and persist (atomic rename),
    // so a crash mid-write can never leave a truncated blob under a hash that
    // claims to describe it.
    let mut tmp = tempfile::NamedTempFile::new_in(shard_dir)
        .with_context(|| format!("creating temp file in {}", shard_dir.display()))?;
    tmp.write_all(&[prefix])
        .context("writing object prefix byte")?;
    tmp.write_all(payload).context("writing object payload")?;
    tmp.flush().context("flushing object temp file")?;

    if let Err(e) = tmp.persist(&dest) {
        // Another writer may have raced us to the same content-addressed path;
        // since the path is addressed by content, whatever is there is
        // byte-identical, so that's success, not a failure.
        if !dest.exists() {
            return Err(e.error).with_context(|| format!("persisting object {}", dest.display()));
        }
    }

    Ok(ObjectRecord {
        hash: hash.to_string(),
        size: bytes.len() as i64,
        stored_size: (payload.len() + 1) as i64,
        compressed: is_compressed,
    })
}

/// Describes an object already on disk, reading only its length and the
/// one-byte encoding prefix rather than re-compressing the payload.
fn describe_stored_object(dest: &Path, hash: &str, size: i64) -> Result<ObjectRecord> {
    let stored_size = fs::metadata(dest)
        .with_context(|| format!("sizing stored object {}", dest.display()))?
        .len() as i64;

    let mut prefix = [0u8; 1];
    File::open(dest)
        .with_context(|| format!("opening stored object {}", dest.display()))?
        .read_exact(&mut prefix)
        .with_context(|| format!("reading prefix of stored object {}", dest.display()))?;

    let compressed = match prefix[0] {
        RAW_PREFIX => false,
        ZLIB_PREFIX => true,
        other => bail!("object {} has unknown storage prefix {other}", dest.display()),
    };

    Ok(ObjectRecord {
        hash: hash.to_string(),
        size,
        stored_size,
        compressed,
    })
}

fn load_object(objects_root: &Path, hash: &str) -> Result<Vec<u8>> {
    let path = object_path(objects_root, hash);
    let mut raw = Vec::new();
    File::open(&path)
        .with_context(|| format!("opening object {}", path.display()))?
        .read_to_end(&mut raw)
        .with_context(|| format!("reading object {}", path.display()))?;

    let (prefix, payload) = raw
        .split_first()
        .ok_or_else(|| anyhow::anyhow!("object {} is empty", path.display()))?;

    let bytes = match *prefix {
        RAW_PREFIX => payload.to_vec(),
        ZLIB_PREFIX => {
            let mut decoder = ZlibDecoder::new(payload);
            let mut out = Vec::new();
            decoder
                .read_to_end(&mut out)
                .with_context(|| format!("inflating object {}", path.display()))?;
            out
        }
        other => bail!("object {} has unknown storage prefix {other}", path.display()),
    };

    let actual_hash = blake3::hash(&bytes).to_hex().to_string();
    if actual_hash != hash {
        bail!(
            "object {} is corrupt: expected hash {hash}, got {actual_hash}",
            path.display()
        );
    }

    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// A small archive mixing Stored and Deflated entries, a name containing
    /// `/`, and two entries with identical content (to prove object dedupe).
    fn build_test_zip(path: &Path) {
        let file = File::create(path).unwrap();
        let mut writer = ZipWriter::new(file);

        let stored = SimpleFileOptions::DEFAULT.compression_method(CompressionMethod::Stored);
        let deflated = SimpleFileOptions::DEFAULT.compression_method(CompressionMethod::Deflated);

        writer.start_file("[Content_Types].xml", deflated).unwrap();
        writer.write_all(b"<Types/>").unwrap();

        writer.start_file("docProps/core.xml", stored).unwrap();
        writer.write_all(b"shared-bytes").unwrap();

        writer.start_file("word/document.xml", deflated).unwrap();
        writer.write_all(b"<document/>").unwrap();

        writer.start_file("word/media/dup.bin", stored).unwrap();
        writer.write_all(b"shared-bytes").unwrap();

        writer.finish().unwrap();
    }

    /// Round-trips a genuine Office file, which a synthetic zip cannot stand in
    /// for: only a real archive has the mixed Stored/Deflated entries, the
    /// 1980-01-01 timestamps and the ~180 parts that a deck actually contains.
    /// Reads every Office file in the repo's `test-data/` folder; see the README
    /// there. Ignored by default so a normal `cargo test` stays fast.
    ///
    /// `cargo test --lib real_office_file_round_trips -- --ignored --nocapture`
    ///
    /// Prints each restored path so it can be opened in PowerPoint to confirm no
    /// repair dialog, which is the one thing no assertion here proves.
    #[test]
    #[ignore]
    fn real_office_file_round_trips() {
        let test_data = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("src-tauri always has a parent")
            .join("test-data");

        let mut sources: Vec<PathBuf> = fs::read_dir(&test_data)
            .unwrap_or_else(|e| panic!("reading {}: {e}", test_data.display()))
            .filter_map(|entry| {
                let path = entry.ok()?.path();
                let ext = path.extension()?.to_string_lossy().to_lowercase();
                matches!(ext.as_str(), "pptx" | "docx" | "xlsx").then_some(path)
            })
            .collect();
        sources.sort();

        assert!(
            !sources.is_empty(),
            "no .pptx/.docx/.xlsx files in {} - see its README",
            test_data.display()
        );

        for source in sources {
            round_trip_one_real_file(&source);
        }
    }

    fn round_trip_one_real_file(source: &Path) {
        let objects_root = tempdir().unwrap();
        let out_dir = tempdir().unwrap();

        let snap = snapshot(&source, objects_root.path()).expect("snapshot real Office file");
        assert!(snap.parts.len() > 1, "a real Office file has many parts");

        let dest = out_dir.path().join("restored.zip");
        restore(&snap.parts, objects_root.path(), &dest).expect("restore real Office file");

        let mut original = ZipArchive::new(File::open(&source).unwrap()).unwrap();
        let mut rebuilt = ZipArchive::new(File::open(&dest).unwrap()).unwrap();

        let mut original_names = Vec::new();
        for i in 0..original.len() {
            let mut entry = original.by_index(i).unwrap();
            if entry.is_dir() {
                continue;
            }
            let name = entry.name().to_string();
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).unwrap();

            let mut restored_bytes = Vec::new();
            rebuilt
                .by_name(&name)
                .unwrap_or_else(|_| panic!("restored archive is missing {name}"))
                .read_to_end(&mut restored_bytes)
                .unwrap();

            assert_eq!(bytes, restored_bytes, "part {name} differs after restore");
            original_names.push(name);
        }

        let rebuilt_names: Vec<String> = (0..rebuilt.len())
            .map(|i| rebuilt.by_index(i).unwrap().name().to_string())
            .collect();
        assert_eq!(original_names, rebuilt_names, "entry order changed");

        // Leaked deliberately, and under the original extension: the whole point
        // is for a human to double-click this and see whether Office complains.
        let name = source.file_name().expect("fixture has a file name");
        let keep = std::env::temp_dir().join(format!("tracer_restored_{}", name.to_string_lossy()));
        fs::copy(&dest, &keep).unwrap();
        println!(
            "{}: restored {} parts; open this and check for a repair dialog:\n  {}",
            name.to_string_lossy(),
            snap.parts.len(),
            keep.display()
        );
    }

    fn count_files(dir: &Path) -> usize {
        let mut count = 0;
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                count += count_files(&path);
            } else {
                count += 1;
            }
        }
        count
    }

    #[test]
    fn round_trip_preserves_entry_order_names_and_bytes() {
        let src_dir = tempdir().unwrap();
        let objects_root = tempdir().unwrap();
        let src = src_dir.path().join("src.zip");
        build_test_zip(&src);

        let snap = snapshot(&src, objects_root.path()).unwrap();

        let dest = src_dir.path().join("restored.zip");
        restore(&snap.parts, objects_root.path(), &dest).unwrap();

        let mut restored = ZipArchive::new(File::open(&dest).unwrap()).unwrap();
        assert_eq!(restored.len(), snap.parts.len());

        for part in &snap.parts {
            let idx = part.entry_order as usize;
            let mut r = restored.by_index(idx).unwrap();
            assert_eq!(r.name(), part.entry_name);

            let mut got = Vec::new();
            r.read_to_end(&mut got).unwrap();
            drop(r);

            let mut r_by_name = restored.by_name(&part.entry_name).unwrap();
            let mut expected = Vec::new();
            r_by_name.read_to_end(&mut expected).unwrap();

            assert_eq!(got, expected);
        }
    }

    #[test]
    fn identical_content_stores_one_object() {
        let src_dir = tempdir().unwrap();
        let objects_root = tempdir().unwrap();
        let src = src_dir.path().join("src.zip");
        build_test_zip(&src);

        let snap = snapshot(&src, objects_root.path()).unwrap();

        let dup_hash = blake3::hash(b"shared-bytes").to_hex().to_string();

        let matching_objects = snap.objects.iter().filter(|o| o.hash == dup_hash).count();
        assert_eq!(matching_objects, 1);

        let matching_parts = snap
            .parts
            .iter()
            .filter(|p| p.object_hash == dup_hash)
            .count();
        assert_eq!(matching_parts, 2);

        let object_file = object_path(objects_root.path(), &dup_hash);
        assert!(object_file.is_file());
    }

    #[test]
    fn second_snapshot_of_unchanged_file_writes_no_new_blobs() {
        let src_dir = tempdir().unwrap();
        let objects_root = tempdir().unwrap();
        let src = src_dir.path().join("src.zip");
        build_test_zip(&src);

        let first = snapshot(&src, objects_root.path()).unwrap();
        let count_after_first = count_files(objects_root.path());

        let second = snapshot(&src, objects_root.path()).unwrap();
        let count_after_second = count_files(objects_root.path());

        assert_eq!(first.manifest_hash, second.manifest_hash);
        assert_eq!(count_after_first, count_after_second);

        // Nothing new is written, but every object is still described, or a
        // revision inserted now would violate revision_parts' foreign key.
        assert_eq!(second.objects.len(), first.objects.len());
        assert_eq!(
            second.objects.iter().map(|o| &o.hash).collect::<HashSet<_>>(),
            first.objects.iter().map(|o| &o.hash).collect::<HashSet<_>>()
        );
        for object in &second.objects {
            assert!(object.size > 0);
            assert!(object.stored_size > 0);
        }
    }

    /// The regression this guards: with the object store already populated and
    /// a brand-new database, every part's object is already on disk, so a
    /// snapshot that reported only newly-written blobs would insert
    /// revision_parts rows referencing absent objects rows and fail the
    /// foreign key - permanently, for every future save.
    #[test]
    fn revision_inserts_when_the_object_store_predates_the_database() {
        use crate::db;

        let src_dir = tempdir().unwrap();
        let objects_root = tempdir().unwrap();
        let src = src_dir.path().join("deck.pptx");
        build_test_zip(&src);

        // First snapshot populates the object store; its DB is then discarded.
        snapshot(&src, objects_root.path()).unwrap();

        let db_file = src_dir.path().join("fresh.db");
        let mut conn = db::init_db(&db_file).unwrap();
        let vault = db::add_vault(&conn, src_dir.path().to_str().unwrap()).unwrap();
        let file_id = db::upsert_file(&conn, vault.id, "deck.pptx", "pptx").unwrap();

        let snap = snapshot(&src, objects_root.path()).unwrap();
        let revision_id = db::insert_revision(
            &mut conn,
            file_id,
            1000,
            snap.byte_size,
            &snap.manifest_hash,
            &snap.parts,
            &snap.objects,
        )
        .expect("inserting a revision whose objects are all already on disk");

        assert_eq!(
            db::get_revision_parts(&conn, revision_id).unwrap().len(),
            snap.parts.len()
        );
    }

    #[test]
    fn objects_are_sharded_two_chars_deep() {
        let src_dir = tempdir().unwrap();
        let objects_root = tempdir().unwrap();
        let src = src_dir.path().join("src.zip");
        build_test_zip(&src);

        let snap = snapshot(&src, objects_root.path()).unwrap();
        let object = snap.objects.first().expect("at least one object stored");

        let full = object_path(objects_root.path(), &object.hash);
        assert!(full.is_file());

        let rel = full.strip_prefix(objects_root.path()).unwrap();
        let mut components = rel.components();
        let shard = components.next().unwrap().as_os_str().to_str().unwrap();
        let rest = components.next().unwrap().as_os_str().to_str().unwrap();
        assert!(components.next().is_none());
        assert_eq!(shard.len(), 2);
        assert_eq!(rest.len(), 62);
    }

    #[test]
    fn restore_is_deterministic() {
        let src_dir = tempdir().unwrap();
        let objects_root = tempdir().unwrap();
        let src = src_dir.path().join("src.zip");
        build_test_zip(&src);

        let snap = snapshot(&src, objects_root.path()).unwrap();

        let dest_a = src_dir.path().join("a.zip");
        let dest_b = src_dir.path().join("b.zip");
        restore(&snap.parts, objects_root.path(), &dest_a).unwrap();
        restore(&snap.parts, objects_root.path(), &dest_b).unwrap();

        assert_eq!(fs::read(&dest_a).unwrap(), fs::read(&dest_b).unwrap());
    }

    #[test]
    fn unsupported_compression_method_is_an_error() {
        let src_dir = tempdir().unwrap();
        let objects_root = tempdir().unwrap();
        let src = src_dir.path().join("bzip2.zip");

        let file = File::create(&src).unwrap();
        let mut writer = ZipWriter::new(file);
        let options = SimpleFileOptions::DEFAULT.compression_method(CompressionMethod::Bzip2);
        writer.start_file("entry.xml", options).unwrap();
        writer.write_all(b"data").unwrap();
        writer.finish().unwrap();

        let result = snapshot(&src, objects_root.path());
        assert!(result.is_err());
    }

    #[test]
    fn corrupt_object_is_detected() {
        let src_dir = tempdir().unwrap();
        let objects_root = tempdir().unwrap();
        let src = src_dir.path().join("src.zip");
        build_test_zip(&src);

        let snap = snapshot(&src, objects_root.path()).unwrap();
        let object = snap.objects.first().expect("at least one object stored");
        let object_file = object_path(objects_root.path(), &object.hash);

        let mut bytes = fs::read(&object_file).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 0xFF;
        fs::write(&object_file, bytes).unwrap();

        let dest = src_dir.path().join("restored.zip");
        let result = restore(&snap.parts, objects_root.path(), &dest);
        assert!(result.is_err());
    }
}
