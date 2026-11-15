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

  function watchTitle(vault: Vault): string {
    return vault.hasTracerFolder
      ? vault.rootPath
      : `${vault.rootPath} — its .tracer folder was removed, so Tracer stopped watching it.`;
  }
</script>

<section class="pane">
  <div class="pane-header">
    <h2>Folders</h2>
    <button onclick={onAddFolder} disabled={addingVault}>
      {addingVault ? "Adding…" : "Add a folder"}
    </button>
  </div>

  {#if addVaultError}
    <p class="error-text">{addVaultError}</p>
  {/if}

  <p class="hint">Delete a folder's .tracer folder to stop watching it.</p>

  <div class="well">
    {#if vaults.status === "loading"}
      <p class="hint pad">Loading your folders…</p>
    {:else if vaults.status === "error"}
      <p class="error-text pad">{vaults.message}</p>
      <button onclick={onRetry}>Try again</button>
    {:else if vaults.data.length === 0}
      <p class="hint pad">
        Pick a folder. Every saved file in it gets a version you can go back to.
      </p>
    {:else}
      <ul class="list">
        {#each vaults.data as vault (vault.id)}
          <li>
            <button
              class="row"
              class:selected={vault.id === selectedVaultId}
              onclick={() => onSelect(vault.id)}
              title={watchTitle(vault)}
            >
              <span class="name">{folderName(vault.rootPath)}</span>
              <span class="state" class:unwatched={!vault.hasTracerFolder}>
                <span class="dot" aria-hidden="true">●</span>
                {vault.hasTracerFolder ? "Watching" : "Not watched"}
              </span>
            </button>
          </li>
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

  .pane-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }

  .pane-header h2 {
    font-size: 11px;
    font-weight: bold;
  }

  .pad {
    padding: 8px;
  }

  .well {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
  }

  .list {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  .row {
    width: 100%;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    text-align: left;
    background: none;
    border: none;
    box-shadow: none;
    border-radius: 0;
    min-height: 24px;
    padding: 3px 8px;
    font: inherit;
    color: var(--ink);
  }

  .row:hover:not(.selected) {
    background: var(--face-hover);
  }

  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }

  .state {
    flex: none;
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: 11px;
    color: var(--live);
  }

  .state .dot {
    font-size: 8px;
  }

  .state.unwatched {
    color: var(--warn);
  }

  .row.selected {
    background: var(--select);
    color: #fff;
  }

  .row.selected .state,
  .row.selected .state.unwatched {
    color: #fff;
  }
</style>
