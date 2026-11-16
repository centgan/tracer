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

/// Entry-name prefixes left out of `manifest_hash` because Office rewrites
/// them on every save regardless of whether the user changed anything (the
/// revision counter, the modified timestamp, total editing time), which would
/// otherwise make the change-detection guard in `watcher::process_path` fire
/// on every save and record a version with no real content change. Extend this
/// list only with other parts proven to be pure save-metadata - excluding a
/// real content part would hide an actual edit, which is worse than recording
/// an extra version.
const MANIFEST_HASH_EXCLUDED_PREFIXES: &[&str] = &["docProps/"];

fn is_excluded_from_manifest_hash(entry_name: &str) -> bool {
    MANIFEST_HASH_EXCLUDED_PREFIXES
        .iter()
        .any(|prefix| entry_name.starts_with(prefix))
}

pub struct Snapshot {
    pub parts: Vec<PartRecord>,
    pub objects: Vec<ObjectRecord>,
    /// Identifies this revision's content for change detection, not restore
    /// (`parts` does that). Covers only parts outside
    /// `MANIFEST_HASH_EXCLUDED_PREFIXES`, so a save that only touched
    /// Office's own save-metadata hashes the same as the save before it.
    pub manifest_hash: String,
    pub byte_size: i64,
}

/// Snapshots an Office file into content-addressed parts.
///
/// Each zip entry's uncompressed bytes are hashed and stored once under that
/// hash; `parts` records how to reassemble the archive, `objects` describes
/// every distinct blob this revision references (whether or not this call was
/// the one that wrote it, so the DB's `objects` table can always satisfy
/// `revision_parts`' foreign key), and `manifest_hash` identifies this
/// revision's content parts (see `MANIFEST_HASH_EXCLUDED_PREFIXES`) so callers
/// can skip storing a revision whose content didn't change.
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

        // Deliberately not hashing entry_order: it counts every part, excluded
        // ones included, so a single new docProps entry would shift the index of
        // every content part and change this hash without any content changing -
        // the exact false version this exclusion exists to prevent. Feeding the
        // parts in sequence already encodes their order.
        if !is_excluded_from_manifest_hash(&name) {
            manifest.update(name.as_bytes());
            manifest.update(b"\0");
            manifest.update(object_hash.as_bytes());
            manifest.update(b"\n");
        }

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

pub fn load_object(objects_root: &Path, hash: &str) -> Result<Vec<u8>> {
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

// ---------------------------------------------------------------------------
// Text extraction and comparison for "compare".
//
// project_plan.md's Diffing section is explicit about the trap: raw-diffing
// the XML shows 400 changed lines for a one-word edit, because Office
// reshuffles the markup on every save. So these functions pull just the text
// runs out of a part (never touching layout, formatting or ids) and diff
// *that*. No XML parser dependency - splitting on the paragraph tag and
// scanning for run tags is the standard approach for OOXML text.
// ---------------------------------------------------------------------------

/// Finds every top-level `<tag ...>` occurrence (self-closing or not) in
/// `xml`, in document order, returning each one's attribute text (between
/// the tag name and the closing `>`/`/>`) paired with its inner text
/// (`None` for a self-closing tag, which has none).
///
/// Tag-boundary aware, so asking for `"a:p"` never matches `"a:pPr"`. The one
/// scanner behind both `tag_spans` (inner text, for paragraphs and runs) and
/// `tag_attrs` (attribute text, for properties like `<w:sz w:val="24"/>`
/// whose value lives on the tag itself, not in its content).
fn tag_occurrences<'a>(xml: &'a str, tag: &str) -> Vec<(&'a str, Option<&'a str>)> {
    let open_prefix = format!("<{tag}");
    let close = format!("</{tag}>");
    let mut out = Vec::new();
    let mut offset = 0usize;

    while let Some(start_rel) = xml[offset..].find(open_prefix.as_str()) {
        let start = offset + start_rel;
        let after = &xml[start + open_prefix.len()..];

        let is_boundary = after.starts_with(|c: char| c == '>' || c == '/' || c.is_whitespace());
        if !is_boundary {
            // Matched a longer tag name sharing this prefix (e.g. "a:pPr"
            // while looking for "a:p") - keep scanning past it.
            offset = start + 1;
            continue;
        }

        let Some(gt_rel) = after.find('>') else {
            break;
        };
        let trimmed = after[..gt_rel].trim_end();
        let is_self_closing = trimmed.ends_with('/');
        let attrs = if is_self_closing { &trimmed[..trimmed.len() - 1] } else { trimmed };
        let content_start = start + open_prefix.len() + gt_rel + 1;

        if is_self_closing {
            out.push((attrs.trim(), None));
            offset = content_start;
            continue;
        }

        let Some(close_rel) = xml[content_start..].find(close.as_str()) else {
            break;
        };
        let content_end = content_start + close_rel;
        out.push((attrs.trim(), Some(&xml[content_start..content_end])));
        offset = content_end + close.len();
    }

    out
}

/// Every top-level `<tag ...>...</tag>` (or self-closing `<tag .../>`,
/// whose content is empty) in `xml`, as raw (undecoded) inner text.
fn tag_spans<'a>(xml: &'a str, tag: &str) -> Vec<&'a str> {
    tag_occurrences(xml, tag)
        .into_iter()
        .map(|(_, content)| content.unwrap_or(""))
        .collect()
}

/// Every top-level `<tag ...>` occurrence's attribute text, for properties
/// that live as attributes on the tag itself (`<w:sz w:val="24"/>`,
/// `<a:rPr b="1" sz="1800">`) rather than in its content.
fn tag_attrs<'a>(xml: &'a str, tag: &str) -> Vec<&'a str> {
    tag_occurrences(xml, tag).into_iter().map(|(attrs, _)| attrs).collect()
}

/// One `attr="value"` lookup within a tag's attribute text, decoded.
fn attr_value(attrs: &str, attr: &str) -> Option<String> {
    let needle = format!("{attr}=\"");
    let start = attrs.find(needle.as_str())? + needle.len();
    let end = attrs[start..].find('"')?;
    Some(decode_entities(&attrs[start..start + end]))
}

