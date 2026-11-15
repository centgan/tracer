// Turns a raw zip entry name (OOXML jargon like "ppt/slides/slide4.xml")
// into something a person who has never heard of OOXML recognises, or marks
// it as internal bookkeeping that isn't worth showing at all. Guessing wrong
// reads worse than saying "other internal changes", so anything this module
// doesn't recognise is internal rather than a prettified guess at its path.

import type { PartChange } from "./types";

export type PartLabel = { kind: "named"; label: string } | { kind: "internal" };

const NAMED: [RegExp, (m: RegExpMatchArray) => string][] = [
  [/^ppt\/slides\/slide(\d+)\.xml$/, (m) => `Slide ${m[1]}`],
  [/^ppt\/notesSlides\/notesSlide(\d+)\.xml$/, (m) => `Notes on slide ${m[1]}`],
  [/^word\/document\.xml$/, () => "The document text"],
  [/^word\/footnotes\.xml$/, () => "Footnotes"],
  [/^word\/endnotes\.xml$/, () => "Endnotes"],
  [/^word\/header(\d+)\.xml$/, (m) => `Header ${m[1]}`],
  [/^word\/footer(\d+)\.xml$/, (m) => `Footer ${m[1]}`],
  [/^xl\/worksheets\/sheet(\d+)\.xml$/, (m) => `Sheet ${m[1]}`],
  [/^xl\/sharedStrings\.xml$/, () => "Cell text"],
  [/(?:^|\/)theme\/theme\d+\.xml$/, () => "Theme"],
  [/(?:^|\/)styles\.xml$/, () => "Styles"],
];

// docProps/*, [Content_Types].xml, anything under a _rels/ folder, and
// xl/calcChain.xml change on nearly every save and mean nothing to a user -
// checked before NAMED so none of those paths can ever reach a named label.
function isInternal(entryName: string): boolean {
  if (entryName.startsWith("docProps/")) return true;
  if (entryName === "[Content_Types].xml") return true;
  if (entryName === "xl/calcChain.xml") return true;
  if (entryName.split("/").includes("_rels")) return true;
  return false;
}

function describeMedia(entryName: string): string | null {
  const match = entryName.match(/^(?:ppt|word|xl)\/media\/([^/]+)$/);
  if (!match) return null;
  const filename = match[1];
  const imageMatch = filename.match(/^image(\d+)\.\w+$/i);
  return imageMatch ? `Picture ${imageMatch[1]}` : filename;
}

// Classifies one zip entry name. Pure and total: every string in, every
// string out, nothing thrown.
export function describeEntry(entryName: string): PartLabel {
  if (isInternal(entryName)) return { kind: "internal" };

  const media = describeMedia(entryName);
  if (media !== null) return { kind: "named", label: media };

  for (const [pattern, toLabel] of NAMED) {
    const match = entryName.match(pattern);
    if (match) return { kind: "named", label: toLabel(match) };
  }

  return { kind: "internal" };
}

// A named change, carrying both what to show (label) and what to ask the
// backend for if the user clicks it (entryName, the raw zip path).
export type NamedChange = { entryName: string; label: string };

// A compare_revisions result, sorted into what a user should actually read:
// friendly labels grouped by status, with internal/bookkeeping entries
// reduced to a count instead of named individually.
export type ChangeSummary = {
  changed: NamedChange[];
  added: NamedChange[];
  removed: NamedChange[];
  internalCount: number;
};

export function summarizeChanges(changes: PartChange[]): ChangeSummary {
  const summary: ChangeSummary = { changed: [], added: [], removed: [], internalCount: 0 };
  for (const change of changes) {
    const described = describeEntry(change.entryName);
    if (described.kind === "internal") {
      summary.internalCount += 1;
      continue;
    }
    const bucket = change.status === "changed" ? summary.changed : change.status === "added" ? summary.added : summary.removed;
    bucket.push({ entryName: change.entryName, label: described.label });
  }
  return summary;
}

// Mirrors the whitelist src-tauri/src/engine.rs's extract_text recognises.
// Kept in lockstep deliberately: a part outside it is one the backend can
// never pull text out of (a picture, a theme, a worksheet's cell grid), so
// there's no point round-tripping to compare_part_text just to be told so -
// it shouldn't even look clickable.
export function supportsTextCompare(entryName: string): boolean {
  if (entryName.startsWith("ppt/slides/slide")) return true;
  if (entryName.startsWith("ppt/notesSlides/")) return true;
  if (entryName === "word/document.xml") return true;
  if (entryName === "word/footnotes.xml") return true;
  if (entryName === "word/endnotes.xml") return true;
  if (entryName.startsWith("word/header")) return true;
  if (entryName.startsWith("word/footer")) return true;
  if (entryName === "xl/sharedStrings.xml") return true;
  return false;
}
