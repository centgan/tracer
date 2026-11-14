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
};

// A tracked file plus the timestamp of its newest revision, if it has one.
// There's no backend query for this directly, so the Files pane derives it
// by reading each file's timeline.
export type FileWithLastChanged = TrackedFile & { lastChanged: number | null };

// The three states every async pane can be in. `null` (used at the call
// site, not part of this type) stands for "nothing to load yet" - e.g. no
// folder picked - which is distinct from "loading" and needs its own copy.
export type Loadable<T> =
  | { status: "loading" }
  | { status: "loaded"; data: T }
  | { status: "error"; message: string };
