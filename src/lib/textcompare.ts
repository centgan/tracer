// Turns a flat TextRow[] (compare_part_text's result) into what the
// side-by-side view actually renders: long runs of unchanged lines folded
// into a single "N lines unchanged" marker, with a little context kept on
// each side so it stays obvious what was collapsed.

import type { TextRow } from "./types";

export type DisplayRow = { kind: "row"; row: TextRow } | { kind: "collapsed"; count: number };

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
      out.push({ kind: "row", row: rows[i] });
      i += 1;
      continue;
    }

    let j = i;
    while (j < rows.length && rows[j].status === "same") j += 1;
    const runLength = j - i;

    if (runLength <= MIN_RUN_TO_COLLAPSE) {
      for (let k = i; k < j; k += 1) out.push({ kind: "row", row: rows[k] });
    } else {
      for (let k = i; k < i + CONTEXT; k += 1) out.push({ kind: "row", row: rows[k] });
      out.push({ kind: "collapsed", count: runLength - CONTEXT * 2 });
      for (let k = j - CONTEXT; k < j; k += 1) out.push({ kind: "row", row: rows[k] });
    }

    i = j;
  }

  return out;
}
