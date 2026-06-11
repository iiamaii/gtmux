<script lang="ts">
  /**
   * ExportSessionModal — ADR-0029 D4/D11/D13.
   *
   * Active session 의 layout 을 `gtmux Session Export Envelope v1` 로
   * download. BE work package 0052 (`GET /api/sessions/{name}/export`) 의존.
   *
   * 동작:
   * 1. 사용자가 SessionMenu/Settings 의 [Export layout…] 클릭 → sessionIODialog.openExport()
   * 2. 본 modal 이 active session name + privacy warning 표시
   * 3. [Download] 클릭 → `exportSession(name)` → blob URL 생성 → 임시 a 클릭 → revoke
   * 4. 성공 시 toast + close
   *
   * Privacy warning (ADR-0029 D11):
   * - inline content (notes / text / document / caption) 포함 가능
   * - local file paths 포함 가능
   * - terminal output / process state 제외
   * - import 시 unmatched terminal panels 는 새 spawn 가능
   *
   * Last-saved 주의 (ADR-0029 D13):
   * - 현재 sessionStore 의 viewport debounce (500ms) 만 stale 가능.
   *   button label 에 명시.
   */

  import Modal from '$lib/ui/Modal.svelte';
  import Button from '$lib/ui/Button.svelte';
  import { sessionStore } from '$lib/stores/sessionStore.svelte';
  import { sessionIODialog } from '$lib/stores/sessionIOdialog.svelte';
  import {
    exportSession,
    UnauthorizedError,
  } from '$lib/http/sessions';
  import { toastStore } from '$lib/ui/toast-store.svelte';

  const open = $derived(sessionIODialog.mode === 'export');
  const activeName = $derived(sessionStore.active?.name ?? null);

  let downloading = $state(false);

  function close(): void {
    if (downloading) return;
    sessionIODialog.close();
  }

  async function onDownload(): Promise<void> {
    if (activeName === null || downloading) return;
    const sessionName = activeName;
    downloading = true;
    try {
      await sessionStore.flushPendingViewport(sessionName);
      const { blob, filename } = await exportSession(sessionName);
      // Trigger browser download via temp <a download>.
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = filename;
      document.body.appendChild(a);
      a.click();
      document.body.removeChild(a);
      // Revoke after a tick — Safari/Firefox 안전.
      setTimeout(() => URL.revokeObjectURL(url), 1_000);
      toastStore.show({
        message: `Exported layout "${sessionName}" to ${filename}`,
        tone: 'success',
      });
      sessionIODialog.close();
    } catch (err) {
      if (err instanceof UnauthorizedError) {
        window.location.href = '/auth';
        return;
      }
      toastStore.show({
        message: `Export failed: ${err instanceof Error ? err.message : String(err)}`,
        tone: 'error',
        durationMs: 7_000,
      });
    } finally {
      downloading = false;
    }
  }
</script>

<Modal {open} onclose={close} title="Export layout">
  {#snippet body()}
    {#if activeName === null}
      <p class="modal-state">No active layout — open a session first.</p>
    {:else}
      <div class="section">
        <div class="kv">
          <span class="k">Layout</span>
          <span class="v mono">{activeName}</span>
        </div>
        <div class="kv">
          <span class="k">Format</span>
          <span class="v mono">gtmux layout export · v1</span>
        </div>
      </div>

      <div class="warn">
        <h4>What's included</h4>
        <ul>
          <li>Notes, text, document inline content</li>
          <li>File path references (path strings only — no file contents)</li>
          <li>Image/document asset references (IDs only — no binary assets)</li>
          <li>Item geometry, colors, layout structure</li>
        </ul>
        <h4>What's excluded</h4>
        <ul>
          <li>Terminal output and process state</li>
          <li>Image/document binary asset data</li>
          <li>Local OS files referenced by file_path items</li>
          <li>Auth credentials, settings, allowlists</li>
        </ul>
        <p class="caveat">
          Imported terminal panels will spawn fresh shells when first attached.
          Pending viewport changes are flushed before export starts.
        </p>
      </div>
    {/if}
  {/snippet}
  {#snippet footer()}
    <Button variant="ghost" onclick={close} disabled={downloading}>Cancel</Button>
    <Button
      variant="primary"
      onclick={() => void onDownload()}
      disabled={activeName === null || downloading}
    >
      {downloading ? 'Exporting…' : 'Download'}
    </Button>
  {/snippet}
</Modal>

<style>
  .section {
    display: grid;
    gap: var(--space-6);
    margin-bottom: var(--space-12);
  }

  .kv {
    display: grid;
    grid-template-columns: 80px 1fr;
    align-items: baseline;
    gap: var(--space-8);
    font-size: var(--text-md);
  }

  .kv .k {
    color: var(--color-fg-muted);
    text-transform: uppercase;
    font-size: 10px;
    letter-spacing: 0.5px;
  }

  .kv .v.mono {
    font-family: var(--font-mono);
    font-size: var(--text-base);
    color: var(--color-fg);
  }

  .warn {
    border: 1px solid var(--color-border);
    background: var(--color-surface-2);
    border-radius: var(--radius-md);
    padding: var(--space-10) var(--space-12);
    font-size: var(--text-sm);
    color: var(--color-fg-muted);
  }

  .warn h4 {
    margin: 0 0 var(--space-4);
    font-size: 10px;
    letter-spacing: 0.5px;
    text-transform: uppercase;
    color: var(--color-fg);
    font-weight: var(--weight-semibold);
  }

  .warn h4:not(:first-child) {
    margin-top: var(--space-10);
  }

  .warn ul {
    margin: 0;
    padding-left: var(--space-16);
  }

  .warn li {
    margin: 2px 0;
  }

  .warn .caveat {
    margin: var(--space-10) 0 0;
    font-style: italic;
    color: var(--color-fg-subtle);
  }
</style>