/// Decodes the five predefined XML entities. `&amp;` is decoded last so a
/// literal escaped entity (`&amp;lt;`, meaning the text `&lt;`) round-trips
/// instead of being double-unescaped.
fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// One run of text sharing one set of formatting, as the file itself stores
/// it. A missing property (`None`/`false`) means inherited/default - never
/// invented.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub size_pt: Option<f32>,
    pub font: Option<String>,
    /// "RRGGBB", as the file stores it.
    pub color: Option<String>,
}

impl Run {
    fn plain(text: String) -> Run {
        Run {
            text,
            bold: false,
            italic: false,
            underline: false,
            size_pt: None,
            font: None,
            color: None,
        }
    }
}

/// One paragraph, as the runs that make it up.
#[derive(Clone, Debug, PartialEq)]
pub struct Paragraph {
    pub runs: Vec<Run>,
}

impl Paragraph {
    /// Concatenated run text - what the existing line-level diffing works
    /// off, same as the flat `String` lines this type replaced.
    pub fn text(&self) -> String {
        self.runs.iter().map(|r| r.text.as_str()).collect()
    }
}

/// A run's formatting with every field still optional, so "this level didn't
/// say" (`None`, fall through to an outer default) stays distinguishable
/// from "this level said off" (`Some(false)`) until the final merge.
#[derive(Clone, Debug, Default)]
struct RunProps {
    bold: Option<bool>,
    italic: Option<bool>,
    underline: Option<bool>,
    size_pt: Option<f32>,
    font: Option<String>,
    color: Option<String>,
}

impl RunProps {
    /// `self` (a run's own properties) wins; `default` (the paragraph's)
    /// fills whatever `self` left unset.
    fn merged_over(&self, default: &RunProps) -> RunProps {
        RunProps {
            bold: self.bold.or(default.bold),
            italic: self.italic.or(default.italic),
            underline: self.underline.or(default.underline),
            size_pt: self.size_pt.or(default.size_pt),
            font: self.font.clone().or_else(|| default.font.clone()),
            color: self.color.clone().or_else(|| default.color.clone()),
        }
    }

    fn into_run(self, text: String) -> Run {
        Run {
            text,
            bold: self.bold.unwrap_or(false),
            italic: self.italic.unwrap_or(false),
            underline: self.underline.unwrap_or(false),
            size_pt: self.size_pt,
            font: self.font,
            color: self.color,
        }
    }
}

/// A boolean toggle element (`<w:b/>`, `<w:b w:val="0"/>`): present and on
/// unless its value says otherwise.
fn flag_is_on(attrs: &str) -> bool {
    !matches!(attr_value(attrs, "w:val").as_deref(), Some("0") | Some("false") | Some("off"))
}

/// Word's run properties: toggle elements (`<w:b/>`, `<w:i/>`, `<w:u .../>`)
/// plus `<w:sz>` (half-points), `<w:rFonts ascii="...">` and `<w:color>`.
fn parse_word_run_props(rpr: &str) -> RunProps {
    let bold = tag_attrs(rpr, "w:b").first().map(|a| flag_is_on(a));
    let italic = tag_attrs(rpr, "w:i").first().map(|a| flag_is_on(a));
    // <w:u w:val="none"/> explicitly turns underlining off; any other value
    // ("single", "double", ...) turns it on.
    let underline = tag_attrs(rpr, "w:u")
        .first()
        .map(|a| attr_value(a, "w:val").as_deref() != Some("none"));
    let size_pt = tag_attrs(rpr, "w:sz")
        .first()
        .and_then(|a| attr_value(a, "w:val"))
        .and_then(|v| v.parse::<f32>().ok())
        .map(|half_points| half_points / 2.0);
    let font = tag_attrs(rpr, "w:rFonts").first().and_then(|a| attr_value(a, "w:ascii"));
    // "auto" means "let the renderer decide", which is the same as no colour.
    let color = tag_attrs(rpr, "w:color")
        .first()
        .and_then(|a| attr_value(a, "w:val"))
        .filter(|v| v != "auto");

    RunProps { bold, italic, underline, size_pt, font, color }
}

/// PowerPoint's run properties: `b`/`i`/`u`/`sz` live as attributes on
/// `<a:rPr>` itself (hundredths of a point for size); font and colour are
/// children (`<a:latin typeface="...">`, `<a:solidFill><a:srgbClr val="...">`).
fn parse_ppt_run_props(attrs: &str, content: Option<&str>) -> RunProps {
    let bold = attr_value(attrs, "b").map(|v| v == "1");
    let italic = attr_value(attrs, "i").map(|v| v == "1");
    let underline = attr_value(attrs, "u").map(|v| v != "none");
    let size_pt = attr_value(attrs, "sz")
        .and_then(|v| v.parse::<f32>().ok())
        .map(|hundredths| hundredths / 100.0);

    let (font, color) = match content {
        None => (None, None),
        Some(content) => {
            let font = tag_attrs(content, "a:latin").first().and_then(|a| attr_value(a, "typeface"));
            let color = tag_spans(content, "a:solidFill")
                .first()
                .and_then(|fill| tag_attrs(fill, "a:srgbClr").first().and_then(|a| attr_value(a, "val")));
            (font, color)
        }
    };

    RunProps { bold, italic, underline, size_pt, font, color }
}

