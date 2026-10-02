<script lang="ts">
  import { onMount } from 'svelte';
  import Button from '$lib/ui/Button.svelte';
  import ShutdownModal from './ShutdownModal.svelte';
  import { serverStatus, type ServerStatus } from '$lib/http/shutdown';
  import ReauthModal from './ReauthModal.svelte';
  import { configRequest, type ConfigSnapshot, type ServerConfig } from '$lib/http/config';
  import { settingsStore } from '$lib/stores/settings.svelte';
  import { configEditor as editor } from '$lib/stores/configEditor.svelte';
  import { InvalidCredentialError, CredentialRequiredError, RateLimitedError } from '$lib/http/stepup';
  let busy = $state(false), reauth = $state(false), discard = $state(false);
  let error = $state(''), message = $state('');
  let stopOpen = $state(false), lifecycle = $state<ServerStatus | null>(null), lifecycleError = $state('');
  async function refreshStatus(): Promise<void> {
    try { lifecycle = await serverStatus(); lifecycleError = ''; }
    catch (e) { lifecycleError = e instanceof Error ? e.message : String(e); }
  }
  onMount(() => { void refreshStatus(); });
  const dirty = $derived(editor.fieldsDirty || editor.contents !== editor.snapshot?.contents);
  function accept(snapshot: ConfigSnapshot): void {
    editor.snapshot = snapshot;
    editor.contents = snapshot.contents ?? '';
    const config = snapshot.saved ?? snapshot.running;
    if (config) {
      editor.port = config.server.port;
      editor.workspace = config.server_workspace ?? '';
      editor.sessionWorkspace = config.default_session_workspace ?? '';
    }
    editor.fieldsDirty = false;
  }
  async function reload(): Promise<void> {
    busy = true; error = ''; discard = false; message = '';
    try { accept(await configRequest<ConfigSnapshot>('/api/config')); }
    catch (e) { error = String(e instanceof Error ? e.message : e); }
    finally { busy = false; }
  }
  onMount(() => { if (!editor.snapshot) void reload(); });
  async function applyFields(): Promise<boolean> {
    busy = true; error = ''; message = '';
    try {
      const result = await configRequest<{ contents: string; saved: ServerConfig }>('/api/config/preview', 'POST', {
        contents: editor.contents, port: editor.port, server_workspace: editor.workspace, default_session_workspace: editor.sessionWorkspace,
      });
      editor.contents = result.contents; editor.fieldsDirty = false;
      message = 'Draft updated. Save configuration to write it to disk.';
      return true;
    } catch (e) { error = e instanceof Error ? e.message : String(e); return false; }
    finally { busy = false; }
  }
  async function prepareSave(): Promise<void> {
    if (editor.fieldsDirty && !await applyFields()) return;
    reauth = true;
  }
  async function save(credential: string): Promise<void> {
    error = ''; busy = true;
    try {
      accept(await configRequest<ConfigSnapshot>('/api/config', 'PUT', {
        contents: editor.contents, revision: editor.snapshot?.revision, credential,
      }));
      await settingsStore.load();
      message = 'Configuration saved. Behavior settings apply now; other settings apply on restart.';
    } catch (e) {
      if (e instanceof InvalidCredentialError || e instanceof CredentialRequiredError || e instanceof RateLimitedError) throw e;
      error = e instanceof Error ? e.message : String(e);
    } finally { busy = false; }
  }
</script>

