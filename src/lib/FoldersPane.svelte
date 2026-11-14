<script lang="ts">
  import type { Loadable, Vault } from "./types";

  let {
    vaults,
    selectedVaultId,
    addingVault,
    addVaultError,
    onSelect,
    onAddFolder,
    onRetry,
  }: {
    vaults: Loadable<Vault[]>;
    selectedVaultId: number | null;
    addingVault: boolean;
    addVaultError: string | null;
    onSelect: (id: number) => void;
    onAddFolder: () => void;
    onRetry: () => void;
  } = $props();

  // The root path is useful as a tooltip, but the last segment is what a
  // person actually recognizes.
  function folderName(rootPath: string): string {
    return rootPath.split(/[/\\]/).filter(Boolean).pop() ?? rootPath;
  }
</script>

<section class="pane folders">
  <div class="pane-header">
    <h2>Folders</h2>
    <button onclick={onAddFolder} disabled={addingVault}>
      {addingVault ? "Adding…" : "Add a folder"}
    </button>
  </div>

  {#if addVaultError}
    <p class="banner error">{addVaultError}</p>
  {/if}

  <p class="hint tracer-explain">
    Tracer only watches folders containing a <code>.tracer</code> folder. Delete that folder to
    stop watching — your files are never touched.
  </p>

  {#if vaults.status === "loading"}
    <p class="hint">Loading your folders…</p>
  {:else if vaults.status === "error"}
    <p class="banner error">{vaults.message}</p>
    <button onclick={onRetry}>Try again</button>
  {:else if vaults.data.length === 0}
    <p class="hint">
      Pick a folder. Every time you save a file in it, Tracer keeps a version
      you can go back to.
    </p>
  {:else}
    <ul class="list">
      {#each vaults.data as vault (vault.id)}
        <li>
          <button
            class="list-item"
            class:selected={vault.id === selectedVaultId}
            onclick={() => onSelect(vault.id)}
            title={vault.rootPath}
          >
            <span class="folder-name">{folderName(vault.rootPath)}</span>
            <span class="watch-status" class:unwatched={!vault.hasTracerFolder}>
              <span class="dot" aria-hidden="true"></span>
              {vault.hasTracerFolder ? "Watching" : "Not watched"}
            </span>
            {#if !vault.hasTracerFolder}
              <span class="watch-explain">
                Its .tracer folder was removed, so Tracer stopped watching this folder.
              </span>
            {/if}
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

  .pane-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
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

  .tracer-explain {
    font-size: 0.78rem;
  }

  .tracer-explain code {
    font-family: inherit;
    background: var(--surface-hover);
    border-radius: 3px;
    padding: 0.05rem 0.3rem;
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
    gap: 0.2rem;
    text-align: left;
    background: none;
    border: 1px solid transparent;
    border-radius: 6px;
    padding: 0.5rem 0.6rem;
    font: inherit;
    color: inherit;
    cursor: pointer;
  }

  .folder-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 100%;
  }

  .watch-status {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    font-size: 0.78rem;
    color: var(--ok);
  }

  .watch-status .dot {
    width: 0.45rem;
    height: 0.45rem;
    border-radius: 50%;
    background: var(--ok);
  }

  .watch-status.unwatched {
    color: var(--warn);
  }

  .watch-status.unwatched .dot {
    background: var(--warn);
  }

  .watch-explain {
    font-size: 0.75rem;
    color: var(--warn);
    background: var(--warn-soft);
    border-radius: 4px;
    padding: 0.2rem 0.4rem;
    white-space: normal;
  }

  .list-item:hover {
    background: var(--surface-hover);
  }

  .list-item.selected {
    background: var(--accent-soft);
    border-color: var(--accent);
    font-weight: 600;
  }

  .banner.error {
    color: var(--error);
    font-size: 0.9rem;
  }
</style>
