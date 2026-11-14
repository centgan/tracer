<script lang="ts">
  import type { FileWithLastChanged, Loadable, Revision } from "./types";
  import TimelineRow from "./TimelineRow.svelte";

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
      {#each revisions.data as revision (revision.id)}
        <TimelineRow
          {revision}
          justArrived={revision.id === justArrivedId}
          {onSetMilestone}
          {onSaveCopy}
        />
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
</style>
