<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import * as api from "$lib/api";
  import type { FileWithLastChanged, Loadable, Revision, Vault } from "$lib/types";
  import FoldersPane from "$lib/FoldersPane.svelte";
  import FilesPane from "$lib/FilesPane.svelte";
  import TimelinePane from "$lib/TimelinePane.svelte";

  function errorMessage(e: unknown): string {
    return e instanceof Error ? e.message : String(e);
  }

  let vaults = $state<Loadable<Vault[]>>({ status: "loading" });
  let selectedVaultId = $state<number | null>(null);
  let addingVault = $state(false);
  let addVaultError = $state<string | null>(null);

  // `null` means "no folder picked yet" - distinct from still loading.
  let files = $state<Loadable<FileWithLastChanged[]> | null>(null);
  let selectedFileId = $state<number | null>(null);

  // `null` means "no file picked yet".
  let revisions = $state<Loadable<Revision[]> | null>(null);

  let justArrivedId = $state<number | null>(null);

  let selectedVault = $derived(
    vaults.status === "loaded" ? (vaults.data.find((v) => v.id === selectedVaultId) ?? null) : null,
  );
  let selectedFile = $derived(
    files && files.status === "loaded"
      ? (files.data.find((f) => f.id === selectedFileId) ?? null)
      : null,
  );

  async function loadVaults() {
    vaults = { status: "loading" };
    try {
      vaults = { status: "loaded", data: await api.listVaults() };
    } catch (e) {
      vaults = { status: "error", message: errorMessage(e) };
    }
  }

  // Tracked files carry no timestamp of their own, so "last changed" comes
  // from the newest entry in each file's timeline (already sorted newest-first).
  async function loadFilesEnriched(vaultId: number): Promise<FileWithLastChanged[]> {
    const tracked = await api.listTrackedFiles(vaultId);
    return Promise.all(
      tracked.map(async (file) => {
        const timeline = await api.getTimeline(file.id);
        return { ...file, lastChanged: timeline[0]?.timestamp ?? null };
      }),
    );
  }

  async function selectVault(id: number) {
    selectedVaultId = id;
    selectedFileId = null;
    revisions = null;
    files = { status: "loading" };
    try {
      files = { status: "loaded", data: await loadFilesEnriched(id) };
    } catch (e) {
      files = { status: "error", message: errorMessage(e) };
    }
  }

  async function refreshFiles() {
    if (selectedVaultId === null) return;
    try {
      files = { status: "loaded", data: await loadFilesEnriched(selectedVaultId) };
    } catch (e) {
      files = { status: "error", message: errorMessage(e) };
    }
  }

  async function selectFile(id: number) {
    selectedFileId = id;
    revisions = { status: "loading" };
    try {
      revisions = { status: "loaded", data: await api.getTimeline(id) };
    } catch (e) {
      revisions = { status: "error", message: errorMessage(e) };
    }
  }

  async function refreshTimeline() {
    if (selectedFileId === null) return;
    try {
      revisions = { status: "loaded", data: await api.getTimeline(selectedFileId) };
    } catch (e) {
      revisions = { status: "error", message: errorMessage(e) };
    }
  }

  async function handleAddFolder() {
    addVaultError = null;
    let picked: string | string[] | null;
    try {
      picked = await open({ directory: true, multiple: false });
    } catch (e) {
      addVaultError = errorMessage(e);
      return;
    }
    if (!picked || Array.isArray(picked)) return;

    addingVault = true;
    try {
      const vault = await api.addVault(picked);
      await loadVaults();
      await selectVault(vault.id);
    } catch (e) {
      addVaultError = errorMessage(e);
    } finally {
      addingVault = false;
    }
  }

  async function handleSetMilestone(revisionId: number, label: string) {
    await api.setMilestone(revisionId, label);
    await refreshTimeline();
  }

  async function handleSaveCopy(revisionId: number): Promise<string> {
    return api.saveCopy(revisionId);
  }

  loadVaults();

  $effect(() => {
    let unlisten: UnlistenFn | undefined;
    let cancelled = false;

    listen<{ fileId: number; revisionId: number }>("revision-created", (event) => {
      const { fileId, revisionId } = event.payload;
      refreshFiles();
      if (fileId === selectedFileId) {
        refreshTimeline();
        justArrivedId = revisionId;
        setTimeout(() => {
          if (justArrivedId === revisionId) justArrivedId = null;
        }, 4000);
      }
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });

    return () => {
      cancelled = true;
      unlisten?.();
    };
  });
