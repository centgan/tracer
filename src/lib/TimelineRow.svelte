<script lang="ts">
  import type { Revision } from "./types";
  import { formatBytes, formatRelativeTime } from "./format";

  let {
    revision,
    selected,
    justArrived,
    onSelect,
  }: {
    revision: Revision;
    selected: boolean;
    justArrived: boolean;
    onSelect: (revisionId: number) => void;
  } = $props();
</script>

<li>
  <button
    class="row"
    class:selected
    class:flash={justArrived}
    aria-pressed={selected}
    onclick={() => onSelect(revision.id)}
  >
    <!-- A long name ellipsizes in a narrow window; the tooltip is the only way
         to read the rest of it. -->
    <span
      class="label"
      class:named={!!revision.milestone}
      title={revision.milestone ?? undefined}
    >
      {#if revision.milestone}<span class="star" aria-hidden="true">★</span>{/if}
      {revision.milestone ?? formatRelativeTime(revision.timestamp)}
    </span>
    <span class="time">{revision.milestone ? formatRelativeTime(revision.timestamp) : ""}</span>
    <span class="size">{formatBytes(revision.byteSize)}</span>
    <span class="author" title={revision.authorEmail ?? undefined}>
      {revision.authorName ?? ""}
    </span>
    <span class="new">{justArrived ? "new" : ""}</span>
  </button>
</li>

<style>
  .row {
    width: 100%;
    display: grid;
    grid-template-columns: var(--timeline-grid);
    align-items: center;
    column-gap: 12px;
    min-height: 24px;
    padding: 3px 8px;
    text-align: left;
    background: none;
    border: none;
    box-shadow: none;
    border-radius: 0;
    font: inherit;
    color: var(--ink);
  }

  .row:hover:not(.selected) {
    background: var(--face-hover);
  }

  @media (prefers-reduced-motion: no-preference) {
    .row:not(.selected) {
      transition: background-color 1.2s ease;
    }
  }

  .row.flash:not(.selected) {
    background: var(--flash);
  }

  .label {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .label.named {
    font-weight: bold;
  }

  .time,
  .size,
  .author {
    font-family: var(--data);
    font-size: 11px;
  }

  .size {
    text-align: right;
    /* Collapses to a zero-width track in a narrow window (see theme.css);
       without this its text would spill into the next column. */
    overflow: hidden;
  }

  .author {
    font-family: var(--ui);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    opacity: 0.75;
  }

  .row.selected .author {
    opacity: 1;
  }

  .row.selected {
    background: var(--select);
    color: #fff;
  }

  .new {
    flex: none;
    font-size: 11px;
    font-weight: bold;
    color: var(--live);
  }

  .row.selected .new {
    color: #fff;
  }
</style>
