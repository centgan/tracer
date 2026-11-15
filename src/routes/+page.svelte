<script lang="ts">
  import "$lib/theme.css";
  import { open } from "@tauri-apps/plugin-dialog";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import * as api from "$lib/api";
  import type {
    CompareResult,
    FileStats,
    FileWithLastChanged,
    Loadable,
    Revision,
    TextComparison,
    Vault,
  } from "$lib/types";
  import FoldersPane from "$lib/FoldersPane.svelte";
  import FilesPane from "$lib/FilesPane.svelte";
  import TimelinePane from "$lib/TimelinePane.svelte";
  import CompareView from "$lib/CompareView.svelte";
  import StatusBar from "$lib/StatusBar.svelte";
  import SettingsDialog from "$lib/SettingsDialog.svelte";

  let settingsOpen = $state(false);

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
  let fileStats = $state<Loadable<FileStats> | null>(null);

  let justArrivedId = $state<number | null>(null);

  // The full-screen text comparison. `null` means the three panes own the
  // window; set, it takes over instead. The panes stay mounted underneath
  // (just hidden) rather than being torn down, so Escape/Done hands back the
  // exact same file, selection and timeline scroll position, per DESIGN.md.
  let compareTarget = $state<{
    olderId: number;
    newerId: number;
    entryName: string;
    partLabel: string;
  } | null>(null);

  let selectedVault = $derived(
    vaults.status === "loaded" ? (vaults.data.find((v) => v.id === selectedVaultId) ?? null) : null,
  );
  let selectedFile = $derived(
    files && files.status === "loaded"
      ? (files.data.find((f) => f.id === selectedFileId) ?? null)
      : null,
  );
  let watchedCount = $derived(
    vaults.status === "loaded" ? vaults.data.filter((v) => v.hasTracerFolder).length : null,
  );
  function findRevision(id: number) {
    return revisions && revisions.status === "loaded"
      ? (revisions.data.find((r) => r.id === id) ?? null)
      : null;
  }
  let compareOlder = $derived(compareTarget ? findRevision(compareTarget.olderId) : null);
  let compareNewer = $derived(compareTarget ? findRevision(compareTarget.newerId) : null);

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
    fileStats = null;
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
    fileStats = { status: "loading" };
    try {
      revisions = { status: "loaded", data: await api.getTimeline(id) };
    } catch (e) {
      revisions = { status: "error", message: errorMessage(e) };
    }
    try {
      fileStats = { status: "loaded", data: await api.getFileStats(id) };
    } catch (e) {
      fileStats = { status: "error", message: errorMessage(e) };
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

  async function refreshFileStats() {
    if (selectedFileId === null) return;
    try {
      fileStats = { status: "loaded", data: await api.getFileStats(selectedFileId) };
    } catch (e) {
      fileStats = { status: "error", message: errorMessage(e) };
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

  async function handleCompare(olderId: number, newerId: number): Promise<CompareResult> {
    return api.compareRevisions(olderId, newerId);
  }

  async function handleComparePartText(
    olderId: number,
    newerId: number,
    entryName: string,
  ): Promise<TextComparison> {
    return api.comparePartText(olderId, newerId, entryName);
  }

  function handleOpenCompare(olderId: number, newerId: number, entryName: string, partLabel: string) {
    compareTarget = { olderId, newerId, entryName, partLabel };
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
        refreshFileStats();
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

<main class="app">
  <div class="title-bar">
    <span class="title-bar-text">Tracer</span>
  </div>

  {#if !compareTarget}
    <StatusBar {watchedCount} onOpenSettings={() => (settingsOpen = true)} />
  {/if}

  <!-- Kept mounted (just hidden) while a comparison is open, rather than
       destroyed - that's what lets Escape/Done hand back the exact file,
       selection and scroll position the user left, per DESIGN.md. -->
  <div class="panes" class:hidden={!!compareTarget}>
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
      {fileStats}
      {justArrivedId}
      onSetMilestone={handleSetMilestone}
      onSaveCopy={handleSaveCopy}
      onCompare={handleCompare}
      onOpenCompare={handleOpenCompare}
    />
  </div>

  {#if compareTarget && compareOlder && compareNewer}
    <CompareView
      older={compareOlder}
      newer={compareNewer}
      fileName={selectedFile?.relativePath ?? ""}
      partLabel={compareTarget.partLabel}
      onLoad={() =>
        handleComparePartText(compareTarget!.olderId, compareTarget!.newerId, compareTarget!.entryName)}
      onDone={() => (compareTarget = null)}
    />
  {/if}

  {#if !compareTarget}
    <footer class="app-footer">
      Tracer versions files, not the folder. Lose the folder and its history goes too.
    </footer>
  {/if}
</main>

<SettingsDialog open={settingsOpen} onClose={() => (settingsOpen = false)} {watchedCount} />

<style>
  .app {
    display: flex;
    flex-direction: column;
    height: 100vh;
  }

  .title-bar {
    display: flex;
    align-items: center;
    height: 28px;
    flex: none;
    padding: 0 10px;
    /* The one loud element in the app, straight out of Windows 95: a navy-
       to-blue gradient behind white bold text. Everything below stays quiet. */
    background: var(--title-fill);
    border-bottom: 1px solid var(--edge);
  }

  .title-bar-text {
    color: var(--title-ink);
    font-size: 13px;
    font-weight: bold;
    letter-spacing: 0.02em;
  }

  .panes {
    flex: 1;
    display: grid;
    grid-template-columns: minmax(170px, 1fr) minmax(200px, 1.1fr) minmax(280px, 1.8fr);
    gap: 8px;
    min-height: 0;
    padding: 8px;
    background: var(--chrome);
  }

  .panes > :global(section) {
    min-height: 0;
  }

  .panes.hidden {
    display: none;
  }

  .app-footer {
    flex: none;
    padding: 4px 12px;
    border-top: 1px solid var(--edge);
    background: var(--chrome);
    font-size: 11px;
    opacity: 0.65;
    text-align: center;
  }
</style>
