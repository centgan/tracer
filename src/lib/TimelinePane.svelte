<script lang="ts">
  import type {
    CompareResult,
    FileStats,
    FileWithLastChanged,
    Loadable,
    Revision,
    TextComparison,
  } from "./types";
  import TimelineRow from "./TimelineRow.svelte";
  import { groupIntoSessions } from "./sessions";
  import { findGaps, sessionTickCount } from "./spine";
  import { formatFileStats, formatRelativeTime, formatSessionSpan } from "./format";
  import { summarizeChanges, supportsTextCompare } from "./partnames";
  import { collapseUnchanged } from "./textcompare";

  let {
    selectedFile,
    revisions,
    fileStats,
    justArrivedId,
    onSetMilestone,
    onSaveCopy,
    onCompare,
    onComparePartText,
  }: {
    selectedFile: FileWithLastChanged | null;
    revisions: Loadable<Revision[]> | null;
    fileStats: Loadable<FileStats> | null;
    justArrivedId: number | null;
    onSetMilestone: (revisionId: number, label: string) => Promise<void>;
    onSaveCopy: (revisionId: number) => Promise<string>;
    onCompare: (olderId: number, newerId: number) => Promise<CompareResult>;
    onComparePartText: (olderId: number, newerId: number, entryName: string) => Promise<TextComparison>;
  } = $props();

  let groups = $derived(
    revisions && revisions.status === "loaded" ? groupIntoSessions(revisions.data) : [],
  );

  // Keyed by "render the gap after this group index" so the template can
  // check it in the same pass it renders each group.
  let gapAfter = $derived(new Map(findGaps(groups).map((g) => [g.afterIndex, g.label])));

  // A group is open if the user toggled it explicitly (`override`), else it
  // falls back to its default: the newest group (so a live-arriving version
  // is visible), and whichever group holds the just-arrived version itself.
  let override = $state<Record<number, boolean>>({});

  let defaultOpen = $derived.by(() => {
    const ids = new Set<number>();
    if (groups.length > 0) ids.add(groups[0].id);
    if (justArrivedId !== null) {
      const owner = groups.find((g) => g.revisions.some((r) => r.id === justArrivedId));
      if (owner) ids.add(owner.id);
    }
    return ids;
  });

  function isOpen(group: { id: number }): boolean {
    return override[group.id] ?? defaultOpen.has(group.id);
  }

  function toggle(group: { id: number }) {
    override[group.id] = !isOpen(group);
  }

  // The selected version, single-select across the whole pane - same model
  // as the Folders and Files panes. Kept local since no other screen needs
  // to know which version is selected.
  let selectedId = $state<number | null>(null);
  let selectedRevision = $derived(
    revisions && revisions.status === "loaded"
      ? (revisions.data.find((r) => r.id === selectedId) ?? null)
      : null,
  );

  let naming = $state(false);
  let labelDraft = $state("");
  let namingBusy = $state(false);
  let namingError = $state<string | null>(null);

  let copyBusy = $state(false);
  let copyPath = $state<string | null>(null);
  let copyError = $state<string | null>(null);

  // The pane's compare flow: picking a second version, then showing the
  // result. "list" is the ordinary timeline every other piece of state above
  // already assumes.
  type PaneMode =
    | { kind: "list" }
    | { kind: "picking"; firstId: number }
    | { kind: "comparing"; olderId: number; newerId: number }
    | { kind: "viewingText"; olderId: number; newerId: number; entryName: string; label: string };

  let mode = $state<PaneMode>({ kind: "list" });
  let pickError = $state<string | null>(null);
  let compareResult = $state<Loadable<CompareResult> | null>(null);
  let textResult = $state<Loadable<TextComparison> | null>(null);

  function findRevision(id: number): Revision | undefined {
    return revisions && revisions.status === "loaded"
      ? revisions.data.find((r) => r.id === id)
      : undefined;
  }

  // What labels a version in the compare heading: its name when it has one,
  // same as the timeline row itself, else its time.
  function versionLabel(revision: Revision): string {
    return revision.milestone ?? formatRelativeTime(revision.timestamp);
  }

  // Switching files should drop the old selection - it's not even in the new
  // list. Tracked by id, not the `selectedFile` object itself, so a file-list
  // refresh (new objects, same id) doesn't blow away the user's selection.
  let lastFileId = $state<number | null>(null);
  $effect(() => {
    const id = selectedFile?.id ?? null;
    if (id !== lastFileId) {
      lastFileId = id;
      selectedId = null;
      naming = false;
      namingError = null;
      copyPath = null;
      copyError = null;
      mode = { kind: "list" };
      pickError = null;
      compareResult = null;
      textResult = null;
    }
  });

  // Escape cancels picking from anywhere in the pane, not just a focused
  // input - the picking state has no input to attach a keydown to.
  $effect(() => {
    if (mode.kind !== "picking") return;
    function onKeydown(e: KeyboardEvent) {
      if (e.key === "Escape") cancelPicking();
    }
    window.addEventListener("keydown", onKeydown);
    return () => window.removeEventListener("keydown", onKeydown);
  });

  function selectRevision(id: number) {
    selectedId = id;
    naming = false;
    namingError = null;
    copyPath = null;
    copyError = null;
  }

  // Routes a row click: normally it selects, but mid-pick it finishes (or
  // rejects) the comparison instead.
  function handleRowClick(id: number) {
    if (mode.kind !== "picking") {
      selectRevision(id);
      return;
    }
    if (id === mode.firstId) {
      pickError = "That's the same version. Pick a different one to compare with.";
      return;
    }
    startCompare(mode.firstId, id);
  }

  function startPicking() {
    if (!selectedRevision) return;
    mode = { kind: "picking", firstId: selectedRevision.id };
    pickError = null;
  }

  function cancelPicking() {
    mode = { kind: "list" };
    pickError = null;
  }

  function backToList() {
    mode = { kind: "list" };
    compareResult = null;
    textResult = null;
  }

  // Opens the side-by-side text view for one changed part. Only reachable
  // from "comparing", so the two version ids it needs are already in hand.
  async function openPartText(entryName: string, label: string) {
    if (mode.kind !== "comparing") return;
    const { olderId, newerId } = mode;
    mode = { kind: "viewingText", olderId, newerId, entryName, label };
    textResult = { status: "loading" };
    try {
      textResult = { status: "loaded", data: await onComparePartText(olderId, newerId, entryName) };
    } catch (e) {
      textResult = {
        status: "error",
        message: e instanceof Error ? e.message : "Couldn't load that comparison.",
      };
    }
  }

  // Back to the list of changed parts, not all the way to the timeline -
  // the compare result backing that list is still in `compareResult`.
  function backToChangedParts() {
    if (mode.kind !== "viewingText") return;
    mode = { kind: "comparing", olderId: mode.olderId, newerId: mode.newerId };
    textResult = null;
  }

  async function startCompare(aId: number, bId: number) {
    const a = findRevision(aId);
    const b = findRevision(bId);
    if (!a || !b) return;
    const [olderId, newerId] = a.timestamp <= b.timestamp ? [a.id, b.id] : [b.id, a.id];
    pickError = null;
    mode = { kind: "comparing", olderId, newerId };
    compareResult = { status: "loading" };
    try {
      compareResult = { status: "loaded", data: await onCompare(olderId, newerId) };
    } catch (e) {
      compareResult = {
        status: "error",
        message: e instanceof Error ? e.message : "Couldn't compare these versions.",
      };
    }
  }

  function startNaming() {
    if (!selectedRevision) return;
    labelDraft = selectedRevision.milestone ?? "";
    namingError = null;
    naming = true;
  }

  function cancelNaming() {
    naming = false;
    namingError = null;
  }

  // Svelte's a11y lint rightly rejects the `autofocus` attribute (it can
  // yank focus from under an unsuspecting user); focusing from an action
  // triggered by our own click handler doesn't have that problem.
  function focusOnMount(node: HTMLElement) {
    node.focus();
  }

  async function confirmNaming() {
    if (!selectedRevision) return;
    const label = labelDraft.trim();
    if (!label) {
      namingError = "Give this version a name first.";
      return;
    }
    namingBusy = true;
    namingError = null;
    try {
      await onSetMilestone(selectedRevision.id, label);
      naming = false;
    } catch (e) {
      namingError = e instanceof Error ? e.message : "Couldn't save that name.";
    } finally {
      namingBusy = false;
    }
  }

  async function handleSaveCopy() {
    if (!selectedRevision) return;
    copyBusy = true;
    copyError = null;
    copyPath = null;
    try {
      copyPath = await onSaveCopy(selectedRevision.id);
    } catch (e) {
      copyError = e instanceof Error ? e.message : "Couldn't save a copy.";
    } finally {
      copyBusy = false;
    }
  }
