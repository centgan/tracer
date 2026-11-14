<script lang="ts">
  import type { FileWithLastChanged, Loadable, Vault } from "./types";
  import { formatRelativeTime } from "./format";

  let {
    selectedVault,
    files,
    selectedFileId,
    onSelect,
  }: {
    selectedVault: Vault | null;
    files: Loadable<FileWithLastChanged[]> | null;
    selectedFileId: number | null;
    onSelect: (id: number) => void;
  } = $props();
</script>

<section class="pane">
  <div class="pane-header">
    <h2>Files</h2>
  </div>

  {#if !selectedVault || !files}
    <p class="hint">Pick a folder on the left to see what's tracked in it.</p>
  {:else if files.status === "loading"}
    <p class="hint">Looking for tracked files…</p>
  {:else if files.status === "error"}
    <p class="banner error">{files.message}</p>
  {:else if files.data.length === 0}
    <p class="hint">
      Files appear here on their own once they're saved. Save a .pptx,
      .docx, or .xlsx in this folder to see it tracked.
    </p>
  {:else}
    <ul class="list">
      {#each files.data as file (file.id)}
        <li>
          <button
            class="list-item"
            class:selected={file.id === selectedFileId}
            onclick={() => onSelect(file.id)}
          >
            <span class="path">{file.relativePath}</span>
            <span class="when">
              {file.lastChanged === null ? "no versions yet" : formatRelativeTime(file.lastChanged)}
            </span>
          </button>
        </li>
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
  }

  .hint {
    color: var(--muted);
    font-size: 0.9rem;
    line-height: 1.4;
  }

  .list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }

  .list-item {
    width: 100%;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 0.15rem;
    text-align: left;
    background: none;
    border: 1px solid transparent;
    border-radius: 6px;
    padding: 0.5rem 0.6rem;
    font: inherit;
    color: inherit;
    cursor: pointer;
  }

  .list-item:hover {
    background: var(--surface-hover);
  }

  .list-item.selected {
    background: var(--accent-soft);
    border-color: var(--accent);
  }

  .path {
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 100%;
  }

  .when {
    font-size: 0.8rem;
    color: var(--muted);
  }

  .banner.error {
    color: var(--error);
    font-size: 0.9rem;
  }
</style>
