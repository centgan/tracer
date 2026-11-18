import { invoke } from "@tauri-apps/api/core";
import type {
  Vault,
  TrackedFile,
  Revision,
  Identity,
  FileStats,
  CompareResult,
  TextComparison,
} from "./types";

function asMessage(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return "Something went wrong.";
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    throw new Error(asMessage(e));
  }
}

export const listVaults = () => call<Vault[]>("list_vaults");
export const addVault = (path: string) => call<Vault>("add_vault", { path });
export const setVaultActive = (vaultId: number, active: boolean) =>
  call<void>("set_vault_active", { vaultId, active });
export const listTrackedFiles = (vaultId: number) =>
  call<TrackedFile[]>("list_tracked_files", { vaultId });
export const getTimeline = (fileId: number) => call<Revision[]>("get_timeline", { fileId });
export const getFileStats = (fileId: number) => call<FileStats>("get_file_stats", { fileId });
export const setMilestone = (revisionId: number, label: string) =>
  call<void>("set_milestone", { revisionId, label });
export const clearMilestone = (revisionId: number) =>
  call<void>("clear_milestone", { revisionId });
export const saveCopy = (revisionId: number) => call<string>("save_copy", { revisionId });
export const compareRevisions = (olderId: number, newerId: number) =>
  call<CompareResult>("compare_revisions", { olderId, newerId });
export const comparePartText = (olderId: number, newerId: number, entryName: string) =>
  call<TextComparison>("compare_part_text", { olderId, newerId, entryName });
export const quitApp = () => call<void>("quit_app");
export const isAutostartEnabled = () => call<boolean>("is_autostart_enabled");
export const setAutostart = (enabled: boolean) => call<void>("set_autostart", { enabled });
export const getIdentity = () => call<Identity>("get_identity");
export const setIdentity = (identity: Identity) =>
  call<void>("set_identity", { name: identity.name, email: identity.email });
