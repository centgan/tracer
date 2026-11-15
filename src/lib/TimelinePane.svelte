<script lang="ts">
  import type { FileWithLastChanged, Loadable, Revision } from "./types";
  import TimelineRow from "./TimelineRow.svelte";
  import { groupIntoSessions } from "./sessions";
  import { formatSessionSpan } from "./format";

  let {
    selectedFile,
    revisions,
    justArrivedId,
    onSetMilestone,
    onSaveCopy,
  }: {
    selectedFile: FileWithLastChanged | null;
    revisions: Loadable<Revision[]> | null;
    justArrivedId: number | null;
    onSetMilestone: (revisionId: number, label: string) => Promise<void>;
    onSaveCopy: (revisionId: number) => Promise<string>;
  } = $props();

  let groups = $derived(
    revisions && revisions.status === "loaded" ? groupIntoSessions(revisions.data) : [],
  );

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
</script>

<section class="pane">
  <div class="pane-header">
    <h2>{selectedFile ? selectedFile.relativePath : "Timeline"}</h2>
  </div>

  <div class="well">
    {#if !selectedFile || !revisions}
      <p class="hint pad">Pick a file in the middle column to see its versions.</p>
    {:else if revisions.status === "loading"}
      <p class="hint pad">Loading versions…</p>
    {:else if revisions.status === "error"}
      <p class="error-text pad">{revisions.message}</p>
    {:else if revisions.data.length === 0}
      <p class="hint pad">No versions yet. Save this file to start its history.</p>
    {:else}
      <ul class="rows">
        {#each groups as group (group.id)}
          {#if group.revisions.length === 1}
            <TimelineRow
              revision={group.revisions[0]}
              justArrived={group.revisions[0].id === justArrivedId}
              {onSetMilestone}
              {onSaveCopy}
            />
          {:else}
            <li class="session">
              <button
                class="session-toggle"
                aria-expanded={isOpen(group)}
                onclick={() => toggle(group)}
              >
                <span class="chevron" class:open={isOpen(group)}>▸</span>
                <span class="session-span"
                  >{formatSessionSpan(
                    group.revisions[group.revisions.length - 1].timestamp,
                    group.revisions[0].timestamp,
                  )}</span
                >
                <span class="session-count">{group.revisions.length} saves</span>
              </button>
              {#if isOpen(group)}
                <ul class="rows nested">
                  {#each group.revisions as revision (revision.id)}
                    <TimelineRow
                      {revision}
                      justArrived={revision.id === justArrivedId}
                      {onSetMilestone}
                      {onSaveCopy}
                    />
                  {/each}
                </ul>
              {/if}
            </li>
          {/if}
        {/each}
      </ul>
    {/if}
  </div>
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

  .pad {
    padding: 8px;
  }

  .well {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
  }

  .rows {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  .session {
    display: flex;
    flex-direction: column;
  }

  .session-toggle {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 8px;
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
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .session-count {
    flex: none;
    opacity: 0.72;
    font-weight: normal;
  }

  .rows.nested {
    padding-left: 16px;
  }
</style>
