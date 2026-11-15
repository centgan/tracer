import { describe, expect, it } from "vitest";
import { collapseUnchanged } from "./textcompare";
import type { TextRow } from "./types";

function same(text: string): TextRow {
  return { status: "same", left: text, right: text, leftSpans: [], rightSpans: [] };
}

function removed(text: string): TextRow {
  return { status: "removed", left: text, right: null, leftSpans: [], rightSpans: [] };
}

describe("collapseUnchanged", () => {
  it("leaves a short run of unchanged lines alone", () => {
    const rows = [same("a"), same("b"), removed("c"), same("d")];
    expect(collapseUnchanged(rows)).toEqual(rows.map((row) => ({ kind: "row", row })));
  });

  it("folds a long run of unchanged lines, keeping context on each side", () => {
    const rows = [
      removed("before"),
      same("1"),
      same("2"),
      same("3"),
      same("4"),
      same("5"),
      same("6"),
      same("7"),
      same("8"),
      removed("after"),
    ];

    const result = collapseUnchanged(rows);

    expect(result[0]).toEqual({ kind: "row", row: rows[0] });
    expect(result[1]).toEqual({ kind: "row", row: same("1") });
    expect(result[2]).toEqual({ kind: "row", row: same("2") });
    expect(result[3]).toEqual({ kind: "collapsed", count: 4 });
    expect(result[4]).toEqual({ kind: "row", row: same("7") });
    expect(result[5]).toEqual({ kind: "row", row: same("8") });
    expect(result[6]).toEqual({ kind: "row", row: rows[9] });
    expect(result).toHaveLength(7);
  });

  it("never collapses when nothing is unchanged", () => {
    const rows = [removed("a"), removed("b")];
    expect(collapseUnchanged(rows)).toEqual(rows.map((row) => ({ kind: "row", row })));
  });
});
