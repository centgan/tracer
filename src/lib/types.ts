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
// between two versions. Mirrors `engine::{TextSpan,TextRow,TextComparison}`
// in src-tauri/src/engine.rs.
export type TextSpan = {
  text: string;
  changed: boolean;
};

export type TextRow = {
  status: "same" | "added" | "removed" | "changed";
  left: string | null;
  right: string | null;
  // Word-level pieces for a "changed" row, so the exact words that differ
  // can be marked. Empty for every other status.
  leftSpans: TextSpan[];
  rightSpans: TextSpan[];
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
