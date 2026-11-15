import { describe, expect, it } from "vitest";
import { formatBytes, formatFileStats } from "./format";

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
