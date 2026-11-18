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
    onToggleActive,
  }: {
    vaults: Loadable<Vault[]>;
    selectedVaultId: number | null;
    addingVault: boolean;
    addVaultError: string | null;
    onSelect: (id: number) => void;
    onAddFolder: () => void;
    onRetry: () => void;
    onToggleActive: (vault: Vault, active: boolean) => Promise<void>;
  } = $props();

  let selectedVault = $derived(
    vaults.status === "loaded" ? (vaults.data.find((v) => v.id === selectedVaultId) ?? null) : null,
  );

  let toggleBusy = $state(false);
  let toggleError = $state<string | null>(null);
  let toggleMessage = $state<string | null>(null);

  async function handleToggleActive() {
    if (!selectedVault) return;
    const vault = selectedVault;
    toggleBusy = true;
    toggleError = null;
    toggleMessage = null;
    try {
      const nextActive = !vault.isActive;
      await onToggleActive(vault, nextActive);
      toggleMessage = nextActive
        ? "Resumed watching. New saves will be tracked again."
        : "Stopped watching. Past versions are kept; new saves won't be recorded until you resume.";
    } catch (e) {
      toggleError = e instanceof Error ? e.message : "Couldn't change that.";
    } finally {
      toggleBusy = false;
    }
  }

  // The root path is useful as a tooltip, but the last segment is what a
  // person actually recognizes.
  function folderName(rootPath: string): string {
    return rootPath.split(/[/\\]/).filter(Boolean).pop() ?? rootPath;
  }

  function watchTitle(vault: Vault): string {
    if (!vault.isActive) {
      return `${vault.rootPath} — not watching. Select it and choose "Resume watching" to start again.`;
    }
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
              <span class="folder-info">
                <svg class="folder-glyph" viewBox="0 0 16 12" aria-hidden="true" focusable="false">
                  <path
                    d="M1 2h5l1.5 1.5H15v7a1 1 0 0 1-1 1H2a1 1 0 0 1-1-1V2z"
                    fill="var(--folder)"
                    stroke="var(--edge)"
                    stroke-width="1"
                  />
                </svg>
                <span class="name">{folderName(vault.rootPath)}</span>
              </span>
              <span class="state" class:unwatched={!vault.hasTracerFolder || !vault.isActive}>
                <span class="dot" aria-hidden="true">●</span>
                {!(vault.hasTracerFolder && vault.isActive) ? "Not watched" : "Watching"}
              </span>
            </button>
          </li>
        {/each}
      </ul>
    {/if}
  </div>

  <div class="actions">
    <button onclick={handleToggleActive} disabled={!selectedVault || toggleBusy}>
      {toggleBusy ? "Working…" : selectedVault?.isActive === false ? "Resume watching" : "Stop watching"}
    </button>
  </div>
  {#if toggleError}<p class="error-text pad">{toggleError}</p>{/if}
  {#if toggleMessage}<p class="hint pad">{toggleMessage}</p>{/if}
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

  .folder-info {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }

  .folder-glyph {
    flex: none;
    width: 16px;
    height: 12px;
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

  .actions {
    flex: none;
    display: flex;
    align-items: center;
    gap: 8px;
    padding-top: 2px;
    border-top: 1px solid var(--edge);
  }
</style>
