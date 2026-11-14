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

  {#if !selectedFile || !revisions}
    <p class="hint">Pick a file in the middle column to see its versions.</p>
  {:else if revisions.status === "loading"}
    <p class="hint">Loading versions…</p>
  {:else if revisions.status === "error"}
    <p class="banner error">{revisions.message}</p>
  {:else if revisions.data.length === 0}
    <p class="hint">No versions yet. Save this file to start its history.</p>
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
              <span class="session-summary">
                {formatSessionSpan(
                  group.revisions[group.revisions.length - 1].timestamp,
                  group.revisions[0].timestamp,
                )}
                · {group.revisions.length} versions
              </span>
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
</section>

<style>
  .pane {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
    padding: 1rem;
    overflow-y: auto;
  }

  .pane-header h2 {
    margin: 0;
    font-size: 1rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .hint {
    color: var(--muted);
    font-size: 0.9rem;
    line-height: 1.4;
  }

  .rows {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }

  .banner.error {
    color: var(--error);
    font-size: 0.9rem;
  }

  .session {
    display: flex;
    flex-direction: column;
  }

  .session-toggle {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.6rem 0.85rem;
    border-radius: 8px;
    border: 1px solid var(--border);
    background: none;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  .chevron {
    display: inline-block;
    color: var(--muted);
    transition: transform 0.15s ease;
  }

  .chevron.open {
    transform: rotate(90deg);
  }

  .session-summary {
    font-size: 0.9rem;
    color: var(--muted);
  }

  .rows.nested {
    margin-top: 0.5rem;
    padding-left: 1.1rem;
    border-left: 1px solid var(--border);
  }
</style>
