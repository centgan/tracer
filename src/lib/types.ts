// Mirrors the Rust types in src-tauri/src/db.rs. Rust serializes camelCase.

export type Vault = {
  id: number;
  rootPath: string;
  isActive: boolean;
  // Computed from the filesystem. False means the folder's `.tracer`
  // directory is gone, so Tracer has stopped watching it even though it's
  // still in the list.
  hasTracerFolder: boolean;
};

export type TrackedFile = {
  id: number;
  vaultId: number;
  relativePath: string;
  extension: string;
};

export type Revision = {
  id: number;
  fileId: number;
  timestamp: number; // unix seconds
  byteSize: number;
  manifestHash: string;
  partCount: number;
  milestone: string | null;
  // Null for every version recorded before author tracking shipped - that's
  // honest, not a bug, and must never be backfilled with a guess.
  authorName: string | null;
  authorEmail: string | null;
  parentId: number | null;
};

// The user's own name/email, used only to label versions they create.
// Empty strings mean unset, not an error.
export type Identity = {
  name: string;
  email: string;
};

// A tracked file plus the timestamp of its newest revision, if it has one.
// There's no backend query for this directly, so the Files pane derives it
// by reading each file's timeline.
export type FileWithLastChanged = TrackedFile & { lastChanged: number | null };

// What a file's whole history costs, and what it would have cost without
// part-level dedup. Mirrors `db::FileStats` in src-tauri/src/db.rs.
export type FileStats = {
  versionCount: number;
  logicalBytes: number;
  storedBytes: number;
};

// One changed zip entry between two versions, in the backend's own jargon
// (entryName is a raw zip path like "ppt/slides/slide4.xml") - see
// src/lib/partnames.ts for turning that into something a user recognises.
export type PartChange = {
  entryName: string;
  status: "added" | "removed" | "changed";
};

// compare_revisions's result: sorted by entryName, oldest-to-newest implied
// by which id the caller passed as olderId/newerId.
export type CompareResult = {
  changes: PartChange[];
  unchangedCount: number;
};

// compare_part_text's word-level comparison of one part's extracted text
// between two versions. Mirrors `engine::{Run,TextSpan,TextRow,TextComparison}`
// in src-tauri/src/engine.rs.

// One run of text sharing one set of formatting, as the document file itself
// stores it. A missing property (null/false) means inherited/default, never
// a guess - see DESIGN.md's "Showing formatting".
export type Run = {
  text: string;
  bold: boolean;
  italic: boolean;
  underline: boolean;
  sizePt: number | null;
  // Never used to set CSS font-family - DESIGN.md: "A font family is named,
  // never faked." A silent substitution would show a difference that isn't
  // there, or hide one that is.
  font: string | null;
  color: string | null; // "RRGGBB", as the file stores it.
};

// One word-level piece of a "changed" row, carrying both whether it differs
// and the formatting of the run it sits in (computed in Rust by intersecting
// the two by character offset, so a changed word inside a bold run comes
// back both bold and marked).
export type TextSpan = {
  text: string;
  changed: boolean;
  bold: boolean;
  italic: boolean;
  underline: boolean;
  sizePt: number | null;
  font: string | null;
  color: string | null;
};

export type TextRow = {
  status: "same" | "added" | "removed" | "changed" | "restyled";
  left: string | null;
  right: string | null;
  // Word-level pieces for a "changed" row, so the exact words that differ
  // can be marked. Empty for every other status.
  leftSpans: TextSpan[];
  rightSpans: TextSpan[];
  // Document-order runs for this row's text, on every row, so any line -
  // not just a "changed" one - renders the way the document actually looks.
  leftRuns: Run[];
  rightRuns: Run[];
  // Plain-language summary of a formatting-only change, e.g.
  // "11pt → 12pt · Calibri → Georgia · now bold". Empty when nothing did.
  formatNote: string;
};

export type TextComparison = {
  rows: TextRow[];
  // False when neither side yielded any text, so the UI can explain itself
  // instead of showing an empty table.
  hasText: boolean;
};

// The three states every async pane can be in. `null` (used at the call
// site, not part of this type) stands for "nothing to load yet" - e.g. no
// folder picked - which is distinct from "loading" and needs its own copy.
export type Loadable<T> =
  | { status: "loading" }
  | { status: "loaded"; data: T }
  | { status: "error"; message: string };