</script>

<section class="pane">
  <div class="pane-header">
    <h2>{selectedFile ? selectedFile.relativePath : "Timeline"}</h2>
    {#if mode.kind === "list" && fileStats?.status === "loaded" && revisions?.status === "loaded" && revisions.data.length > 0}
      <p class="stats-line">{formatFileStats(fileStats.data)}</p>
    {/if}
  </div>

  {#if mode.kind === "picking"}
    <p class="hint pad pick-hint">
      Pick another version to compare with.
      {#if pickError}<span class="error-text">{pickError}</span>{/if}
    </p>
  {/if}

  <div class="well">
    {#if mode.kind === "comparing"}
      {@const older = findRevision(mode.olderId)}
      {@const newer = findRevision(mode.newerId)}
      <div class="compare pad">
        <p class="compare-heading">
          Comparing {older ? versionLabel(older) : "one version"} to {newer
            ? versionLabel(newer)
            : "another version"}
        </p>
        {#if compareResult?.status === "loading"}
          <p class="hint">Comparing…</p>
        {:else if compareResult?.status === "error"}
          <p class="error-text">{compareResult.message}</p>
        {:else if compareResult?.status === "loaded"}
          {@const summary = summarizeChanges(compareResult.data.changes)}
          {@const nothingNamed =
            summary.changed.length === 0 && summary.added.length === 0 && summary.removed.length === 0}
          {#if nothingNamed}
            <p class="hint">
              Nothing meaningful changed between these versions - only internal bookkeeping
              differs.
            </p>
          {:else}
            {#if summary.changed.length > 0}
              <div class="change-group">
                <h3>Changed</h3>
                <ul>
                  {#each summary.changed as item}
                    <li>
                      {#if supportsTextCompare(item.entryName)}
                        <button class="plain part-link" onclick={() => openPartText(item.entryName, item.label)}>
                          {item.label}
                        </button>
                      {:else}
                        {item.label}
                      {/if}
                    </li>
                  {/each}
                </ul>
              </div>
            {/if}
            {#if summary.added.length > 0}
              <div class="change-group">
                <h3>Added</h3>
                <ul>
                  {#each summary.added as item}
                    <li>
                      {#if supportsTextCompare(item.entryName)}
                        <button class="plain part-link" onclick={() => openPartText(item.entryName, item.label)}>
                          {item.label}
                        </button>
                      {:else}
                        {item.label}
                      {/if}
                    </li>
                  {/each}
                </ul>
              </div>
            {/if}
            {#if summary.removed.length > 0}
              <div class="change-group">
                <h3>Removed</h3>
                <ul>
                  {#each summary.removed as item}
                    <li>
                      {#if supportsTextCompare(item.entryName)}
                        <button class="plain part-link" onclick={() => openPartText(item.entryName, item.label)}>
                          {item.label}
                        </button>
                      {:else}
                        {item.label}
                      {/if}
                    </li>
                  {/each}
                </ul>
              </div>
            {/if}
          {/if}
          <p class="hint compare-footer">
            {compareResult.data.unchangedCount} part{compareResult.data.unchangedCount === 1
              ? ""
              : "s"} unchanged
            {#if summary.internalCount > 0}
              · {summary.internalCount} internal change{summary.internalCount === 1 ? "" : "s"} hidden
            {/if}
          </p>
        {/if}
      </div>
    {:else if mode.kind === "viewingText"}
      {@const older = findRevision(mode.olderId)}
      {@const newer = findRevision(mode.newerId)}
      <div class="text-compare pad">
        <p class="compare-heading">{mode.label}</p>
        {#if textResult?.status === "loading"}
          <p class="hint">Loading…</p>
        {:else if textResult?.status === "error"}
          <p class="error-text">{textResult.message}</p>
        {:else if textResult?.status === "loaded"}
          {#if !textResult.data.hasText}
            <p class="hint">Text comparison isn't available for this part.</p>
          {:else}
            <div class="text-columns">
              <div class="column-header">{older ? versionLabel(older) : "Older version"}</div>
              <div class="column-header">{newer ? versionLabel(newer) : "Newer version"}</div>
              {#each collapseUnchanged(textResult.data.rows) as item}
                {#if item.kind === "collapsed"}
                  <div class="collapsed-marker">
                    {item.count} unchanged line{item.count === 1 ? "" : "s"} hidden
                  </div>
                {:else if item.row.status === "same"}
                  <div class="full-row same-row">{item.row.left}</div>
                {:else}
                  <div
                    class="text-cell left-cell"
                    class:removed={item.row.status === "removed"}
                    class:empty={item.row.left === null}
                  >
                    {#if item.row.left !== null}
                      {#if item.row.status === "removed"}
                        <span class="marker" aria-hidden="true">−&nbsp;</span>
                      {/if}
                      {#if item.row.leftSpans.length > 0}
                        {#each item.row.leftSpans as span}<span class:diff-word-removed={span.changed}
                            >{span.text}</span
                          >{/each}
                      {:else}
                        {item.row.left}
                      {/if}
                    {/if}
                  </div>
                  <div
                    class="text-cell right-cell"
                    class:added={item.row.status === "added"}
                    class:empty={item.row.right === null}
                  >
                    {#if item.row.right !== null}
                      {#if item.row.status === "added"}
                        <span class="marker" aria-hidden="true">+&nbsp;</span>
                      {/if}
                      {#if item.row.rightSpans.length > 0}
                        {#each item.row.rightSpans as span}<span class:diff-word-added={span.changed}
                            >{span.text}</span
                          >{/each}
                      {:else}
                        {item.row.right}
                      {/if}
                    {/if}
                  </div>
                {/if}
              {/each}
            </div>
          {/if}
        {/if}
      </div>
    {:else if !selectedFile || !revisions}
      <p class="hint pad">Pick a file in the middle column to see its versions.</p>
    {:else if revisions.status === "loading"}
      <p class="hint pad">Loading versions…</p>
    {:else if revisions.status === "error"}
      <p class="error-text pad">{revisions.message}</p>
    {:else if revisions.data.length === 0}
      <p class="hint pad">No versions yet. Save this file to start its history.</p>
    {:else}
      <ul class="rows">
        {#each groups as group, i (group.id)}
          {#if group.revisions.length === 1}
            <TimelineRow
              revision={group.revisions[0]}
              selected={group.revisions[0].id === selectedId}
              justArrived={group.revisions[0].id === justArrivedId}
              onSelect={handleRowClick}
            />
          {:else}
            <li class="session">
              <button
                class="session-toggle"
                aria-expanded={isOpen(group)}
                onclick={() => toggle(group)}
              >
                <span class="spine" aria-hidden="true">
                  <span class="spine-line"></span>
                  <span class="spine-ticks">
                    {#each Array(sessionTickCount(group.revisions.length)) as _}
                      <span class="tick"></span>
                    {/each}
                  </span>
                </span>
                <!-- Same column order as a version row: when it happened, then
                     what it was. The time column always holds a time. -->
                <span class="session-span"
                  >{formatSessionSpan(
                    group.revisions[group.revisions.length - 1].timestamp,
                    group.revisions[0].timestamp,
                  )}</span
                >
                <span class="session-label">
                  <span class="chevron" class:open={isOpen(group)} aria-hidden="true">▸</span>
                  {group.revisions.length} saves
                </span>
              </button>
              {#if isOpen(group)}
                <!-- Unindented: these rows share the spine's x position with
                     every other row, so the vertical line stays unbroken
                     instead of jumping sideways. -->
                <ul class="rows">
                  {#each group.revisions as revision (revision.id)}
                    <TimelineRow
                      {revision}
                      selected={revision.id === selectedId}
                      justArrived={revision.id === justArrivedId}
                      onSelect={handleRowClick}
                    />
                  {/each}
                </ul>
              {/if}
            </li>
          {/if}
          {#if gapAfter.has(i)}
            <li class="gap-row">
              <span class="spine" aria-hidden="true">
                <span class="spine-line dashed"></span>
              </span>
              <span class="gap-label">{gapAfter.get(i)}</span>
            </li>
          {/if}
        {/each}
      </ul>
    {/if}
  </div>

  <div class="actions">
    {#if mode.kind === "viewingText"}
      <button onclick={backToChangedParts}>Back to changed parts</button>
    {:else if mode.kind === "comparing"}
      <button onclick={backToList}>Back to versions</button>
    {:else if mode.kind === "picking"}
      <button class="plain" onclick={cancelPicking}>Cancel</button>
    {:else if naming}
      <input
        use:focusOnMount
        bind:value={labelDraft}
        placeholder="e.g. sent to supervisor"
        disabled={namingBusy}
        onkeydown={(e) => {
          if (e.key === "Enter") confirmNaming();
          if (e.key === "Escape") cancelNaming();
        }}
      />
      <button onclick={confirmNaming} disabled={namingBusy}>Save</button>
      <button class="plain" onclick={cancelNaming} disabled={namingBusy}>Cancel</button>
    {:else}
      <button onclick={startNaming} disabled={!selectedRevision}>Name this version</button>
      <button onclick={handleSaveCopy} disabled={!selectedRevision || copyBusy}>
        {copyBusy ? "Saving…" : "Save a copy"}
      </button>
      <button onclick={startPicking} disabled={!selectedRevision}>Compare…</button>
    {/if}
  </div>
  {#if namingError}<p class="error-text pad">{namingError}</p>{/if}
  {#if copyPath}
    <p class="hint pad">Saved a read-only copy to {copyPath}. Your live file wasn't touched.</p>
  {/if}
  {#if copyError}<p class="error-text pad">{copyError}</p>{/if}
</section>

<style>
  .pane {
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-height: 0;
    height: 100%;
  }

  .pane-header h2 {
    font-size: 11px;
    font-weight: bold;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* A quiet fact, not a banner: same size and weight as the hint text
     elsewhere, no colour, no emphasis. */
  .stats-line {
    font-size: 11px;
    opacity: 0.72;
  }

  .pad {
    padding: 8px;
  }

  .pick-hint {
    flex: none;
    font-weight: bold;
  }

  .pick-hint .error-text {
    display: block;
    font-weight: normal;
    margin-top: 2px;
  }

  .compare-heading {
    font-size: 13px;
    font-weight: bold;
    margin-bottom: 10px;
  }

  .change-group {
    margin-bottom: 10px;
  }

  .change-group h3 {
    font-size: 11px;
    font-weight: bold;
    margin-bottom: 2px;
  }

  .change-group ul {
    margin: 0;
    padding-left: 1.4em;
  }

  .change-group li {
    font-size: 13px;
    line-height: 1.5;
  }

  .compare-footer {
    margin-top: 6px;
  }

  /* A changed/added/removed part that has text to show - styled as a link so
     it reads as clickable without a full button bevel in a list of names. */
  .part-link {
    font-size: 13px;
    text-decoration: underline;
    color: var(--ink);
  }

  .text-compare {
    display: flex;
    flex-direction: column;
    min-height: 0;
  }

  .text-columns {
    display: grid;
    grid-template-columns: 1fr 1fr;
    column-gap: 10px;
    align-items: start;
  }

  .column-header {
    position: sticky;
    top: 0;
    background: var(--well);
    font-size: 11px;
    font-weight: bold;
    padding: 2px 10px 4px;
    border-bottom: 1px solid var(--edge);
  }

  /* Spans both columns: an unchanged line (shown once, not duplicated per
     side) and the "N lines hidden" marker both read better full-width. */
  .full-row {
    grid-column: 1 / -1;
  }

  .same-row {
    padding: 1px 10px;
    font-size: 13px;
    line-height: 1.5;
    opacity: 0.68;
  }

  .collapsed-marker {
    padding: 3px 0;
    text-align: center;
    font-size: 11px;
    font-style: italic;
    opacity: 0.6;
  }

  .text-cell {
    padding: 1px 10px;
    font-size: 13px;
    line-height: 1.5;
    min-height: 1.5em;
    white-space: pre-wrap;
    overflow-wrap: break-word;
  }

  .text-cell.removed {
    background: var(--diff-remove-bg);
  }

  .text-cell.added {
    background: var(--diff-add-bg);
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
     left as a blank line, since there's no second column left to align with. */
  @media (max-width: 760px) {
    .text-columns {
      grid-template-columns: 1fr;
    }

    .text-cell.empty {
      display: none;
    }
  }

  .well {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    /* Four metadata columns plus a label have a real minimum width. Below
       it, scroll horizontally instead of mangling every column down to
       unreadable slivers. */
    overflow-x: auto;
  }

  .rows {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  /* Hairlines between rows so 200 versions still read as discrete items
     instead of running together. Mixed from --ink so the contrast direction
     is always right: dark lines on the light well, light lines on the dark
     one, with neither reading as heavy as a real border. */
  .rows > li:not(:last-child) {
    border-bottom: 1px solid color-mix(in srgb, var(--ink) 12%, transparent);
  }

  .session {
    display: flex;
    flex-direction: column;
  }

  .session-toggle {
    width: 100%;
    display: grid;
    grid-template-columns: var(--timeline-grid);
    align-items: center;
    column-gap: 12px;
    min-height: 24px;
    padding: 3px 8px;
    background: none;
    border: none;
    box-shadow: none;
    border-radius: 0;
    color: var(--ink);
    font: inherit;
    font-size: 11px;
    font-weight: bold;
    text-align: left;
  }

  .session-toggle:hover {
    background: var(--face-hover);
  }

  .session-label {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .chevron {
    display: inline-block;
    flex: none;
  }

  @media (prefers-reduced-motion: no-preference) {
    .chevron {
      transition: transform 0.12s ease;
    }
  }

  .chevron.open {
    transform: rotate(90deg);
  }

  .session-span {
    grid-column: 2;
    font-size: 11px;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--data);
    font-weight: normal;
    opacity: 0.75;
  }

  .gap-row {
    display: grid;
    grid-template-columns: var(--timeline-grid);
    align-items: center;
    column-gap: 12px;
    min-height: 28px;
    padding: 3px 8px;
  }

  .gap-label {
    grid-column: 2 / -1;
    font-size: 11px;
    font-style: italic;
    opacity: 0.6;
  }

  .actions {
    flex: none;
    display: flex;
    align-items: center;
    gap: 8px;
    padding-top: 2px;
    border-top: 1px solid var(--edge);
  }

  .actions input {
    flex: 1;
    min-width: 0;
  }
</style>
