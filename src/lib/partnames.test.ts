import { describe, expect, it } from "vitest";
import { describeEntry, summarizeChanges, supportsTextCompare } from "./partnames";
import type { PartChange } from "./types";

describe("describeEntry", () => {
  it.each([
    ["ppt/slides/slide4.xml", "Slide 4"],
    ["ppt/notesSlides/notesSlide2.xml", "Notes on slide 2"],
    ["word/document.xml", "The document text"],
    ["word/footnotes.xml", "Footnotes"],
    ["word/endnotes.xml", "Endnotes"],
    ["word/header1.xml", "Header 1"],
    ["word/footer1.xml", "Footer 1"],
    ["xl/worksheets/sheet1.xml", "Sheet 1"],
    ["xl/sharedStrings.xml", "Cell text"],
    ["ppt/theme/theme1.xml", "Theme"],
    ["word/theme/theme1.xml", "Theme"],
    ["word/styles.xml", "Styles"],
  ])("maps %s to %s", (entryName, label) => {
    expect(describeEntry(entryName)).toEqual({ kind: "named", label });
  });

  it("names a picture by its number", () => {
    expect(describeEntry("ppt/media/image3.png")).toEqual({ kind: "named", label: "Picture 3" });
    expect(describeEntry("word/media/image12.jpeg")).toEqual({
      kind: "named",
      label: "Picture 12",
    });
  });

  it("falls back to the file's own name for a non-imageN media entry", () => {
    expect(describeEntry("ppt/media/logo.emf")).toEqual({ kind: "named", label: "logo.emf" });
  });

  it("parses multi-digit sheet and slide numbers", () => {
    expect(describeEntry("ppt/slides/slide42.xml")).toEqual({ kind: "named", label: "Slide 42" });
    expect(describeEntry("xl/worksheets/sheet10.xml")).toEqual({
      kind: "named",
      label: "Sheet 10",
    });
  });

  it("classifies bookkeeping parts as internal rather than naming them", () => {
    expect(describeEntry("docProps/core.xml")).toEqual({ kind: "internal" });
    expect(describeEntry("docProps/app.xml")).toEqual({ kind: "internal" });
    expect(describeEntry("[Content_Types].xml")).toEqual({ kind: "internal" });
    expect(describeEntry("xl/calcChain.xml")).toEqual({ kind: "internal" });
    expect(describeEntry("_rels/.rels")).toEqual({ kind: "internal" });
    expect(describeEntry("word/_rels/document.xml.rels")).toEqual({ kind: "internal" });
  });

  it("never shows docProps/core.xml as a named change", () => {
    const result = describeEntry("docProps/core.xml");
    expect(result.kind).not.toBe("named");
  });

  it("treats an unrecognised path as internal instead of guessing a label", () => {
    expect(describeEntry("ppt/presentation.xml")).toEqual({ kind: "internal" });
    expect(describeEntry("some/made/up/path.xml")).toEqual({ kind: "internal" });
  });
});

function change(entryName: string, status: PartChange["status"]): PartChange {
  return { entryName, status };
}

describe("summarizeChanges", () => {
  it("groups named entries by status and counts internal ones instead of naming them", () => {
    const summary = summarizeChanges([
      change("ppt/slides/slide4.xml", "changed"),
      change("ppt/media/image9.png", "added"),
      change("ppt/slides/slide7.xml", "removed"),
      change("docProps/core.xml", "changed"),
      change("[Content_Types].xml", "changed"),
    ]);

    expect(summary.changed).toEqual([{ entryName: "ppt/slides/slide4.xml", label: "Slide 4" }]);
    expect(summary.added).toEqual([{ entryName: "ppt/media/image9.png", label: "Picture 9" }]);
    expect(summary.removed).toEqual([{ entryName: "ppt/slides/slide7.xml", label: "Slide 7" }]);
    expect(summary.internalCount).toBe(2);
  });

  it("returns every bucket empty when only internal parts changed", () => {
    const summary = summarizeChanges([change("docProps/app.xml", "changed")]);

    expect(summary).toEqual({ changed: [], added: [], removed: [], internalCount: 1 });
  });

  it("handles no changes at all", () => {
    expect(summarizeChanges([])).toEqual({ changed: [], added: [], removed: [], internalCount: 0 });
  });
});

describe("supportsTextCompare", () => {
  it.each([
    "ppt/slides/slide4.xml",
    "ppt/notesSlides/notesSlide2.xml",
    "word/document.xml",
    "word/footnotes.xml",
    "word/endnotes.xml",
    "word/header1.xml",
    "word/footer1.xml",
    "xl/sharedStrings.xml",
  ])("says yes for %s", (entryName) => {
    expect(supportsTextCompare(entryName)).toBe(true);
  });

  it.each([
    "ppt/media/image3.png",
    "word/media/image12.jpeg",
    "xl/worksheets/sheet1.xml",
    "ppt/theme/theme1.xml",
    "word/styles.xml",
    "docProps/core.xml",
  ])("says no for %s - a picture or a part with no text to extract", (entryName) => {
    expect(supportsTextCompare(entryName)).toBe(false);
  });
});
