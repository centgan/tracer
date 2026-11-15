<script lang="ts">
  import * as api from "./api";

  let { watchedCount, onOpenSettings }: { watchedCount: number | null; onOpenSettings: () => void } =
    $props();

  let quitting = $state(false);
  let quitError = $state<string | null>(null);

  function errorMessage(e: unknown): string {
    return e instanceof Error ? e.message : String(e);
  }

  async function handleQuit() {
    quitting = true;
    quitError = null;
    try {
      await api.quitApp();
    } catch (e) {
      quitError = errorMessage(e);
      quitting = false;
    }
  }
</script>

<div class="status-strip">
  <div class="status-left">
    <span class="dot" aria-hidden="true">●</span>
    <span>
      {watchedCount === null
        ? "Tracer is running"
        : `Watching ${watchedCount} ${watchedCount === 1 ? "folder" : "folders"}`}
    </span>
  </div>
  <div class="status-right">
    <button onclick={onOpenSettings}>Settings</button>
    <button onclick={handleQuit} disabled={quitting}>
      {quitting ? "Quitting…" : "Quit Tracer"}
    </button>
  </div>
</div>
{#if quitError}
  <p class="error-text quit-error">{quitError}</p>
{/if}

<style>
  .status-strip {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 5px 12px;
    min-height: 28px;
    background: var(--chrome);
    border-bottom: 1px solid var(--edge);
  }

  .status-left {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .dot {
    color: var(--live);
    line-height: 1;
    font-size: 10px;
  }

  .status-right {
    display: flex;
    gap: 8px;
  }

  .quit-error {
    padding: 2px 12px;
    background: var(--chrome);
  }
</style>