<h3>Server control</h3>
<p class="hint">Manage the server shared by all sessions and browser tabs.</p>
{#if lifecycle}
  <p class="hint" role="status">{lifecycle.instance} · {lifecycle.state} · {lifecycle.active_terminals} active terminals · {lifecycle.attached_sessions} attached sessions</p>
  <div class="actions">
    <Button variant="danger" disabled={!lifecycle.can_shutdown || lifecycle.state === 'stopping'} onclick={() => stopOpen = true}>Stop server…</Button>
    <Button variant="ghost" onclick={() => void refreshStatus()}>Refresh status</Button>
  </div>
  {#if !lifecycle.can_shutdown}<p class="hint">Server lifecycle is managed by the embedding host.</p>{/if}
  <p class="hint">Restart using the CLI or host application. Automatic restart and background app controls are not available in this server.</p>
{:else}<p class="hint" role="status">{lifecycleError || 'Loading server status…'}</p>{/if}
<ShutdownModal open={stopOpen} sessionName={lifecycle?.instance ?? ''} onclose={() => { stopOpen = false; void refreshStatus(); }} />

<h3>Server configuration</h3>
<p class="hint">Save server settings to TOML. Behavior settings apply immediately; other changes apply on the next start. Command-line flags and environment variables still take precedence over this file.</p>
{#if error}<p class="error" role="alert">{error}</p>{/if}
{#if message}<p class="hint" role="status">{message}</p>{/if}
{#if !editor.snapshot}
  <p class="hint">{busy ? 'Loading configuration…' : 'Configuration unavailable.'}</p>
  <Button disabled={busy} onclick={() => void reload()}>Retry</Button>
{:else if !editor.snapshot.available}
  <p class="hint">{editor.snapshot.reason}</p>
{:else}
  <p class="path">{editor.snapshot.path}</p>
  <p class="hint">Running at {editor.snapshot.running.server.bind}:{editor.snapshot.running.server.port}</p>
  {#if editor.snapshot.restart_required}<p class="notice" role="status">Configuration file changed since startup. Restart the server to load the saved values.</p>{/if}
  {#if editor.snapshot.validation_error}<p class="error">Saved file needs correction: {editor.snapshot.validation_error}</p>{/if}
  <fieldset disabled={busy}>
    <legend>Common settings</legend>
    <label class="row"><span>Port<span class="hint">1024–65535. Port availability is checked when the server starts.</span></span>
      <input type="number" min="1024" max="65535" bind:value={editor.port} oninput={() => editor.fieldsDirty = true} /></label>
    <label class="row"><span>Server workspace<span class="hint">Root folder the server can access. Leave blank to use the account home.</span></span>
      <input type="text" bind:value={editor.workspace} oninput={() => editor.fieldsDirty = true} /></label>
    <label class="row"><span>Default session workspace<span class="hint">Existing folder inside the server workspace. Leave blank for the default.</span></span>
      <input type="text" bind:value={editor.sessionWorkspace} oninput={() => editor.fieldsDirty = true} /></label>
    <Button disabled={!editor.fieldsDirty || busy} onclick={() => void applyFields()}>Apply fields to draft</Button>
    <details>
      <summary>Advanced TOML configuration</summary>
      <p class="hint">Includes bind address, security, proxy and runtime settings. Preserve security checks when exposing the server remotely. Existing comments are retained.</p>
      <label for="server-config-document">Configuration draft</label>
      <textarea id="server-config-document" spellcheck="false" rows="18" bind:value={editor.contents} disabled={editor.fieldsDirty}
        aria-describedby="config-document-help"></textarea>
      <p class="hint" id="config-document-help">Apply common fields before editing TOML directly. Unsaved edits stay here when switching Settings sections.</p>
    </details>
  </fieldset>
  <div class="actions">
    <Button variant="primary" disabled={busy || !dirty} onclick={() => void prepareSave()}>Save configuration</Button>
    <Button disabled={busy} onclick={() => { if (dirty) discard = true; else void reload(); }}>Reload from disk</Button>
    {#if dirty}<span class="hint">Unsaved changes</span>{/if}
  </div>
  {#if discard}<div class="actions" role="group" aria-label="Discard draft">
    <span class="hint">Discard your draft and reload the file?</span>
    <Button onclick={() => void reload()}>Discard and reload</Button><Button variant="ghost" onclick={() => discard = false}>Keep editing</Button>
  </div>{/if}
{/if}
<ReauthModal open={reauth} title="Save server configuration" description="Confirm writing these settings to the server configuration file. The running server will not restart."
  confirmLabel="Save configuration" onSubmit={save} onCancel={() => reauth = false} />
<style>
  h3 { margin: 0; padding: 24px 0 5px; font-size: var(--text-xl); font-weight: var(--weight-semibold); }
  .hint { display: block; color: var(--color-fg-muted); font-size: var(--text-base); line-height: var(--leading-normal); max-width: 62ch; }
  .path { font: var(--text-base) var(--font-mono); overflow-wrap: anywhere; }
  .error { color: var(--color-danger); font-size: var(--text-base); overflow-wrap: anywhere; }
  .notice { color: var(--color-warning); font-size: var(--text-base); }
  fieldset { border: 0; padding: 0; min-width: 0; margin: 24px 0 16px; }
  legend { color: var(--color-fg-muted); font: var(--text-xs) var(--font-mono); text-transform: uppercase; letter-spacing: .6px; }
  .row { display: grid; grid-template-columns: minmax(0, 1fr) minmax(140px, 1fr); align-items: center; gap: 28px;
    padding: 14px 0; border-bottom: 1px solid var(--color-border); font-size: var(--text-md); }
  input, textarea { box-sizing: border-box; min-width: 0; width: 100%; border: 1px solid var(--color-border);
    border-radius: var(--radius-md); background: var(--color-surface); color: var(--color-fg); font: inherit; font-size: var(--text-base); padding: 7px 10px; }
  textarea { margin-top: 8px; resize: vertical; font-family: var(--font-mono); }
  input:hover:not(:disabled), textarea:hover:not(:disabled) { border-color: var(--color-border-strong); }
  input:focus-visible, textarea:focus-visible, summary:focus-visible { outline: 2px solid var(--color-info); outline-offset: 2px; }
  input:disabled, textarea:disabled { opacity: .5; }
  details { margin-top: 24px; }
  summary { cursor: pointer; font-size: var(--text-md); }
  .actions { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; margin: 14px 0; }
  @media (max-width: 720px) { .row { grid-template-columns: 1fr; gap: 10px; } }
</style>