/// Word paragraphs (`<w:p>`/`<w:r>`/`<w:t>`), with each run's own `<w:rPr>`
/// falling back to the paragraph's default in `<w:pPr>/<w:rPr>` for whatever
/// it doesn't specify itself.
fn word_paragraphs(xml: &str) -> Vec<Paragraph> {
    tag_spans(xml, "w:p")
        .into_iter()
        .map(|p_content| {
            let default_props = tag_spans(p_content, "w:pPr")
                .first()
                .and_then(|ppr| tag_spans(ppr, "w:rPr").first().map(|rpr| parse_word_run_props(rpr)))
                .unwrap_or_default();

            let runs = tag_spans(p_content, "w:r")
                .into_iter()
                .map(|r_content| {
                    let own = tag_spans(r_content, "w:rPr")
                        .first()
                        .map(|rpr| parse_word_run_props(rpr))
                        .unwrap_or_default();
                    let text: String =
                        tag_spans(r_content, "w:t").into_iter().map(decode_entities).collect();
                    own.merged_over(&default_props).into_run(text)
                })
                .collect();

            Paragraph { runs }
        })
        .collect()
}

/// PowerPoint paragraphs (`<a:p>`/`<a:r>`/`<a:t>`). No paragraph-level
/// default to fall back to - PowerPoint's own default lives in the slide
/// layout/master, out of scope here.
fn ppt_paragraphs(xml: &str) -> Vec<Paragraph> {
    tag_spans(xml, "a:p")
        .into_iter()
        .map(|p_content| {
            let runs = tag_spans(p_content, "a:r")
                .into_iter()
                .map(|r_content| {
                    let props = match tag_occurrences(r_content, "a:rPr").into_iter().next() {
                        Some((attrs, content)) => parse_ppt_run_props(attrs, content),
                        None => RunProps::default(),
                    };
                    let text: String =
                        tag_spans(r_content, "a:t").into_iter().map(decode_entities).collect();
                    props.into_run(text)
                })
                .collect();

            Paragraph { runs }
        })
        .collect()
}

/// Pulls a part's text and formatting out in document order, one entry per
/// paragraph.
///
/// Returns an empty `Vec` for a part with no recognised text runs (including
/// one whose bytes aren't valid UTF-8) rather than guessing - the UI says
/// text comparison isn't available instead of showing nonsense.
pub fn extract_text(entry_name: &str, bytes: &[u8]) -> Vec<Paragraph> {
    let Ok(xml) = std::str::from_utf8(bytes) else {
        return Vec::new();
    };

    if entry_name.starts_with("ppt/slides/slide") || entry_name.starts_with("ppt/notesSlides/") {
        return ppt_paragraphs(xml);
    }

    let is_word_text_part = entry_name == "word/document.xml"
        || entry_name == "word/footnotes.xml"
        || entry_name == "word/endnotes.xml"
        || entry_name.starts_with("word/header")
        || entry_name.starts_with("word/footer");
    if is_word_text_part {
        return word_paragraphs(xml);
    }

    if entry_name == "xl/sharedStrings.xml" {
        // Excel's shared strings have no paragraph structure or run-level
        // formatting worth showing - each cell's text is its own unformatted
        // paragraph.
        return tag_spans(xml, "t")
            .into_iter()
            .map(decode_entities)
            .map(|text| Paragraph { runs: vec![Run::plain(text)] })
            .collect();
    }

    Vec::new()
}

/// One word-level piece of a changed line, carrying both whether it differs
/// and the formatting of the run it sits in - the frontend needs both at
/// once to mark a changed word inside (say) a bold run without losing the
/// bold.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextSpan {
    pub text: String,
    pub changed: bool,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub size_pt: Option<f32>,
    pub font: Option<String>,
    pub color: Option<String>,
}

impl TextSpan {
    fn plain(text: String, changed: bool) -> TextSpan {
        TextSpan {
            text,
            changed,
            bold: false,
            italic: false,
            underline: false,
            size_pt: None,
            font: None,
            color: None,
        }
    }
}

/// One row of a side-by-side text comparison.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextRow {
    /// "same" | "added" | "removed" | "changed" | "restyled"
    pub status: String,
    pub left: Option<String>,
    pub right: Option<String>,
    /// Word-level pieces for a "changed" row, so the exact words that differ
    /// can be marked. Empty for every other status.
    pub left_spans: Vec<TextSpan>,
    pub right_spans: Vec<TextSpan>,
    /// Document-order runs for this row's text, on every row, so the
    /// frontend can render any line - not just a "changed" one - the way the
    /// document actually looks. Empty on the side that has no text (the
    /// gone side of a "removed" row, the absent side of an "added" row).
    pub left_runs: Vec<Run>,
    pub right_runs: Vec<Run>,
    /// Plain-language summary of what changed about the formatting, e.g.
    /// "11pt → 12pt · Calibri → Georgia · now bold". Empty when nothing did.
    pub format_note: String,
}

/// A side-by-side comparison of a part's extracted text between two versions.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextComparison {
    pub rows: Vec<TextRow>,
    /// False when neither side yielded any text, so the UI can explain itself
    /// instead of showing an empty table.
    pub has_text: bool,
}

/// Word-level diff of one changed line, split into the spans that render
/// either side of it. Formatting-free - `intersect_runs_and_spans` layers
/// formatting on afterward.
fn word_spans(old: &str, new: &str) -> (Vec<TextSpan>, Vec<TextSpan>) {
    let mut left = Vec::new();
    let mut right = Vec::new();

    for change in similar::TextDiff::from_words(old, new).iter_all_changes() {
        let text = change.value().to_string();
        match change.tag() {
            similar::ChangeTag::Equal => {
                left.push(TextSpan::plain(text.clone(), false));
                right.push(TextSpan::plain(text, false));
            }
            similar::ChangeTag::Delete => left.push(TextSpan::plain(text, true)),
            similar::ChangeTag::Insert => right.push(TextSpan::plain(text, true)),
        }
    }

    (left, right)
}

