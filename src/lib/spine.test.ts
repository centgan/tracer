import { describe, expect, it } from "vitest";
import { findGaps, formatGapLength, GAP_THRESHOLD_SECONDS } from "./spine";
import type { TimelineGroup } from "./sessions";
import type { Revision } from "./types";

function version(id: number, timestamp: number): Revision {
  return {
    id,
    fileId: 1,
    timestamp,
    byteSize: 1000,
    manifestHash: `hash${id}`,
    partCount: 3,
    milestone: null,
    authorName: null,
    authorEmail: null,
    parentId: null,
  };
}

// One version per group, by id, newest first - enough to exercise gap
// placement without pulling in groupIntoSessions itself.
function groupOf(id: number, timestamp: number): TimelineGroup {
  return { id, revisions: [version(id, timestamp)] };
}

const NOON = 1_700_000_000;

describe("findGaps", () => {
  it("emits no gap below the threshold", () => {
    const groups = [groupOf(2, NOON), groupOf(1, NOON - (GAP_THRESHOLD_SECONDS - 1))];
    expect(findGaps(groups)).toEqual([]);
  });

  it("emits a gap at or above the threshold", () => {
    const groups = [groupOf(2, NOON), groupOf(1, NOON - GAP_THRESHOLD_SECONDS)];
    expect(findGaps(groups)).toEqual([{ afterIndex: 0, label: "a day" }]);
  });

  it("never emits a gap before the first entry or after the last", () => {
    const groups = [groupOf(3, NOON), groupOf(2, NOON - 60), groupOf(1, NOON - 120)];
    const gaps = findGaps(groups);
    for (const gap of gaps) {
      expect(gap.afterIndex).toBeGreaterThanOrEqual(0);
      expect(gap.afterIndex).toBeLessThan(groups.length - 1);
    }
  });

  it("handles a single entry with no gaps at all", () => {
    expect(findGaps([groupOf(1, NOON)])).toEqual([]);
  });
});

describe("formatGapLength", () => {
  const DAY = GAP_THRESHOLD_SECONDS;

  it("wording at a day", () => {
    expect(formatGapLength(DAY)).toBe("a day");
    expect(formatGapLength(3 * DAY)).toBe("3 days");
  });

  it("wording at a week", () => {
    expect(formatGapLength(7 * DAY)).toBe("a week");
    expect(formatGapLength(21 * DAY)).toBe("3 weeks");
  });

  it("wording at a month", () => {
    expect(formatGapLength(30 * DAY)).toBe("a month");
    expect(formatGapLength(60 * DAY)).toBe("2 months");
  });

  it("wording at a year", () => {
    expect(formatGapLength(365 * DAY)).toBe("a year");
    expect(formatGapLength(2 * 365 * DAY)).toBe("2 years");
  });
});
