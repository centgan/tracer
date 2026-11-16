// Turns a flat TextRow[] (compare_part_text's result) into what the
// side-by-side view actually renders: long runs of unchanged lines folded
// into a single "N lines unchanged" marker, with a little context kept on
// each side so it stays obvious what was collapsed.

import type { Run, TextRow } from "./types";

// `line` is the row's 1-based position in the *full* rows array (not the
// collapsed display list), so it survives folding: a paragraph after a
// hidden run keeps its real number instead of restarting from the count of
// what's still visible. That's what lets the gutter make "paragraph 21"
// mean something even when 4-20 are hidden.
export type DisplayRow =
  | { kind: "row"; row: TextRow; line: number }
  | { kind: "collapsed"; count: number };

// How many unchanged lines to keep visible around a collapsed run.
const CONTEXT = 2;
// A run shorter than this just isn't worth folding - collapsing it would
// hide as much as it shows.
const MIN_RUN_TO_COLLAPSE = CONTEXT * 2 + 3;

export function collapseUnchanged(rows: TextRow[]): DisplayRow[] {
  const out: DisplayRow[] = [];
  let i = 0;

  while (i < rows.length) {
    if (rows[i].status !== "same") {
      out.push({ kind: "row", row: rows[i], line: i + 1 });
      i += 1;
      continue;
    }

    let j = i;
    while (j < rows.length && rows[j].status === "same") j += 1;
    const runLength = j - i;

    if (runLength <= MIN_RUN_TO_COLLAPSE) {
      for (let k = i; k < j; k += 1) out.push({ kind: "row", row: rows[k], line: k + 1 });
    } else {
      for (let k = i; k < i + CONTEXT; k += 1) out.push({ kind: "row", row: rows[k], line: k + 1 });
      out.push({ kind: "collapsed", count: runLength - CONTEXT * 2 });
      for (let k = j - CONTEXT; k < j; k += 1) out.push({ kind: "row", row: rows[k], line: k + 1 });
    }

    i = j;
  }

  return out;
}

// Rendering a line the way the document does (DESIGN.md's "Showing
// formatting"): sizes scale in proportion to the comparison's own content
// rather than in absolute points, so a 28pt title in a list of 11pt body
// text reads as a title without taking over the pane, and a font family is
// named in the format note rather than ever set as CSS font-family - we
// can't know whether the reader has it installed, and a silent substitution
// would show a difference that isn't there or hide one that is.
const MIN_SCALE = 0.85;
const MAX_SCALE = 1.7;

// The comparison's most common run size, by character count - the baseline
// everything else scales against. 12pt when nothing in the comparison
// carries a size at all (plain text has nothing to be proportional to).
export function baselineSizePt(rows: TextRow[]): number {
  const weightBySize = new Map<number, number>();
  for (const row of rows) {
    for (const run of [...row.leftRuns, ...row.rightRuns, ...row.leftSpans, ...row.rightSpans]) {
      if (run.sizePt == null) continue;
      weightBySize.set(run.sizePt, (weightBySize.get(run.sizePt) ?? 0) + run.text.length);
    }
  }
  let best = 12;
  let bestWeight = -1;
  for (const [size, weight] of weightBySize) {
    if (weight > bestWeight) {
      best = size;
      bestWeight = weight;
    }
  }
  return best;
}

function scaleFor(sizePt: number | null, baseline: number): number {
  if (sizePt == null || baseline <= 0) return 1;
  return Math.min(MAX_SCALE, Math.max(MIN_SCALE, sizePt / baseline));
}

// A piece of formatted text on either side of a "changed" row: a document
// Run (whole-line rendering) or a TextSpan (word-marked rendering) - both
// carry the same formatting fields, which is all this needs.
type Formatted = Pick<Run, "bold" | "italic" | "underline" | "sizePt" | "color">;

// Inline style for one rendered piece. `markedAs` is the diff direction
// ("removed"/"added") when this piece is itself a changed word or a whole
// added/removed line, `null` for plain content - it supplies the interface
// marking colour and decoration, but only as a fallback: a colour or
// underline the *document* actually carries always wins, so a red word in
// someone's document can never be mistaken for our deletion red (DESIGN.md's
// "Comparing text" / "Showing formatting").
export function pieceStyle(
  piece: Formatted,
  baseline: number,
  markedAs: "removed" | "added" | null,
): string {
  const parts: string[] = [];
  if (piece.bold) parts.push("font-weight: bold");
  if (piece.italic) parts.push("font-style: italic");
  if (piece.sizePt != null) parts.push(`font-size: ${scaleFor(piece.sizePt, baseline).toFixed(2)}em`);

  const color = piece.color ? `#${piece.color}` : markedAs === "removed" ? "var(--diff-remove)" : markedAs === "added" ? "var(--diff-add)" : null;
  if (color) parts.push(`color: ${color}`);

  const decorations: string[] = [];
  if (piece.underline) decorations.push("underline");
  if (markedAs === "removed") decorations.push("line-through");
  if (markedAs === "added") decorations.push("underline");
  if (decorations.length > 0) {
    parts.push(`text-decoration-line: ${[...new Set(decorations)].join(" ")}`);
  }

  return parts.join("; ");
}
