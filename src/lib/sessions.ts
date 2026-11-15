import type { Revision } from "./types";

// Saves inside one sitting land seconds to minutes apart; the next sitting
// is usually at least a coffee break later. 20 minutes separates those two
// cases without being so long it bridges a real gap (lunch, next morning).
export const SESSION_GAP_SECONDS = 20 * 60;

export type TimelineGroup = {
  // Keyed by the group's newest revision, stable across reorders.
  id: number;
  revisions: Revision[];
};

// Groups consecutive versions (already sorted newest-first) into work
// sessions. A named version always starts (and ends) its own group of one,
// since named versions must never be collapsed inside a session.
export function groupIntoSessions(revisions: Revision[]): TimelineGroup[] {
  const groups: TimelineGroup[] = [];
  for (const revision of revisions) {
    const prevGroup = groups[groups.length - 1];
    const prev = prevGroup?.revisions[prevGroup.revisions.length - 1];
    const gapSeconds = prev ? prev.timestamp - revision.timestamp : Infinity;
    const canJoin =
      prevGroup !== undefined &&
      revision.milestone === null &&
      prev?.milestone == null &&
      gapSeconds < SESSION_GAP_SECONDS;

    if (canJoin) {
      prevGroup.revisions.push(revision);
    } else {
      groups.push({ id: revision.id, revisions: [revision] });
    }
  }
  return groups;
}
