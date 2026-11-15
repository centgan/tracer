// Turns a flat TextRow[] (compare_part_text's result) into what the
// side-by-side view actually renders: long runs of unchanged lines folded
// into a single "N lines unchanged" marker, with a little context kept on
// each side so it stays obvious what was collapsed.

import type { TextRow } from "./types";

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
