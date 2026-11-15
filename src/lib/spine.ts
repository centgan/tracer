import type { TimelineGroup } from "./sessions";

// A gap at least this long between two consecutive timeline entries is worth
// calling out as a quiet period - anything shorter is just normal time
// passing within the same stretch of work.
export const GAP_THRESHOLD_SECONDS = 24 * 60 * 60;

const WEEK = 7 * GAP_THRESHOLD_SECONDS;
const MONTH = 30 * GAP_THRESHOLD_SECONDS;
const YEAR = 365 * GAP_THRESHOLD_SECONDS;

export type SpineGap = {
  // The gap renders between groups[afterIndex] and groups[afterIndex + 1].
  afterIndex: number;
  label: string;
};

// Quiet periods in a newest-first list of timeline entries (groups, i.e.
// rendered rows - a solo version or a collapsed session). A gap only ever
// sits between two existing entries: there is nothing on the other side of
// the first or the last to bound it against.
export function findGaps(groups: TimelineGroup[]): SpineGap[] {
  const gaps: SpineGap[] = [];
  for (let i = 0; i < groups.length - 1; i++) {
    const olderEdgeOfNewerGroup = groups[i].revisions[groups[i].revisions.length - 1].timestamp;
    const newerEdgeOfOlderGroup = groups[i + 1].revisions[0].timestamp;
    const seconds = olderEdgeOfNewerGroup - newerEdgeOfOlderGroup;
    if (seconds >= GAP_THRESHOLD_SECONDS) {
      gaps.push({ afterIndex: i, label: formatGapLength(seconds) });
    }
  }
  return gaps;
}

export function formatGapLength(seconds: number): string {
  if (seconds < WEEK) {
    const days = Math.round(seconds / GAP_THRESHOLD_SECONDS);
    return days === 1 ? "a day" : `${days} days`;
  }
  if (seconds < MONTH) {
    const weeks = Math.round(seconds / WEEK);
    return weeks === 1 ? "a week" : `${weeks} weeks`;
  }
  if (seconds < YEAR) {
    const months = Math.round(seconds / MONTH);
    return months === 1 ? "a month" : `${months} months`;
  }
  const years = Math.round(seconds / YEAR);
  return years === 1 ? "a year" : `${years} years`;
}

// A session row's density: a few stacked ticks, more saves meaning more
// ticks, capped so a 200-save session doesn't render 200 elements.
export const MAX_SESSION_TICKS = 5;

export function sessionTickCount(saveCount: number): number {
  return Math.min(saveCount, MAX_SESSION_TICKS);
}
