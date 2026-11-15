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

  // Office's own colours for its three formats - already in the user's head,
  // so reusing them beats inventing a new scheme. Anything else gets a
  // neutral glyph: a colour with no meaning behind it is just decoration.
  const FILE_KIND_COLOR: Record<string, string> = {
    doc: "var(--doc)",
    docx: "var(--doc)",
    xls: "var(--sheet)",
    xlsx: "var(--sheet)",
    ppt: "var(--deck)",
    pptx: "var(--deck)",
  };

  function fileGlyphColor(extension: string): string {
    return FILE_KIND_COLOR[extension.toLowerCase()] ?? "var(--face-dark)";
  }
</script>

<section class="pane">
  <div class="pane-header">
    <h2>Files</h2>
  </div>

  <div class="well">
    {#if !selectedVault || !files}
      <p class="hint pad">Pick a folder on the left to see what's tracked in it.</p>
    {:else if files.status === "loading"}
      <p class="hint pad">Looking for tracked files…</p>
    {:else if files.status === "error"}
      <p class="error-text pad">{files.message}</p>
    {:else if files.data.length === 0}
      <p class="hint pad">Files appear here once they're saved in this folder.</p>
    {:else}
      <ul class="list">
        {#each files.data as file (file.id)}
          <li>
            <button
              class="row"
              class:selected={file.id === selectedFileId}
              onclick={() => onSelect(file.id)}
            >
              <span class="file-info">
                <svg class="file-glyph" viewBox="0 0 12 14" aria-hidden="true" focusable="false">
                  <path
                    d="M1 1h6l4 4v8a1 1 0 0 1-1 1H2a1 1 0 0 1-1-1V1z"
                    fill={fileGlyphColor(file.extension)}
                    stroke="var(--edge)"
                    stroke-width="1"
                  />
                  <path d="M7 1v4h4" fill="none" stroke="var(--edge)" stroke-width="1" />
                </svg>
                <span class="name">{file.relativePath}</span>
              </span>
              <span class="when">
                {file.lastChanged === null ? "no versions" : formatRelativeTime(file.lastChanged)}
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
    align-items: baseline;
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

  .file-info {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }

  .file-glyph {
    flex: none;
    width: 12px;
    height: 14px;
  }

  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }

  .when {
    flex: none;
    font-family: var(--data);
    font-size: 11px;
    opacity: 0.75;
  }

  .row.selected {
    background: var(--select);
    color: #fff;
  }

  .row.selected .when {
    opacity: 1;
  }
</style>
