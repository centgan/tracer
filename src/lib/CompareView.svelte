<script lang="ts">
  import type { Loadable, Revision, TextComparison } from "./types";
  import { collapseUnchanged } from "./textcompare";
  import { formatDateOnly, versionLabel } from "./format";

  // This is the one screen that takes over the whole window (see
  // DESIGN.md's "Comparing text") - the caller swaps it in for the folders/
  // files/timeline panes and is responsible for keeping those mounted
  // underneath so Escape/Done can return to them unchanged.
  let {
    older,
    newer,
    fileName,
    partLabel,
    onLoad,
    onDone,
  }: {
    older: Revision;
    newer: Revision;
    fileName: string;
    partLabel: string;
    onLoad: () => Promise<TextComparison>;
    onDone: () => void;
  } = $props();

  let comparison = $state<Loadable<TextComparison>>({ status: "loading" });

  async function load() {
    comparison = { status: "loading" };
    try {
      comparison = { status: "loaded", data: await onLoad() };
    } catch (e) {
      comparison = {
        status: "error",
        message: e instanceof Error ? e.message : "Couldn't load that comparison.",
      };
    }
  }
  load();

  let displayRows = $derived(
    comparison.status === "loaded" ? collapseUnchanged(comparison.data.rows) : [],
  );

  // Every line a reader would actually want to jump to - the ones that
  // changed. Unchanged text is context, so it never gets a stop; without
  // this, a long document means scrolling to hunt for what moved.
  let changedLines = $derived(
    comparison.status === "loaded"
      ? comparison.data.rows.flatMap((row, i) => (row.status === "same" ? [] : [i + 1]))
      : [],
  );

  let cursor = $state(0);

  // Keyed by line number so prev/next can jump straight to a target instead
  // of walking the DOM. Populated as each row mounts via `trackLine` below.
  const lineEls = new Map<number, HTMLElement>();

  function trackLine(node: HTMLElement, line: number) {
    lineEls.set(line, node);
    return {
      destroy() {
        if (lineEls.get(line) === node) lineEls.delete(line);
      },
    };
  }

  function jumpTo(target: number) {
    if (changedLines.length === 0) return;
    cursor = ((target % changedLines.length) + changedLines.length) % changedLines.length;
    const reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    lineEls.get(changedLines[cursor])?.scrollIntoView({
      block: "center",
      behavior: reduceMotion ? "auto" : "smooth",
    });
  }

  function next() {
    jumpTo(cursor + 1);
  }

  function prev() {
    jumpTo(cursor - 1);
  }

  $effect(() => {
    function onKeydown(e: KeyboardEvent) {
      if (e.key === "Escape") onDone();
    }
    window.addEventListener("keydown", onKeydown);
    return () => window.removeEventListener("keydown", onKeydown);
  });
</script>

