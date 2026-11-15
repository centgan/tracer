export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const kb = bytes / 1024;
  if (kb < 1024) return `${kb.toFixed(kb < 10 ? 1 : 0)} KB`;
  const mb = kb / 1024;
  return `${mb.toFixed(mb < 10 ? 1 : 0)} MB`;
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
  return { text: date.toLocaleDateString(), withTime: false };
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
