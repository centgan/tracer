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

<li class="row" class:pinned={!!revision.milestone} class:flash={justArrived}>
  <div class="main">
    <div class="when">
      {formatRelativeTime(revision.timestamp)}
      <span class="size">· {formatBytes(revision.byteSize)}</span>
      {#if revision.authorName}
        <span class="author" title={revision.authorEmail ?? undefined}
          >· {revision.authorName}</span
        >
      {/if}
      {#if justArrived}<span class="badge">new</span>{/if}
    </div>

    {#if revision.milestone && !naming}
      <button class="milestone" onclick={startNaming} title="Rename this version">
        ★ {revision.milestone}
      </button>
    {:else if !naming}
      <button class="link" onclick={startNaming}>Name this version</button>
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
        <button
          class="ghost"
          onclick={() => (naming = false)}
          disabled={namingBusy}
        >
          Cancel
        </button>
      </div>
      {#if namingError}<p class="banner error">{namingError}</p>{/if}
    {/if}
  </div>

  <div class="actions">
    <button onclick={handleSaveCopy} disabled={copyBusy}>
      {copyBusy ? "Saving…" : "Save a copy"}
    </button>
    {#if copyPath}
      <p class="hint">
        Saved a read-only copy to {copyPath}. Your live file wasn't touched.
      </p>
    {/if}
    {#if copyError}<p class="banner error">{copyError}</p>{/if}
  </div>
</li>

<style>
  .row {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    gap: 1rem;
    padding: 0.75rem 0.85rem;
    border-radius: 8px;
    border: 1px solid var(--border);
    transition: background-color 1.5s ease;
  }

  .row.pinned {
    background: var(--accent-soft);
    border-color: var(--accent);
  }

  .row.flash {
    background: var(--flash);
  }

  .main {
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
    min-width: 0;
  }

  .when {
    font-size: 0.9rem;
  }

  .size,
  .author {
    color: var(--muted);
  }

  .badge {
    margin-left: 0.4rem;
    font-size: 0.7rem;
    text-transform: uppercase;
    letter-spacing: 0.03em;
    background: var(--accent);
    color: var(--on-accent);
    border-radius: 999px;
    padding: 0.1rem 0.5rem;
  }

  .milestone {
    align-self: flex-start;
    background: none;
    border: none;
    padding: 0;
    font: inherit;
    font-weight: 600;
    color: var(--accent);
    cursor: pointer;
    text-align: left;
  }

  .link {
    align-self: flex-start;
    background: none;
    border: none;
    padding: 0;
    font: inherit;
    font-size: 0.85rem;
    color: var(--muted);
    text-decoration: underline;
    cursor: pointer;
  }

  .naming {
    display: flex;
    gap: 0.4rem;
    align-items: center;
  }

  .naming input {
    flex: 1;
    min-width: 0;
  }

  .actions {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 0.3rem;
    max-width: 18rem;
  }

  .hint,
  .banner.error {
    font-size: 0.8rem;
    text-align: right;
    overflow-wrap: anywhere;
  }

  .hint {
    color: var(--muted);
  }

  .banner.error {
    color: var(--error);
  }

  button.ghost {
    background: none;
  }
</style>
