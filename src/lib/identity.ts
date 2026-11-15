import type { Identity } from "./types";

// Shown once at least one folder is being watched and the identity has never
// been set. Skipped entirely with nothing watched - there's nothing to label
// yet - and skipped once either field is set, even if the other is blank.
export function shouldShowIdentityNudge(identity: Identity, watchedCount: number | null): boolean {
  return identity.name === "" && identity.email === "" && !!watchedCount;
}
