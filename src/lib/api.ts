import { invoke } from "@tauri-apps/api/core";
import type { Vault, TrackedFile, Revision } from "./types";

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
export const listTrackedFiles = (vaultId: number) =>
  call<TrackedFile[]>("list_tracked_files", { vaultId });
export const getTimeline = (fileId: number) => call<Revision[]>("get_timeline", { fileId });
export const setMilestone = (revisionId: number, label: string) =>
  call<void>("set_milestone", { revisionId, label });
export const saveCopy = (revisionId: number) => call<string>("save_copy", { revisionId });
