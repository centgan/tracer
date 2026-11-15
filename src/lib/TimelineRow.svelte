<script lang="ts">
  import type { Revision } from "./types";
  import { formatBytes, formatRelativeTime } from "./format";

  let {
    revision,
    justArrived,
    onSetMilestone,
    onSaveCopy,
  }: {
    revision: Revision;
    justArrived: boolean;
    onSetMilestone: (revisionId: number, label: string) => Promise<void>;
    onSaveCopy: (revisionId: number) => Promise<string>;
  } = $props();

  let naming = $state(false);
  // Populated by startNaming right before the input shows, not here - an
  // initializer that reads a prop only captures its value once, which would
  // go stale if the milestone changes later without the user reopening this.
  let labelDraft = $state("");
  let namingBusy = $state(false);
  let namingError = $state<string | null>(null);

  let copyBusy = $state(false);
  let copyPath = $state<string | null>(null);
  let copyError = $state<string | null>(null);

  function startNaming() {
    labelDraft = revision.milestone ?? "";
    namingError = null;
    naming = true;
  }

  async function confirmNaming() {
    const label = labelDraft.trim();
    if (!label) {
      namingError = "Give this version a name first.";
      return;
    }
    namingBusy = true;
    namingError = null;
    try {
      await onSetMilestone(revision.id, label);
      naming = false;
    } catch (e) {
      namingError = e instanceof Error ? e.message : "Couldn't save that name.";
    } finally {
      namingBusy = false;
    }
  }

  async function handleSaveCopy() {
    copyBusy = true;
    copyError = null;
    copyPath = null;
    try {
      copyPath = await onSaveCopy(revision.id);
    } catch (e) {
      copyError = e instanceof Error ? e.message : "Couldn't save a copy.";
    } finally {
      copyBusy = false;
    }
  }
</script>

<li class="row" class:flash={justArrived}>
  <div class="row-top">
    <div class="main">
      <div class="fields">
        <span class="time">{formatRelativeTime(revision.timestamp)}</span>
        <span class="size">{formatBytes(revision.byteSize)}</span>
        <span class="author" title={revision.authorEmail ?? undefined}>
          {revision.authorName ?? ""}
        </span>
        {#if justArrived}<span class="new">new</span>{/if}
      </div>

      <!-- Name sits below its own timestamp, same slot as "Name this version".
           Above it, a star read as a heading for the row underneath and you
           couldn't tell which version was the named one. -->
      {#if revision.milestone && !naming}
        <button class="plain milestone" onclick={startNaming} title="Rename this version">
          <span aria-hidden="true">★</span>
          {revision.milestone}
        </button>
      {/if}

      {#if !revision.milestone && !naming}
        <button class="plain name-link" onclick={startNaming}>Name this version</button>
      {/if}

      {#if naming}
        <div class="naming">
          <input
            bind:value={labelDraft}
            placeholder="e.g. sent to supervisor"
            disabled={namingBusy}
            onkeydown={(e) => e.key === "Enter" && confirmNaming()}
          />
          <button onclick={confirmNaming} disabled={namingBusy}>Save</button>
          <button class="plain" onclick={() => (naming = false)} disabled={namingBusy}>
            Cancel
          </button>
        </div>
        {#if namingError}<p class="error-text">{namingError}</p>{/if}
      {/if}
    </div>

    <button class="plain copy-btn" onclick={handleSaveCopy} disabled={copyBusy}>
      {copyBusy ? "Saving…" : "Save a copy"}
    </button>
  </div>

  {#if copyPath}
    <p class="hint copy-note">Saved a read-only copy to {copyPath}. Your live file wasn't touched.</p>
  {/if}
  {#if copyError}<p class="error-text copy-note">{copyError}</p>{/if}
</li>

<style>
  .row {
    padding: 4px 8px;
  }

  @media (prefers-reduced-motion: no-preference) {
    .row {
      transition: background-color 1.2s ease;
    }
  }

  .row.flash {
    background: var(--flash);
  }

  .row-top {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 12px;
  }

  .main {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .milestone {
    align-self: flex-start;
    font-size: 13px;
    font-weight: bold;
  }

  .fields {
    display: flex;
    align-items: baseline;
    gap: 12px;
    font-family: var(--data);
    font-size: 11px;
  }

  .time {
    flex: none;
    width: 10.5em;
    white-space: nowrap;
  }

  .size {
    flex: none;
    width: 4.5em;
    text-align: right;
  }

  .author {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--ui);
    opacity: 0.75;
  }

  .new {
    flex: none;
    color: var(--live);
    font-weight: bold;
  }

  .name-link {
    align-self: flex-start;
    font-size: 11px;
    opacity: 0.72;
  }

  .naming {
    display: flex;
    gap: 6px;
    align-items: center;
  }

  .naming input {
    flex: 1;
    min-width: 0;
  }

  .copy-btn {
    flex: none;
    font-size: 11px;
    opacity: 0;
  }

  @media (prefers-reduced-motion: no-preference) {
    .copy-btn {
      transition: opacity 0.1s ease;
    }
  }

  .row:hover .copy-btn,
  .row:focus-within .copy-btn,
  .copy-btn:focus-visible {
    opacity: 1;
  }

  .copy-note {
    text-align: right;
  }
</style>