<section class="compare-view">
  <div class="bar">
    <h2>{partLabel} <span class="file-name">— {fileName}</span></h2>
    {#if comparison.status === "loaded" && comparison.data.hasText}
      <span class="change-count">
        {changedLines.length} change{changedLines.length === 1 ? "" : "s"}
      </span>
      <div class="nav">
        <button onclick={prev} disabled={changedLines.length === 0} aria-label="Previous change"
          >‹ prev</button
        >
        <button onclick={next} disabled={changedLines.length === 0} aria-label="Next change"
          >next ›</button
        >
      </div>
    {/if}
    <button class="done" onclick={onDone}>Done</button>
  </div>

  {#if comparison.status === "loading"}
    <p class="hint pad">Loading…</p>
  {:else if comparison.status === "error"}
    <p class="error-text pad">{comparison.message}</p>
  {:else if !comparison.data.hasText}
    <p class="hint pad">Text comparison isn't available for this part.</p>
  {:else}
    <div class="well text-well">
      <div class="text-columns">
        <div class="headers">
          <div class="column-header removed-header">
            <span class="marker" aria-hidden="true">−</span>
            <span class="who">
              <span class="who-name">{versionLabel(older)}</span>
              <span class="who-meta"
                >{formatDateOnly(older.timestamp)}{older.authorName
                  ? ` · ${older.authorName}`
                  : ""}</span
              >
            </span>
          </div>
          <div class="column-header added-header">
            <span class="marker" aria-hidden="true">+</span>
            <span class="who">
              <span class="who-name">{versionLabel(newer)}</span>
              <span class="who-meta"
                >{formatDateOnly(newer.timestamp)}{newer.authorName
                  ? ` · ${newer.authorName}`
                  : ""}</span
              >
            </span>
          </div>
        </div>
        {#each displayRows as item}
          {#if item.kind === "collapsed"}
            <div class="collapsed-marker">
              {item.count} unchanged line{item.count === 1 ? "" : "s"} hidden
            </div>
          {:else if item.row.status === "same"}
            <div class="full-row same-row" use:trackLine={item.line}>
              <span class="gutter" aria-hidden="true">{item.line}</span>
              <span class="line-text">{item.row.left}</span>
            </div>
          {:else}
            <div
              class="text-cell left-cell"
              class:removed={item.row.status === "removed"}
              class:empty={item.row.left === null}
              use:trackLine={item.line}
            >
              {#if item.row.left !== null}
                <span class="gutter" aria-hidden="true">{item.line}</span>
                <span class="line-text">
                  {#if item.row.status === "removed" || item.row.status === "changed"}
                    <span class="marker" aria-hidden="true">−&nbsp;</span>
                  {/if}
                  {#if item.row.leftSpans.length > 0}
                    {#each item.row.leftSpans as span}<span class:diff-word-removed={span.changed}
                        >{span.text}</span
                      >{/each}
                  {:else}
                    {item.row.left}
                  {/if}
                </span>
              {/if}
            </div>
            <div
              class="text-cell right-cell"
              class:added={item.row.status === "added"}
              class:empty={item.row.right === null}
            >
              {#if item.row.right !== null}
                <span class="gutter" aria-hidden="true">{item.line}</span>
                <span class="line-text">
                  {#if item.row.status === "added" || item.row.status === "changed"}
                    <span class="marker" aria-hidden="true">+&nbsp;</span>
                  {/if}
                  {#if item.row.rightSpans.length > 0}
                    {#each item.row.rightSpans as span}<span class:diff-word-added={span.changed}
                        >{span.text}</span
                      >{/each}
                  {:else}
                    {item.row.right}
                  {/if}
                </span>
              {/if}
            </div>
          {/if}
        {/each}
      </div>
    </div>
  {/if}
</section>

<style>
  .compare-view {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
  }

  .bar {
    flex: none;
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 6px 10px;
    background: var(--chrome);
    border-bottom: 1px solid var(--edge);
  }

  .bar h2 {
    font-size: 13px;
    font-weight: bold;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: 1;
  }

  .file-name {
    font-weight: normal;
    opacity: 0.72;
  }

  .change-count {
    flex: none;
    font-size: 11px;
    opacity: 0.72;
  }

  .nav {
    flex: none;
    display: flex;
    gap: 4px;
  }

  .done {
    flex: none;
  }

  .pad {
    padding: 10px;
  }

  .well.text-well {
    flex: 1;
    min-height: 0;
    margin: 8px;
    overflow-y: auto;
    overflow-x: auto;
  }

  .text-columns {
    display: grid;
    grid-template-columns: 1fr 1fr;
    column-gap: 10px;
    align-items: start;
    /* A rule down the middle, so the split between older and newer is drawn
       rather than implied by alignment. Without it a changed line's two halves
       are only related by where they happen to sit. */
    background: linear-gradient(
        color-mix(in srgb, var(--ink) 18%, transparent),
        color-mix(in srgb, var(--ink) 18%, transparent)
      )
      no-repeat center top / 1px 100%;
  }

  @media (max-width: 760px) {
    /* Stacked: there are no columns left to divide. */
    .text-columns {
      background: none;
    }
  }

  /* One sticky unit holding both halves, so the reader never has to scroll
     up to remember which side is which. Pinning the two halves separately
     would fight over the same `top: 0` as soon as one scrolled underneath
     the other; wrapping them together means there's only ever one sticky
     element, at any width. */
  .headers {
    grid-column: 1 / -1;
    position: sticky;
    top: 0;
    z-index: 1;
    display: flex;
    background: var(--well);
  }

  /* Tinted to match the body's own removed/added colours so the association
     between a side and its meaning is made once here and holds all the way
     down. */
  .column-header {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: flex-start;
    gap: 6px;
    font-size: 11px;
    padding: 6px 10px 8px;
    border-bottom: 1px solid var(--edge);
  }

  .column-header .marker {
    font-weight: bold;
    font-size: 14px;
    line-height: 1.2;
  }

  /* Opaque, unlike --diff-remove-bg/--diff-add-bg used in the body - this
     header sits sticky over scrolling rows, and a translucent tint would let
     their text bleed through underneath it. color-mix bakes the same tint
     against --well as a solid colour instead. */
  .removed-header {
    background: color-mix(in srgb, var(--diff-remove) 14%, var(--well));
  }

  .removed-header .marker {
    color: var(--diff-remove);
  }

  .added-header {
    background: color-mix(in srgb, var(--diff-add) 14%, var(--well));
  }

  .added-header .marker {
    color: var(--diff-add);
  }

  .who {
    min-width: 0;
    display: flex;
    flex-direction: column;
  }

  .who-name {
    font-weight: bold;
    font-size: 13px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .who-meta {
    opacity: 0.72;
  }

  /* Spans both columns: an unchanged line (shown once, not duplicated per
     side) and the "N lines hidden" marker both read better full-width. */
  .full-row {
    grid-column: 1 / -1;
    /* Opaque, so the column rule behind it doesn't run through the middle of a
       sentence and make an unchanged paragraph look split in two. */
    background: var(--well);
  }

  /* Fading the text, not the row: element opacity would make the row's own
     background translucent too, letting the column rule show through the
     middle of an unchanged sentence. */
  .same-row .line-text,
  .same-row .gutter {
    opacity: 0.68;
  }

  .collapsed-marker {
    /* Opaque for the same reason as .full-row - it spans both columns. */
    background: var(--well);
    grid-column: 1 / -1;
    padding: 4px 0;
    text-align: center;
    font-size: 11px;
    font-style: italic;
    opacity: 0.6;
  }

  /* Every line pairs a gutter number with its text in its own two-column
     grid, so a wrapped paragraph's continuation indents under the text
     rather than under the number. */
  .same-row,
  .text-cell {
    display: grid;
    grid-template-columns: 2em 1fr;
    column-gap: 6px;
    padding: 1px 10px;
    font-size: 13px;
    line-height: 1.5;
    min-height: 1.5em;
  }

  .gutter {
    font-family: var(--data);
    font-size: 11px;
    text-align: right;
    opacity: 0.5;
    padding-top: 0.2em;
  }

  .line-text {
    white-space: pre-wrap;
    overflow-wrap: break-word;
  }

  .text-cell.removed {
    background: var(--diff-remove-bg);
  }

  /* A paragraph that is gone entirely gets struck through, in the removed
     colour, on the older side only. A tint alone left it looking like a line
     that merely moved; this says deleted. */
  .text-cell.removed .line-text {
    color: var(--diff-remove);
    text-decoration: line-through;
    text-decoration-thickness: 1px;
  }

  .text-cell.removed .marker,
  .text-cell.added .marker {
    text-decoration: none;
  }

  .text-cell.added {
    background: var(--diff-add-bg);
  }

  .text-cell.added .line-text {
    color: var(--diff-add);
  }

  /* A changed line shows on both sides, so stacked it would otherwise be two
     rows with the same number and no way to tell which is the older one. */
  .text-cell.left-cell .marker {
    color: var(--diff-remove);
  }

  .text-cell.right-cell .marker {
    color: var(--diff-add);
  }

  /* The marker is the colour-blind-safe signal a whole line was added or
     removed, independent of the background tint. */
  .text-cell.removed .marker {
    color: var(--diff-remove);
    font-weight: bold;
  }

  .text-cell.added .marker {
    color: var(--diff-add);
    font-weight: bold;
  }

  /* Word-level marks inside a "changed" row. Colour plus a second, non-colour
     channel (strikethrough on the old word, underline on the new one) so the
     exact words that differ are legible without relying on hue. */
  .diff-word-removed {
    background: var(--diff-remove-bg);
    color: var(--diff-remove);
    text-decoration: line-through;
  }

  .diff-word-added {
    background: var(--diff-add-bg);
    color: var(--diff-add);
    text-decoration: underline;
    text-decoration-thickness: 2px;
  }

  /* Below this, two columns squeeze into unreadable slivers - stack instead,
     older line directly above its newer counterpart. The empty half of an
     added/removed row (nothing happened on that side) is dropped rather than
     left as a blank line, since there's no second column left to align with.
     Each cell keeps its own gutter+text grid, so the stacked headers above
     keep making sense: the −/+ tint and name/date/author sit right above
     their own side's lines instead of one column collapsing into the other. */
  @media (max-width: 760px) {
    .text-columns {
      grid-template-columns: 1fr;
    }

    .headers {
      flex-direction: column;
    }

    .text-cell.empty {
      display: none;
    }
  }
</style>
