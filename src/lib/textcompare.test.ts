import { describe, expect, it } from "vitest";
import { baselineSizePt, collapseUnchanged, pieceStyle } from "./textcompare";
import type { Run, TextRow } from "./types";

function run(text: string, overrides: Partial<Run> = {}): Run {
  return {
    text,
    bold: false,
    italic: false,
    underline: false,
    sizePt: null,
    font: null,
    color: null,
    ...overrides,
  };
}

function same(text: string): TextRow {
  return {
    status: "same",
    left: text,
    right: text,
    leftSpans: [],
    rightSpans: [],
    leftRuns: [run(text)],
    rightRuns: [run(text)],
    formatNote: "",
  };
}

function removed(text: string): TextRow {
  return {
    status: "removed",
    left: text,
    right: null,
    leftSpans: [],
    rightSpans: [],
    leftRuns: [run(text)],
    rightRuns: [],
    formatNote: "",
  };
}

describe("collapseUnchanged", () => {
  it("leaves a short run of unchanged lines alone", () => {
    const rows = [same("a"), same("b"), removed("c"), same("d")];
    expect(collapseUnchanged(rows)).toEqual(
      rows.map((row, i) => ({ kind: "row", row, line: i + 1 })),
    );
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

    expect(result[0]).toEqual({ kind: "row", row: rows[0], line: 1 });
    expect(result[1]).toEqual({ kind: "row", row: same("1"), line: 2 });
    expect(result[2]).toEqual({ kind: "row", row: same("2"), line: 3 });
    expect(result[3]).toEqual({ kind: "collapsed", count: 4 });
    expect(result[4]).toEqual({ kind: "row", row: same("7"), line: 8 });
    expect(result[5]).toEqual({ kind: "row", row: same("8"), line: 9 });
    expect(result[6]).toEqual({ kind: "row", row: rows[9], line: 10 });
    expect(result).toHaveLength(7);
  });

  it("never collapses when nothing is unchanged", () => {
    const rows = [removed("a"), removed("b")];
    expect(collapseUnchanged(rows)).toEqual(
      rows.map((row, i) => ({ kind: "row", row, line: i + 1 })),
    );
  });

  // A long run of unchanged lines that reaches the end of the document has
  // nothing after it to give context for - showing the document's literal
  // last two lines looked like an unexplained leftover, not context for
  // anything.
  it("drops trailing context when the unchanged run reaches the end of the document", () => {
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
    ];

    const result = collapseUnchanged(rows);

    expect(result[0]).toEqual({ kind: "row", row: rows[0], line: 1 });
    expect(result[1]).toEqual({ kind: "row", row: same("1"), line: 2 });
    expect(result[2]).toEqual({ kind: "row", row: same("2"), line: 3 });
    expect(result[3]).toEqual({ kind: "collapsed", count: 6 });
    expect(result).toHaveLength(4);
  });

  // Symmetrically, a run starting at row 1 has nothing before it either.
  it("drops leading context when the unchanged run starts at the beginning of the document", () => {
    const rows = [
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

    expect(result[0]).toEqual({ kind: "collapsed", count: 6 });
    expect(result[1]).toEqual({ kind: "row", row: same("7"), line: 7 });
    expect(result[2]).toEqual({ kind: "row", row: same("8"), line: 8 });
    expect(result[3]).toEqual({ kind: "row", row: rows[8], line: 9 });
    expect(result).toHaveLength(4);
  });
});

describe("baselineSizePt", () => {
  it("picks the size covering the most characters, not the most runs", () => {
    const rows: TextRow[] = [
      {
        status: "same",
        left: "x",
        right: "x",
        leftSpans: [],
        rightSpans: [],
        leftRuns: [run("A heading", { sizePt: 28 }), run("ten short bits of 11pt body text", { sizePt: 11 })],
        rightRuns: [],
        formatNote: "",
      },
    ];
    expect(baselineSizePt(rows)).toBe(11);
  });

  it("defaults to 12 when nothing in the comparison carries a size", () => {
    expect(baselineSizePt([same("Plain text")])).toBe(12);
  });
});

describe("pieceStyle", () => {
  it("renders bold, italic and a proportional size", () => {
    const style = pieceStyle(run("Heading", { bold: true, italic: true, sizePt: 18 }), 12, null);
    expect(style).toContain("font-weight: bold");
    expect(style).toContain("font-style: italic");
    expect(style).toContain("font-size: 1.50em");
  });

  it("clamps an extreme size so nothing becomes unreadable or takes over the pane", () => {
    const huge = pieceStyle(run("Title", { sizePt: 96 }), 12, null);
    expect(huge).toContain("font-size: 1.70em");

    const tiny = pieceStyle(run("Footnote", { sizePt: 2 }), 12, null);
    expect(tiny).toContain("font-size: 0.85em");
  });

  it("lets a document colour win over the interface's removed/added colour", () => {
    const style = pieceStyle(run("word", { color: "FF0000" }), 12, "removed");
    expect(style).toContain("color: #FF0000");
    expect(style).not.toContain("var(--diff-remove)");
  });

  it("falls back to the interface colour when the document carries none", () => {
    const style = pieceStyle(run("word"), 12, "added");
    expect(style).toContain("color: var(--diff-add)");
  });

  it("combines a document underline with the removed strikethrough", () => {
    const style = pieceStyle(run("word", { underline: true }), 12, "removed");
    expect(style).toContain("text-decoration-line: underline line-through");
  });
});
