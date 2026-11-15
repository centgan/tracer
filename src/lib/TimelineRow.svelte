<script lang="ts">
  import type { Revision } from "./types";
  import { formatRelativeTime } from "./format";

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
    <span class="spine" aria-hidden="true">
      <span class="spine-line"></span>
      <span class="spine-mark" class:named={!!revision.milestone}></span>
    </span>
    <!-- The time is always in the same column. It used to swap places with the
         name, so the first column meant "name" on some rows and "when" on
         others and you had to re-read every line to know which. -->
    <span class="time">{formatRelativeTime(revision.timestamp)}</span>
    <!-- A long name ellipsizes in a narrow window; the tooltip is the only way
         to read the rest of it. -->
    <span class="name" title={revision.milestone ?? undefined}>
      {#if revision.milestone}
        <span class="star" aria-hidden="true">★</span>{revision.milestone}
      {/if}
    </span>
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

  .name {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: bold;
  }

  .star {
    margin-right: 4px;
  }

  .time {
    font-family: var(--data);
    font-size: 11px;
  }

  .author {
    font-size: 11px;
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
