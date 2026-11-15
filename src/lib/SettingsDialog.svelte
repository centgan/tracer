<script lang="ts">
  import * as api from "./api";
  import type { Identity, Loadable } from "./types";
  import { shouldShowIdentityNudge } from "./identity";

  let {
    open,
    onClose,
    watchedCount,
  }: { open: boolean; onClose: () => void; watchedCount: number | null } = $props();

  let dialogEl = $state<HTMLDialogElement>();
  let loaded = false;

  let autostart = $state<Loadable<boolean>>({ status: "loading" });
  let autostartError = $state<string | null>(null);

  let identity = $state<Loadable<Identity>>({ status: "loading" });
  let nameDraft = $state("");
  let emailDraft = $state("");
  let identityError = $state<string | null>(null);

  let showIdentityNudge = $derived(
    identity.status === "loaded" && shouldShowIdentityNudge(identity.data, watchedCount),
  );

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

  async function loadIdentity() {
    identity = { status: "loading" };
    try {
      const data = await api.getIdentity();
      identity = { status: "loaded", data };
      nameDraft = data.name;
      emailDraft = data.email;
    } catch (e) {
      identity = { status: "error", message: errorMessage(e) };
    }
  }

  async function saveIdentity() {
    if (identity.status !== "loaded") return;
    const name = nameDraft.trim();
    const email = emailDraft.trim();
    if (name === identity.data.name && email === identity.data.email) return;
    identityError = null;
    try {
      await api.setIdentity({ name, email });
      identity = { status: "loaded", data: { name, email } };
    } catch (e) {
      identityError = errorMessage(e);
    }
  }

  $effect(() => {
    if (!dialogEl) return;
    if (open) {
      if (!loaded) {
        loaded = true;
        loadAutostart();
        loadIdentity();
      }
      if (!dialogEl.open) dialogEl.showModal();
    } else if (dialogEl.open) {
      dialogEl.close();
    }
  });
</script>

<dialog bind:this={dialogEl} class="dialog" onclose={onClose}>
  <div class="dialog-title-bar">
    <span>Settings</span>
    <button class="plain close" onclick={() => dialogEl?.close()} aria-label="Close settings">
      ✕
    </button>
  </div>

  <div class="dialog-body">
    <div class="field">
      <label for="settings-name">Your name</label>
      <input
        id="settings-name"
        type="text"
        bind:value={nameDraft}
        disabled={identity.status !== "loaded"}
        onblur={saveIdentity}
      />
    </div>
    <div class="field">
      <label for="settings-email">Email</label>
      <input
        id="settings-email"
        type="email"
        bind:value={emailDraft}
        disabled={identity.status !== "loaded"}
        onblur={saveIdentity}
      />
    </div>
    <p class="hint">Labels versions you create, so others sharing the folder know who made them.</p>
    {#if showIdentityNudge}
      <p class="hint">Versions you create aren't labelled yet.</p>
    {/if}
    {#if identity.status === "error"}
      <p class="error-text">
        {identity.message}
        <button class="plain" onclick={loadIdentity}>Try again</button>
      </p>
    {:else if identityError}
      <p class="error-text">{identityError}</p>
    {/if}

    <label class="checkbox-row">
      <input
        type="checkbox"
        checked={autostart.status === "loaded" && autostart.data}
        disabled={autostart.status !== "loaded"}
        onchange={(e) => toggleAutostart(e.currentTarget.checked)}
      />
      Start Tracer when I log in
    </label>
    <p class="hint">Off: versions stop recording after a restart, until you reopen Tracer.</p>
    {#if autostart.status === "error"}
      <p class="error-text">
        {autostart.message}
        <button class="plain" onclick={loadAutostart}>Try again</button>
      </p>
    {:else if autostartError}
      <p class="error-text">{autostartError}</p>
    {/if}
  </div>

  <div class="dialog-actions">
    <button onclick={() => dialogEl?.close()}>OK</button>
  </div>
</dialog>

<style>
  .dialog {
    width: 320px;
    padding: 0;
    background: var(--face);
    border: 1px solid var(--edge);
    box-shadow:
      inset 1px 1px 0 var(--light),
      inset -1px -1px 0 var(--shade);
    color: var(--ink);
  }

  .dialog::backdrop {
    background: rgba(0, 0, 0, 0.25);
  }

  .dialog-title-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 3px 6px 3px 8px;
    background: repeating-linear-gradient(
      to bottom,
      var(--face) 0px,
      var(--face) 2px,
      var(--chrome) 2px,
      var(--chrome) 4px
    );
    border-bottom: 1px solid var(--edge);
    font-size: 13px;
    font-weight: bold;
  }

  .close {
    font-size: 11px;
    line-height: 1;
  }

  .dialog-body {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 12px;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .field label {
    font-size: 11px;
    font-weight: bold;
  }

  .checkbox-row {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-top: 6px;
    font-size: 13px;
  }

  .hint {
    margin-left: 0;
  }

  .dialog-actions {
    display: flex;
    justify-content: flex-end;
    padding: 8px 12px 12px;
  }
</style>
