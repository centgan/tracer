import { describe, expect, it } from "vitest";
import { groupIntoSessions, SESSION_GAP_SECONDS } from "./sessions";
import type { Revision } from "./types";

// Timeline order is newest-first, so each entry here is older than the last.
function version(id: number, timestamp: number, milestone: string | null = null): Revision {
  return {
    id,
    fileId: 1,
    timestamp,
    byteSize: 1000,
    manifestHash: `hash${id}`,
    partCount: 3,
    milestone,
  };
}

const NOON = 1_700_000_000;

describe("groupIntoSessions", () => {
  it("collapses saves made within one sitting", () => {
    const groups = groupIntoSessions([
      version(3, NOON),
      version(2, NOON - 60),
      version(1, NOON - 120),
    ]);

    expect(groups).toHaveLength(1);
    expect(groups[0].revisions.map((r) => r.id)).toEqual([3, 2, 1]);
  });

  it("starts a new session once the gap is reached", () => {
    const groups = groupIntoSessions([
      version(2, NOON),
      version(1, NOON - SESSION_GAP_SECONDS),
    ]);

    expect(groups).toHaveLength(2);
  });

  it("keeps a sitting together one second under the gap", () => {
    const groups = groupIntoSessions([
      version(2, NOON),
      version(1, NOON - SESSION_GAP_SECONDS + 1),
    ]);

    expect(groups).toHaveLength(1);
  });

  // The whole point of naming a version is to find it again, so it must never
  // be hidden inside a collapsed session.
  it("never collapses a named version, and breaks the run on both sides", () => {
    const groups = groupIntoSessions([
      version(3, NOON),
      version(2, NOON - 60, "sent to supervisor"),
      version(1, NOON - 120),
    ]);

    expect(groups.map((g) => g.revisions.map((r) => r.id))).toEqual([[3], [2], [1]]);
  });

  it("handles an empty timeline", () => {
    expect(groupIntoSessions([])).toEqual([]);
  });

  it("gives every group a stable id and loses no versions", () => {
    const input = [version(4, NOON), version(3, NOON - 60), version(2, NOON - 9999), version(1, NOON - 10050)];
    const groups = groupIntoSessions(input);

    expect(groups.flatMap((g) => g.revisions)).toHaveLength(input.length);
    expect(new Set(groups.map((g) => g.id)).size).toBe(groups.length);
  });
});
