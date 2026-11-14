<script lang="ts">
  import * as api from "./api";
  import type { Loadable } from "./types";

  let { watchedCount }: { watchedCount: number | null } = $props();

  let autostart = $state<Loadable<boolean>>({ status: "loading" });
  let quitting = $state(false);
  let quitError = $state<string | null>(null);

  // A failed toggle is reported beside the checkbox rather than as a Loadable
  // error, because an error status disables the control - which would leave the
  // user unable to retry the very thing that just failed.
  let autostartError = $state<string | null>(null);

  function errorMessage(e: unknown): string {
    return e instanceof Error ? e.message : String(e);
  }

  async function loadAutostart() {
    autostart = { status: "loading" };
    try {
      autostart = { status: "loaded", data: await api.isAutostartEnabled() };
    } catch (e) {
      autostart = { status: "error", message: errorMessage(e) };
    }
  }

  async function toggleAutostart(enabled: boolean) {
    if (autostart.status !== "loaded") return;
    const previous = autostart.data;
    autostart = { status: "loaded", data: enabled };
    autostartError = null;
    try {
      await api.setAutostart(enabled);
    } catch (e) {
      autostart = { status: "loaded", data: previous };
      autostartError = errorMessage(e);
    }
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

  loadAutostart();
</script>

<div class="status-bar">
  <div class="status-row">
    <span class="dot running" aria-hidden="true"></span>
    <p class="status-text">
      Tracer is running{watchedCount !== null
        ? ` and watching ${watchedCount} ${watchedCount === 1 ? "folder" : "folders"}`
        : ""}. Closing this window does not stop it — Tracer keeps running in the menu bar.
    </p>
    <div class="quit">
      <button class="quit-btn" onclick={handleQuit} disabled={quitting}>
        {quitting ? "Quitting…" : "Quit Tracer"}
      </button>
      <p class="quit-hint">Stops watching all folders until you open Tracer again.</p>
    </div>
  </div>
  {#if quitError}
    <p class="banner error">{quitError}</p>
  {/if}

  <label class="autostart">
    <input
      type="checkbox"
      checked={autostart.status === "loaded" && autostart.data}
      disabled={autostart.status !== "loaded"}
      onchange={(e) => toggleAutostart(e.currentTarget.checked)}
    />
    Start Tracer when I log in
  </label>
  <p class="hint">
    With this off, Tracer only records versions while it's running, so files saved after a
    restart aren't kept until you open Tracer again.
  </p>
  {#if autostart.status === "error"}
    <p class="banner error">
      Couldn't check whether Tracer starts at login: {autostart.message}
      <button class="retry" onclick={loadAutostart}>Try again</button>
    </p>
  {:else if autostartError}
    <p class="banner error">Couldn't change that setting: {autostartError}</p>
  {/if}
</div>

<style>
  .status-bar {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
    padding: 0.5rem 1.2rem 0.7rem;
    border-bottom: 1px solid var(--border);
    background: var(--surface);
  }

  .status-row {
    display: flex;
    align-items: center;
    gap: 0.6rem;
  }

  .dot {
    flex: none;
    width: 0.55rem;
    height: 0.55rem;
    border-radius: 50%;
  }

  .dot.running {
    background: var(--ok);
  }

  .status-text {
    flex: 1;
    margin: 0;
    font-size: 0.82rem;
    color: var(--muted);
  }

  .quit {
    flex: none;
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 0.1rem;
  }

  .quit-btn {
    white-space: nowrap;
  }

  .quit-hint {
    margin: 0;
    font-size: 0.7rem;
    color: var(--muted);
  }

  .autostart {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    font-size: 0.85rem;
  }

  .hint {
    margin: 0 0 0 1.45rem;
    color: var(--muted);
    font-size: 0.75rem;
    line-height: 1.4;
  }

  .banner.error {
    margin: 0;
    color: var(--error);
    font-size: 0.8rem;
  }

  .retry {
    margin-left: 0.4rem;
    padding: 0.1rem 0.45rem;
    border: 1px solid currentColor;
    border-radius: 4px;
    background: none;
    color: inherit;
    font: inherit;
    cursor: pointer;
  }
</style>
