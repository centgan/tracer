export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const kb = bytes / 1024;
  if (kb < 1024) return `${kb.toFixed(kb < 10 ? 1 : 0)} KB`;
  const mb = kb / 1024;
  if (mb < 1024) return `${mb.toFixed(mb < 10 ? 1 : 0)} MB`;
  // A whole file's history can run into gigabytes; always keep one decimal
  // here, since rounding "11.7 GB" to "12 GB" throws away the figure that
  // matters at this scale.
  const gb = mb / 1024;
  return `${gb.toFixed(1)} GB`;
}

import type { FileStats } from "./types";

// The quiet stats line near the timeline header. Never claims a saving that
// isn't real: a brand-new file has storedBytes >= logicalBytes (one version,
// nothing shared yet), so it gets just the count, no "kept in" clause.
export function formatFileStats(stats: FileStats): string {
  const versions = `${stats.versionCount} version${stats.versionCount === 1 ? "" : "s"}`;
  if (stats.storedBytes >= stats.logicalBytes) return versions;
  return `${versions} · ${formatBytes(stats.logicalBytes)} of history, kept in ${formatBytes(stats.storedBytes)}`;
}

function startOfDay(d: Date): number {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
}

// Buckets a date relative to now. `withTime` is false once we've fallen back
// to a plain calendar date, which already reads fine without a clock time.
function dayLabel(date: Date, now: Date): { text: string; withTime: boolean } {
  const dayDiff = Math.round((startOfDay(now) - startOfDay(date)) / 86_400_000);
  if (dayDiff <= 0) return { text: "today", withTime: true };
  if (dayDiff === 1) return { text: "yesterday", withTime: true };
  if (dayDiff < 7) return { text: `${dayDiff} days ago`, withTime: true };

  // Not toLocaleDateString: "11/6/2026" is ambiguous between the 11th of June
  // and the 6th of November, and in a timeline you are often trying to place a
  // version precisely. A named month can't be misread. The year appears only
  // when it isn't the current one, so a busy year stays uncluttered.
  const sameYear = date.getFullYear() === now.getFullYear();
  return {
    text: date.toLocaleDateString(undefined, {
      day: "numeric",
      month: "short",
      ...(sameYear ? {} : { year: "numeric" }),
    }),
    withTime: false,
  };
}

// Timestamps from the backend are unix seconds, not milliseconds.
export function formatRelativeTime(unixSeconds: number): string {
  const date = new Date(unixSeconds * 1000);
  const time = date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  const { text, withTime } = dayLabel(date, new Date());
  return withTime ? `${text} ${time}` : text;
}

// A work session's time span, e.g. "today 14:02 – 16:40". Labeled by the
// session's start (its oldest version) since a session rarely crosses
// midnight - it's defined by gaps under SESSION_GAP_SECONDS.
export function formatSessionSpan(startUnixSeconds: number, endUnixSeconds: number): string {
  const start = new Date(startUnixSeconds * 1000);
  const end = new Date(endUnixSeconds * 1000);
  const { text, withTime } = dayLabel(start, new Date());
  if (!withTime) return text;
  const startTime = start.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  const endTime = end.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  return `${text} ${startTime} – ${endTime}`;
}