</script>

<main>
  <header class="app-header">
    <h1>Tracer</h1>
    <p class="tagline">Every save, kept. Nothing to configure.</p>
  </header>

  <div class="panes">
    <FoldersPane
      {vaults}
      {selectedVaultId}
      {addingVault}
      {addVaultError}
      onSelect={selectVault}
      onAddFolder={handleAddFolder}
      onRetry={loadVaults}
    />
    <FilesPane {selectedVault} {files} {selectedFileId} onSelect={selectFile} />
    <TimelinePane
      {selectedFile}
      {revisions}
      {justArrivedId}
      onSetMilestone={handleSetMilestone}
      onSaveCopy={handleSaveCopy}
    />
  </div>

  <footer class="app-footer">
    Tracer keeps versions of your files, not a backup of your folder. If the
    folder itself is lost, its history goes with it.
  </footer>
</main>

<style>
  :global(:root) {
    --bg: #f6f6f6;
    --surface: #ffffff;
    --surface-hover: #eef0f3;
    --border: #e0e0e0;
    --text: #0f0f0f;
    --muted: #6b6b6b;
    --accent: #396cd8;
    --accent-soft: #e8f0ff;
    --on-accent: #ffffff;
    --error: #c0392b;
    --flash: #fff6cf;

    font-family:
      -apple-system, BlinkMacSystemFont, "Segoe UI", Inter, Avenir, Helvetica, Arial, sans-serif;
    font-size: 15px;
    line-height: 1.4;
    color-scheme: light;
  }

  @media (prefers-color-scheme: dark) {
    :global(:root) {
      --bg: #1e1e1e;
      --surface: #2a2a2a;
      --surface-hover: #343434;
      --border: #3c3c3c;
      --text: #f0f0f0;
      --muted: #9a9a9a;
      --accent: #6ea2ff;
      --accent-soft: #223252;
      --on-accent: #0f0f0f;
      --error: #ff6b5e;
      --flash: #3a3420;
      color-scheme: dark;
    }
  }

  :global(body) {
    margin: 0;
    background: var(--bg);
    color: var(--text);
  }

  :global(button) {
    font: inherit;
    color: inherit;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 0.45rem 0.8rem;
    cursor: pointer;
  }

  :global(button:hover:not(:disabled)) {
    background: var(--surface-hover);
  }

  :global(button:disabled) {
    opacity: 0.6;
    cursor: default;
  }

  :global(input) {
    font: inherit;
    color: inherit;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 0.4rem 0.6rem;
  }

  main {
    display: flex;
    flex-direction: column;
    height: 100vh;
  }

  .app-header {
    padding: 0.9rem 1.2rem 0.6rem;
    border-bottom: 1px solid var(--border);
  }

  .app-header h1 {
    margin: 0;
    font-size: 1.2rem;
  }

  .tagline {
    margin: 0.15rem 0 0;
    color: var(--muted);
    font-size: 0.85rem;
  }

  .panes {
    flex: 1;
    display: grid;
    grid-template-columns: minmax(180px, 1fr) minmax(220px, 1.2fr) minmax(280px, 1.8fr);
    min-height: 0;
  }

  .panes > :global(section) {
    border-right: 1px solid var(--border);
    background: var(--surface);
  }

  .panes > :global(section:last-child) {
    border-right: none;
  }

  .app-footer {
    padding: 0.5rem 1.2rem;
    border-top: 1px solid var(--border);
    font-size: 0.75rem;
    color: var(--muted);
    text-align: center;
  }
</style>