/// `runs` and `spans` both partition the same string - `runs` by formatting
/// (from the file), `spans` by whether a word changed (from `word_spans`).
/// Walked character by character and re-grouped wherever either partition
/// draws a boundary, so a changed word inside a bold run comes back as a
/// piece that is both bold and marked, rather than the diff mark forcing the
/// formatting to lose a boundary or vice versa.
///
/// `runs` and `spans` must describe the same text; if they don't (lengths
/// differ), formatting is kept and change-marking is dropped rather than
/// panicking on mismatched input.
fn intersect_runs_and_spans(runs: &[Run], spans: &[TextSpan]) -> Vec<TextSpan> {
    let run_chars: Vec<(char, usize)> =
        runs.iter().enumerate().flat_map(|(i, r)| r.text.chars().map(move |c| (c, i))).collect();
    let span_chars: Vec<(char, usize)> =
        spans.iter().enumerate().flat_map(|(i, s)| s.text.chars().map(move |c| (c, i))).collect();

    if run_chars.len() != span_chars.len() {
        return runs
            .iter()
            .map(|r| TextSpan {
                text: r.text.clone(),
                changed: false,
                bold: r.bold,
                italic: r.italic,
                underline: r.underline,
                size_pt: r.size_pt,
                font: r.font.clone(),
                color: r.color.clone(),
            })
            .collect();
    }

    let mut out: Vec<TextSpan> = Vec::new();
    for ((c, run_i), (_, span_i)) in run_chars.into_iter().zip(span_chars) {
        let run = &runs[run_i];
        let changed = spans[span_i].changed;

        let continues_last = out.last().is_some_and(|last| {
            last.changed == changed
                && last.bold == run.bold
                && last.italic == run.italic
                && last.underline == run.underline
                && last.size_pt == run.size_pt
                && last.font == run.font
                && last.color == run.color
        });

        if continues_last {
            out.last_mut().expect("just checked Some above").text.push(c);
        } else {
            out.push(TextSpan {
                text: c.to_string(),
                changed,
                bold: run.bold,
                italic: run.italic,
                underline: run.underline,
                size_pt: run.size_pt,
                font: run.font.clone(),
                color: run.color.clone(),
            });
        }
    }
    out
}

/// Renders a point size without a trailing `.0`.
fn format_pt(pt: f32) -> String {
    if pt.fract() == 0.0 {
        format!("{}", pt as i32)
    } else {
        format!("{pt}")
    }
}

/// Plain-language summary of a formatting-only change between two paragraphs
/// whose text is identical, comparing only what a reader actually notices -
/// font, size, bold, italic, underline, colour. Uses each paragraph's first
/// run as its representative formatting: Office applies one style per
/// paragraph in the overwhelming common case, and a mixed-run paragraph
/// still gets the most visible change described rather than every run's.
/// Empty when nothing a reader would notice changed.
fn format_note(old: &Paragraph, new: &Paragraph) -> String {
    let (Some(old_run), Some(new_run)) = (old.runs.first(), new.runs.first()) else {
        return String::new();
    };

    let mut parts = Vec::new();

    if let (Some(o), Some(n)) = (old_run.size_pt, new_run.size_pt) {
        if o != n {
            parts.push(format!("{}pt → {}pt", format_pt(o), format_pt(n)));
        }
    }

    match (&old_run.font, &new_run.font) {
        (Some(o), Some(n)) if o != n => parts.push(format!("{o} → {n}")),
        // "default → Georgia" reads like a font called default. The file
        // simply names no font when the paragraph inherits one, and the
        // reader doesn't know or care which it inherited.
        (None, Some(n)) => parts.push(format!("font now {n}")),
        (Some(o), None) => parts.push(format!("no longer {o}")),
        _ => {}
    }

    if old_run.bold != new_run.bold {
        parts.push(if new_run.bold { "now bold" } else { "no longer bold" }.to_string());
    }
    if old_run.italic != new_run.italic {
        parts.push(if new_run.italic { "now italic" } else { "no longer italic" }.to_string());
    }
    if old_run.underline != new_run.underline {
        parts.push(
            if new_run.underline { "now underlined" } else { "no longer underlined" }.to_string(),
        );
    }

    if old_run.color != new_run.color {
        match &new_run.color {
            Some(n) => parts.push(format!("now #{n}")),
            None => parts.push("colour removed".to_string()),
        }
    }

    parts.join(" · ")
}

