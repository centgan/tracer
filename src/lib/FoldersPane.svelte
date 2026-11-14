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
            {folderName(vault.rootPath)}
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
    text-align: left;
    background: none;
    border: 1px solid transparent;
    border-radius: 6px;
    padding: 0.5rem 0.6rem;
    font: inherit;
    color: inherit;
    cursor: pointer;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
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
