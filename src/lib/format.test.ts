import { describe, expect, it } from "vitest";
import { formatBytes, formatFileStats, formatSessionSpan } from "./format";

describe("formatBytes", () => {
  it("renders gigabytes with one decimal even at large magnitudes", () => {
    expect(formatBytes(11.7 * 1024 ** 3)).toBe("11.7 GB");
  });
});

describe("formatFileStats", () => {
  it("pluralizes a single version correctly", () => {
    expect(formatFileStats({ versionCount: 1, logicalBytes: 100, storedBytes: 100 })).toBe(
      "1 version",
    );
  });

  it("shows only the count when nothing has been saved yet", () => {
    expect(formatFileStats({ versionCount: 1, logicalBytes: 1000, storedBytes: 1000 })).toBe(
      "1 version",
    );
    // storedBytes can exceed logicalBytes slightly for tiny files (container
    // overhead); never claim a negative saving either.
    expect(formatFileStats({ versionCount: 1, logicalBytes: 1000, storedBytes: 1200 })).toBe(
      "1 version",
    );
  });

  it("shows the saving once there is one", () => {
    expect(
      formatFileStats({
        versionCount: 312,
        logicalBytes: 11.7 * 1024 ** 3,
        storedBytes: 94 * 1024 ** 2,
      }),
    ).toBe("312 versions · 11.7 GB of history, kept in 94 MB");
  });
});

describe("formatSessionSpan", () => {
  const at = (h: number, m: number) => new Date(2026, 10, 15, h, m).getTime() / 1000;

  it("says the meridiem once so the span fits its column", () => {
    const span = formatSessionSpan(at(14, 22), at(14, 37));
    expect(span).toContain("–");
    // The old format repeated "PM" twice and got ellipsized away.
    expect(span.match(/[AP]M/g)?.length ?? 0).toBeLessThanOrEqual(1);
    expect(span.length).toBeLessThan(22);
  });

  it("falls back to a plain date once a session is old enough", () => {
    const old = new Date(2020, 0, 2, 9, 0).getTime() / 1000;
    expect(formatSessionSpan(old, old + 600)).not.toContain("–");
  });
});