/// Aligns `older`'s and `newer`'s paragraphs and marks what changed.
///
/// Line alignment comes from `similar` (so a paragraph inserted earlier in
/// the document doesn't make every paragraph after it look changed); a
/// `Replace` op additionally gets a word-level diff on each paired line, so a
/// one-word edit marks that word rather than the whole paragraph. When a
/// replaced block's side lengths differ, the first `min(old_len, new_len)`
/// lines are paired as "changed" and the rest fall back to "removed"/"added".
///
/// A pair whose text matches exactly but whose formatting doesn't is
/// "restyled", not "same" - a paragraph that was entirely reformatted and
/// never reworded was previously invisible to this comparison.
pub fn diff_text(older: &[Paragraph], newer: &[Paragraph]) -> TextComparison {
    if older.is_empty() && newer.is_empty() {
        return TextComparison { rows: Vec::new(), has_text: false };
    }

    let older_text: Vec<String> = older.iter().map(Paragraph::text).collect();
    let newer_text: Vec<String> = newer.iter().map(Paragraph::text).collect();
    let older_refs: Vec<&str> = older_text.iter().map(String::as_str).collect();
    let newer_refs: Vec<&str> = newer_text.iter().map(String::as_str).collect();
    let diff = similar::TextDiff::from_slices(&older_refs, &newer_refs);

    let paired_row = |old: &Paragraph, new: &Paragraph| -> TextRow {
        let text = old.text();
        let note = format_note(old, new);
        TextRow {
            status: if note.is_empty() { "same" } else { "restyled" }.to_string(),
            left: Some(text.clone()),
            right: Some(text),
            left_spans: Vec::new(),
            right_spans: Vec::new(),
            left_runs: old.runs.clone(),
            right_runs: new.runs.clone(),
            format_note: note,
        }
    };
    let removed_row = |p: &Paragraph| TextRow {
        status: "removed".to_string(),
        left: Some(p.text()),
        right: None,
        left_spans: Vec::new(),
        right_spans: Vec::new(),
        left_runs: p.runs.clone(),
        right_runs: Vec::new(),
        format_note: String::new(),
    };
    let added_row = |p: &Paragraph| TextRow {
        status: "added".to_string(),
        left: None,
        right: Some(p.text()),
        left_spans: Vec::new(),
        right_spans: Vec::new(),
        left_runs: Vec::new(),
        right_runs: p.runs.clone(),
        format_note: String::new(),
    };

    let mut rows = Vec::new();
    for op in diff.ops() {
        match *op {
            similar::DiffOp::Equal { old_index, new_index, len } => {
                rows.extend(
                    (0..len).map(|i| paired_row(&older[old_index + i], &newer[new_index + i])),
                );
            }
            similar::DiffOp::Delete { old_index, old_len, .. } => {
                rows.extend((0..old_len).map(|i| removed_row(&older[old_index + i])));
            }
            similar::DiffOp::Insert { new_index, new_len, .. } => {
                rows.extend((0..new_len).map(|i| added_row(&newer[new_index + i])));
            }
            similar::DiffOp::Replace { old_index, old_len, new_index, new_len } => {
                let paired = old_len.min(new_len);
                for i in 0..paired {
                    let (old_p, new_p) = (&older[old_index + i], &newer[new_index + i]);

                    // A replace block pairs lines by position, so two lines
                    // that are identical can end up married to each other when
                    // the real edit is elsewhere in the block. Reporting those
                    // as "changed" put an unmarked paragraph across both
                    // columns with nothing to show for itself, and it never
                    // collapsed as unchanged because its status said otherwise.
                    if old_p.text() == new_p.text() {
                        rows.push(paired_row(old_p, new_p));
                        continue;
                    }

                    let (old_line, new_line) = (old_p.text(), new_p.text());
                    let (left_spans, right_spans) = word_spans(&old_line, &new_line);
                    rows.push(TextRow {
                        status: "changed".to_string(),
                        left: Some(old_line),
                        right: Some(new_line),
                        left_spans: intersect_runs_and_spans(&old_p.runs, &left_spans),
                        right_spans: intersect_runs_and_spans(&new_p.runs, &right_spans),
                        left_runs: old_p.runs.clone(),
                        right_runs: new_p.runs.clone(),
                        format_note: String::new(),
                    });
                }
                rows.extend((paired..old_len).map(|i| removed_row(&older[old_index + i])));
                rows.extend((paired..new_len).map(|i| added_row(&newer[new_index + i])));
            }
        }
    }

    TextComparison { rows, has_text: true }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Scratch space for tests, kept inside the project under `target/` rather
    /// than the OS temp directory, so a test run never scatters copies of real
    /// documents outside this repo.
    fn scratch_root() -> PathBuf {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/test-tmp");
        fs::create_dir_all(&root).expect("creating test scratch directory");
        root
    }

    /// Shadows `tempfile::tempdir` so every test's scratch lands in `target/`.
    /// Still deleted on drop, exactly like the real one.
    fn tempdir() -> std::io::Result<TempDir> {
        TempDir::new_in(scratch_root())
    }

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

    /// Like `build_test_zip`, but with `docProps/core.xml` and
    /// `word/document.xml` contents overridable, to test `manifest_hash`'s
    /// sensitivity to each independently.
    /// Office adding a metadata part it didn't write before must not look like
    /// a content change. It shifts every later part's `entry_order`, which is
    /// why the manifest hash doesn't include that index.
    #[test]
    fn an_extra_metadata_part_does_not_change_manifest_hash() {
        let src_dir = tempdir().unwrap();
        let objects_root = tempdir().unwrap();

        let without = src_dir.path().join("without.docx");
        build_test_zip_with(&without, b"<core rev='1'/>", b"<document>same</document>");

        // Same content, plus one more docProps entry ahead of the content part.
        let with = src_dir.path().join("with.docx");
        {
            let file = File::create(&with).unwrap();
            let mut writer = ZipWriter::new(file);
            let stored = SimpleFileOptions::DEFAULT.compression_method(CompressionMethod::Stored);
            let deflated =
                SimpleFileOptions::DEFAULT.compression_method(CompressionMethod::Deflated);

            writer.start_file("[Content_Types].xml", deflated).unwrap();
            writer.write_all(b"<Types/>").unwrap();
            writer.start_file("docProps/core.xml", stored).unwrap();
            writer.write_all(b"<core rev='1'/>").unwrap();
            writer.start_file("docProps/custom.xml", stored).unwrap();
            writer.write_all(b"<custom/>").unwrap();
            writer.start_file("word/document.xml", deflated).unwrap();
            writer.write_all(b"<document>same</document>").unwrap();
            writer.finish().unwrap();
        }

        let a = snapshot(&without, objects_root.path()).unwrap();
        let b = snapshot(&with, objects_root.path()).unwrap();

        assert_eq!(
            a.manifest_hash, b.manifest_hash,
            "an added metadata part shifted a content part's index and was mistaken for an edit"
        );
        // The extra part is still stored and restorable - only the hash ignores it.
        assert_eq!(a.parts.len() + 1, b.parts.len());
    }

    fn build_test_zip_with(path: &Path, core_xml: &[u8], document_xml: &[u8]) {
        let file = File::create(path).unwrap();
        let mut writer = ZipWriter::new(file);

        let stored = SimpleFileOptions::DEFAULT.compression_method(CompressionMethod::Stored);
        let deflated = SimpleFileOptions::DEFAULT.compression_method(CompressionMethod::Deflated);

        writer.start_file("[Content_Types].xml", deflated).unwrap();
        writer.write_all(b"<Types/>").unwrap();

        writer.start_file("docProps/core.xml", stored).unwrap();
        writer.write_all(core_xml).unwrap();

        writer.start_file("word/document.xml", deflated).unwrap();
        writer.write_all(document_xml).unwrap();

        writer.finish().unwrap();
    }

    /// Round-trips a genuine Office file, which a synthetic zip cannot stand in
    /// for: only a real archive has the mixed Stored/Deflated entries, the
    /// 1980-01-01 timestamps and the ~180 parts that a deck actually contains.
    /// Reads every Office file in the repo's `test-data/` folder, which is
    /// gitignored because real documents must not be published - create it and
    /// drop throwaway files in. Ignored by default so `cargo test` stays fast.
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
            "no .pptx/.docx/.xlsx files in {} - put a throwaway Office file there",
            test_data.display()
        );

        for source in sources {
            round_trip_one_real_file(&source);
        }
    }

    /// Times `snapshot` on real Office files, to show where save-to-seen
    /// latency actually goes before anyone tunes the watcher's debounce.
    ///
    /// `cargo test --lib snapshot_cost_on_real_files -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn snapshot_cost_on_real_files() {
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
        assert!(!sources.is_empty(), "no Office files in {}", test_data.display());

        for source in sources {
            let objects_root = tempdir().unwrap();
            let size = fs::metadata(&source).unwrap().len();

            let cold = std::time::Instant::now();
            let snap = snapshot(&source, objects_root.path()).unwrap();
            let cold = cold.elapsed();

            // Second run: every object already on disk, which is the normal
            // case once a file has been saved a few times.
            let warm = std::time::Instant::now();
            snapshot(&source, objects_root.path()).unwrap();
            let warm = warm.elapsed();

            println!(
                "{:>52}  {:>7} KB  {:>3} parts  first {:>6.1} ms  repeat {:>6.1} ms",
                source.file_name().unwrap().to_string_lossy(),
                size / 1024,
                snap.parts.len(),
                cold.as_secs_f64() * 1000.0,
                warm.as_secs_f64() * 1000.0,
            );
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

        // Kept deliberately, under the original extension: the whole point is
        // for a human to double-click this and see whether Office complains.
        // Inside target/ so these copies of real documents stay in the repo and
        // stay gitignored.
        let name = source.file_name().expect("fixture has a file name");
        let keep_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/test-restored");
        fs::create_dir_all(&keep_dir).unwrap();
        let keep = keep_dir.join(name);
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
            db::NewRevision {
                file_id,
                timestamp: 1000,
                byte_size: snap.byte_size,
                manifest_hash: &snap.manifest_hash,
                parts: &snap.parts,
                objects: &snap.objects,
                author_name: None,
                author_email: None,
                parent_id: None,
            },
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

    /// The headline regression: Office rewrites `docProps/core.xml` (revision
    /// counter, modified timestamp) on every save even when the user changed
    /// nothing, which must not look like a content change.
    #[test]
    fn metadata_only_change_does_not_change_manifest_hash() {
        let src_dir = tempdir().unwrap();
        let objects_root = tempdir().unwrap();

        let a = src_dir.path().join("a.zip");
        let b = src_dir.path().join("b.zip");
        build_test_zip_with(&a, b"<cp:revision>54</cp:revision>", b"<document/>");
        build_test_zip_with(&b, b"<cp:revision>55</cp:revision>", b"<document/>");

        let snap_a = snapshot(&a, objects_root.path()).unwrap();
        let snap_b = snapshot(&b, objects_root.path()).unwrap();

        assert_eq!(snap_a.manifest_hash, snap_b.manifest_hash);

        let core_hash = |snap: &Snapshot| {
            snap.parts
                .iter()
                .find(|p| p.entry_name == "docProps/core.xml")
                .unwrap()
                .object_hash
                .clone()
        };
        assert_ne!(
            core_hash(&snap_a),
            core_hash(&snap_b),
            "the metadata really differed and really was stored, just not hashed into manifest_hash"
        );
    }

    #[test]
    fn content_part_change_does_change_manifest_hash() {
        let src_dir = tempdir().unwrap();
        let objects_root = tempdir().unwrap();

        let a = src_dir.path().join("a.zip");
        let b = src_dir.path().join("b.zip");
        build_test_zip_with(&a, b"<cp:revision>54</cp:revision>", b"<document>old</document>");
        build_test_zip_with(&b, b"<cp:revision>54</cp:revision>", b"<document>new</document>");

        let snap_a = snapshot(&a, objects_root.path()).unwrap();
        let snap_b = snapshot(&b, objects_root.path()).unwrap();

        assert_ne!(snap_a.manifest_hash, snap_b.manifest_hash);
    }

    /// Narrowing the hash must not narrow storage: the excluded part still
    /// restores as whatever was actually saved, not a prior version.
    #[test]
    fn metadata_only_change_still_restores_its_own_content() {
        let src_dir = tempdir().unwrap();
        let objects_root = tempdir().unwrap();

        let src = src_dir.path().join("src.zip");
        build_test_zip_with(&src, b"<cp:revision>55</cp:revision>", b"<document/>");

        let snap = snapshot(&src, objects_root.path()).unwrap();

        let dest = src_dir.path().join("restored.zip");
        restore(&snap.parts, objects_root.path(), &dest).unwrap();

        let mut restored = ZipArchive::new(File::open(&dest).unwrap()).unwrap();
        let mut bytes = Vec::new();
        restored
            .by_name("docProps/core.xml")
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        assert_eq!(bytes, b"<cp:revision>55</cp:revision>");
    }

    fn paragraph_texts(paragraphs: &[Paragraph]) -> Vec<String> {
        paragraphs.iter().map(Paragraph::text).collect()
    }

    #[test]
    fn extract_text_reads_a_two_paragraph_slide() {
        let xml = r#"<p:sld><p:cSld><p:spTree>
            <p:sp><p:txBody><a:p><a:r><a:t>Welcome</a:t></a:r></a:p></p:txBody></p:sp>
            <p:sp><p:txBody><a:p><a:r><a:t>Thanks for coming</a:t></a:r></a:p></p:txBody></p:sp>
        </p:spTree></p:cSld></p:sld>"#;

        let paragraphs = extract_text("ppt/slides/slide1.xml", xml.as_bytes());
        assert_eq!(paragraph_texts(&paragraphs), vec!["Welcome", "Thanks for coming"]);
    }

    #[test]
    fn extract_text_joins_a_word_run_split_mid_sentence() {
        // Word splits a run on every formatting change, so the same sentence
        // routinely arrives as several adjacent <w:t> runs.
        let xml = r#"<w:document><w:body>
            <w:p><w:r><w:t>Hello </w:t></w:r><w:r><w:t>world</w:t></w:r></w:p>
        </w:body></w:document>"#;

        let paragraphs = extract_text("word/document.xml", xml.as_bytes());
        assert_eq!(paragraph_texts(&paragraphs), vec!["Hello world"]);
        assert_eq!(paragraphs[0].runs.len(), 2, "each <w:r> stays its own run");
    }

    #[test]
    fn extract_text_decodes_entities() {
        let xml = r#"<w:document><w:body>
            <w:p><w:r><w:t>Tom &amp; Jerry &lt;3</w:t></w:r></w:p>
        </w:body></w:document>"#;

        let paragraphs = extract_text("word/document.xml", xml.as_bytes());
        assert_eq!(paragraph_texts(&paragraphs), vec!["Tom & Jerry <3"]);
    }

    #[test]
    fn extract_text_reads_excel_shared_strings() {
        let xml = r#"<sst><si><t>Revenue</t></si><si><t>Q1 2024</t></si></sst>"#;
        let paragraphs = extract_text("xl/sharedStrings.xml", xml.as_bytes());
        assert_eq!(paragraph_texts(&paragraphs), vec!["Revenue", "Q1 2024"]);
    }

    #[test]
    fn extract_text_is_empty_for_an_unrecognised_part() {
        assert_eq!(extract_text("ppt/theme/theme1.xml", b"<a:theme/>"), Vec::<Paragraph>::new());
        assert_eq!(extract_text("docProps/core.xml", b"<cp:core/>"), Vec::<Paragraph>::new());
    }

    #[test]
    fn extract_text_ignores_run_properties_that_share_the_run_tags_prefix() {
        // "a:rPr" (run properties) must never be mistaken for "a:r".
        let xml = r#"<a:p><a:r><a:rPr sz="1800"/><a:t>Sized text</a:t></a:r></a:p>"#;
        let paragraphs = extract_text("ppt/slides/slide2.xml", xml.as_bytes());
        assert_eq!(paragraph_texts(&paragraphs), vec!["Sized text"]);
    }

    #[test]
    fn word_size_is_parsed_from_half_points() {
        let xml = r#"<w:document><w:body>
            <w:p><w:r><w:rPr><w:sz w:val="24"/></w:rPr><w:t>Body</w:t></w:r></w:p>
        </w:body></w:document>"#;
        let paragraphs = extract_text("word/document.xml", xml.as_bytes());
        assert_eq!(paragraphs[0].runs[0].size_pt, Some(12.0));
    }

    #[test]
    fn ppt_size_is_parsed_from_hundredths_of_a_point() {
        let xml = r#"<a:p><a:r><a:rPr sz="1800"/><a:t>Heading</a:t></a:r></a:p>"#;
        let paragraphs = extract_text("ppt/slides/slide3.xml", xml.as_bytes());
        assert_eq!(paragraphs[0].runs[0].size_pt, Some(18.0));
    }

    #[test]
    fn a_missing_property_stays_none_or_false() {
        let xml = r#"<w:document><w:body>
            <w:p><w:r><w:t>Plain</w:t></w:r></w:p>
        </w:body></w:document>"#;
        let paragraphs = extract_text("word/document.xml", xml.as_bytes());
        let run = &paragraphs[0].runs[0];
        assert!(!run.bold);
        assert!(!run.italic);
        assert!(!run.underline);
        assert_eq!(run.size_pt, None);
        assert_eq!(run.font, None);
        assert_eq!(run.color, None);
    }

    #[test]
    fn a_run_without_its_own_rpr_inherits_the_paragraph_default() {
        let xml = r#"<w:document><w:body>
            <w:p>
                <w:pPr><w:rPr><w:b/><w:sz w:val="28"/></w:rPr></w:pPr>
                <w:r><w:t>Inherits</w:t></w:r>
                <w:r><w:rPr><w:b w:val="0"/></w:rPr><w:t>Overrides bold off</w:t></w:r>
            </w:p>
        </w:body></w:document>"#;
        let paragraphs = extract_text("word/document.xml", xml.as_bytes());
        let runs = &paragraphs[0].runs;
        assert!(runs[0].bold);
        assert_eq!(runs[0].size_pt, Some(14.0), "size still falls through to the paragraph default");
        assert!(!runs[1].bold, "the run's own <w:b w:val=\"0\"/> wins over the paragraph default");
        assert_eq!(runs[1].size_pt, Some(14.0));
    }

    fn plain(text: &str) -> Paragraph {
        Paragraph { runs: vec![Run::plain(text.to_string())] }
    }

    fn row(status: &str, left: Option<&str>, right: Option<&str>) -> TextRow {
        TextRow {
            status: status.to_string(),
            left: left.map(str::to_string),
            right: right.map(str::to_string),
            left_spans: Vec::new(),
            right_spans: Vec::new(),
            left_runs: left.map(|t| vec![Run::plain(t.to_string())]).unwrap_or_default(),
            right_runs: right.map(|t| vec![Run::plain(t.to_string())]).unwrap_or_default(),
            format_note: String::new(),
        }
    }

    /// A replace block pairs lines by position, so an identical line can be
    /// paired with itself while the real edit sits elsewhere in the block.
    /// Reporting that as "changed" showed an unmarked paragraph across both
    /// columns, and it never collapsed as unchanged because its status lied.
    #[test]
    fn identical_lines_inside_a_replace_block_are_not_reported_as_changed() {
        let older: Vec<Paragraph> = [
            "Opening paragraph.",
            "A line that really does change.",
            "Date: ___________________",
            "A trailing paragraph nobody touched.",
        ]
        .map(plain)
        .to_vec();
        let newer: Vec<Paragraph> = [
            "Opening paragraph.",
            "A line that genuinely does change.",
            "Date: ___________________",
            "A trailing paragraph nobody touched.",
        ]
        .map(plain)
        .to_vec();

        let diff = diff_text(&older, &newer);

        let changed: Vec<&TextRow> = diff.rows.iter().filter(|r| r.status != "same").collect();
        assert_eq!(changed.len(), 1, "only one line actually differs: {:?}",
            diff.rows.iter().map(|r| (&r.status, &r.left)).collect::<Vec<_>>());

        for row in &diff.rows {
            if row.status == "changed" {
                assert_ne!(row.left, row.right, "a changed row must actually differ");
                assert!(
                    row.left_spans.iter().any(|s| s.changed),
                    "a changed row must mark the words that differ"
                );
            }
        }
    }

    #[test]
    fn diff_text_marks_an_unchanged_paragraph_as_same() {
        let older = vec![plain("Welcome")];
        let newer = vec![plain("Welcome")];

        let result = diff_text(&older, &newer);
        assert!(result.has_text);
        assert_eq!(result.rows, vec![row("same", Some("Welcome"), Some("Welcome"))]);
    }

    #[test]
    fn diff_text_isolates_the_one_word_that_changed() {
        let older = vec![plain("The quick brown fox")];
        let newer = vec![plain("The quick red fox")];

        let result = diff_text(&older, &newer);
        assert_eq!(result.rows.len(), 1);
        let changed = &result.rows[0];
        assert_eq!(changed.status, "changed", "a text change, not just formatting, is still \"changed\"");

        // Only "brown"/"red" are marked - not the whole line.
        let changed_left: Vec<&str> = changed
            .left_spans
            .iter()
            .filter(|s| s.changed)
            .map(|s| s.text.as_str())
            .collect();
        let changed_right: Vec<&str> = changed
            .right_spans
            .iter()
            .filter(|s| s.changed)
            .map(|s| s.text.as_str())
            .collect();
        assert_eq!(changed_left, vec!["brown"]);
        assert_eq!(changed_right, vec!["red"]);

        let unchanged_left_words = changed.left_spans.iter().filter(|s| !s.changed).count();
        assert!(unchanged_left_words > 0, "the rest of the line must stay unmarked");
    }

    #[test]
    fn diff_text_marks_an_added_paragraph() {
        let older = vec![plain("Intro")];
        let newer = vec![plain("Intro"), plain("New section")];

        let result = diff_text(&older, &newer);
        assert_eq!(
            result.rows,
            vec![row("same", Some("Intro"), Some("Intro")), row("added", None, Some("New section"))]
        );
    }

    #[test]
    fn diff_text_marks_a_removed_paragraph() {
        let older = vec![plain("Intro"), plain("Old section")];
        let newer = vec![plain("Intro")];

        let result = diff_text(&older, &newer);
        assert_eq!(
            result.rows,
            vec![row("same", Some("Intro"), Some("Intro")), row("removed", Some("Old section"), None)]
        );
    }

    #[test]
    fn diff_text_reports_no_text_when_both_sides_are_empty() {
        let result = diff_text(&[], &[]);
        assert!(!result.has_text);
        assert!(result.rows.is_empty());
    }

    fn bold_run(text: &str) -> Run {
        Run { bold: true, ..Run::plain(text.to_string()) }
    }

    #[test]
    fn a_bold_only_change_is_restyled_with_a_sensible_note() {
        let older = vec![Paragraph { runs: vec![Run::plain("Thank you for your patience.".to_string())] }];
        let newer = vec![Paragraph { runs: vec![bold_run("Thank you for your patience.")] }];

        let result = diff_text(&older, &newer);
        assert_eq!(result.rows.len(), 1);
        let restyled = &result.rows[0];
        assert_eq!(restyled.status, "restyled");
        assert_eq!(restyled.left, restyled.right, "a restyled row carries the same text on both sides");
        assert_eq!(restyled.format_note, "now bold");
        assert_eq!(restyled.left_runs[0].bold, false);
        assert_eq!(restyled.right_runs[0].bold, true);
    }

    #[test]
    fn a_fully_unchanged_paragraph_has_no_format_note() {
        let older = vec![plain("Nothing moved.")];
        let newer = vec![plain("Nothing moved.")];
        let result = diff_text(&older, &newer);
        assert_eq!(result.rows[0].status, "same");
        assert_eq!(result.rows[0].format_note, "");
    }

    #[test]
    fn intersecting_runs_and_spans_keeps_a_changed_word_bold() {
        // "quick" sits entirely inside a bold run; "brown fox" is plain. Only
        // "brown" differs between old and new.
        let runs = vec![bold_run("The quick "), Run::plain("brown fox".to_string())];
        let (left_spans, _) = word_spans("The quick brown fox", "The quick red fox");

        let pieces = intersect_runs_and_spans(&runs, &left_spans);

        // Reassembling the pieces must reproduce the original text exactly.
        let reassembled: String = pieces.iter().map(|p| p.text.as_str()).collect();
        assert_eq!(reassembled, "The quick brown fox");

        let bold_piece = pieces.iter().find(|p| p.text == "The quick ").expect("a bold piece survives");
        assert!(bold_piece.bold);
        assert!(!bold_piece.changed);

        let changed_piece = pieces.iter().find(|p| p.changed).expect("the changed word survives");
        assert_eq!(changed_piece.text, "brown");
        assert!(!changed_piece.bold, "the changed word sits in the plain run, not the bold one");
    }

    #[test]
    fn intersecting_a_changed_word_inside_a_bold_run_marks_it_both_bold_and_changed() {
        let runs = vec![bold_run("The quick brown fox")];
        let (left_spans, _) = word_spans("The quick brown fox", "The quick red fox");

        let pieces = intersect_runs_and_spans(&runs, &left_spans);

        let changed_piece = pieces.iter().find(|p| p.changed).expect("the changed word survives");
        assert_eq!(changed_piece.text, "brown");
        assert!(changed_piece.bold, "the whole run is bold, so the changed word inside it is too");
    }
}
