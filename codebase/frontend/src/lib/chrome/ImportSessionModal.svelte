<script lang="ts">
  /**
   * ImportSessionModal — ADR-0029 D5/D6/D7/D12.
   *
   * 흐름:
   *   pick → parse envelope → preview → name confirm → POST import
   *     → 성공 → "Open imported layout?" confirm
   *     → 409 → name 재입력 (rename)
   *     → 400/other → toast
   *
   * Side-effect-free (ADR-0029 D5): import 자체는 새 layout-backed session file 만 생성.
   * "Open" 선택 시에만 detach + attach.
   */

  import Modal from '$lib/ui/Modal.svelte';
  import Button from '$lib/ui/Button.svelte';
  import { sessionStore } from '$lib/stores/sessionStore.svelte';
  import { sessionIODialog } from '$lib/stores/sessionIOdialog.svelte';
  import { workspaceSwitcher } from '$lib/stores/workspaceSwitcher.svelte';
  import {
    EnvelopeParseError,
    ImportNameConflictError,
    ImportSchemaError,
    UnauthorizedError,
    attachSession,
    detachSession,
    importSession,
    parseEnvelope,
    SESSION_NAME_REGEX,
    type SessionExportEnvelope,
  } from '$lib/http/sessions';
  import { getWebpageId } from '$lib/session/webpageId';
  import type { CanvasItem } from '$lib/types/canvas';
  import { toastStore } from '$lib/ui/toast-store.svelte';

  const open = $derived(sessionIODialog.mode === 'import');

  // Client-side body cap — BE `sessions::SESSION_PUT_MAX_BYTES` 와 동일 16 MiB.
  // 초과 시 friendly toast/inline error (네트워크 안 타고 차단), 동일 값을 BE 가
  // 413 으로 보장 (ADR-0029 §6 / amend ②).
  const IMPORT_MAX_BYTES = 16 * 1024 * 1024;

  // Stage 머신:
  //   pick      → file picker 노출
  //   preview   → envelope OK, target name confirm
  //   importing → POST in-flight
  //   done      → "Open imported layout?" 확인
  type Stage = 'pick' | 'preview' | 'importing' | 'done';
  let stage = $state<Stage>('pick');
  let envelope = $state<SessionExportEnvelope | null>(null);
  let sourceFilename = $state<string>('');
  let targetName = $state<string>('');
  let nameError = $state<string | null>(null);
  let parseError = $state<string | null>(null);
  let importedName = $state<string | null>(null);
  let activeSwapping = $state(false);

  // Reset 시 open 토글로 회귀.
  $effect(() => {
    if (!open) {
      stage = 'pick';
      envelope = null;
      sourceFilename = '';
      targetName = '';
      nameError = null;
      parseError = null;
      importedName = null;
      activeSwapping = false;
    }
  });

  // Preview metrics — schema v2 layout 안의 type 별 count.
  const preview = $derived.by(() => {
    if (envelope === null) return null;
    const items = envelope.layout.items as CanvasItem[];
    const counts = {
      total: items.length,
      groups: envelope.layout.groups.length,
      terminal: 0,
      text: 0,
      note: 0,
      inlineDocument: 0,
      assetReference: 0,
      file_path: 0,
      shape: 0,
      line: 0,
      other: 0,
    };
    for (const it of items) {
      switch (it.type) {
        case 'terminal': counts.terminal += 1; break;
        case 'text': counts.text += 1; break;
        case 'note': counts.note += 1; break;
        case 'file_path': counts.file_path += 1; break;
        case 'image':
          if ((it.asset_id ?? '').length > 0) counts.assetReference += 1;
          break;
        case 'document':
          if ((it.asset_id ?? '').length === 0 && it.content !== undefined) counts.inlineDocument += 1;
          if ((it.asset_id ?? '').length > 0) counts.assetReference += 1;
          break;
        case 'rect':
        case 'ellipse': counts.shape += 1; break;
        case 'line': counts.line += 1; break;
        default: counts.other += 1;
      }
    }
    return counts;
  });

  function close(): void {
    if (stage === 'importing' || activeSwapping) return;
    sessionIODialog.close();
  }

  function suggestName(env: SessionExportEnvelope, file: string): string {
    // ADR-0029 D6 — 추천 기본값: envelope.session_name → filename → fallback.
    const fromEnv = env.session_name.trim();
    if (fromEnv.length > 0 && SESSION_NAME_REGEX.test(fromEnv)) return fromEnv;
    const stripped = file.replace(/\.gtmux-session\.json$/i, '').replace(/\.json$/i, '');
    if (SESSION_NAME_REGEX.test(stripped)) return stripped;
    return 'imported-layout';
  }

  function formatMB(bytes: number): string {
    const mb = bytes / (1024 * 1024);
    return mb >= 10 ? `${Math.round(mb)} MB` : `${mb.toFixed(1)} MB`;
  }

  async function onFileChange(e: Event): Promise<void> {
    const input = e.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    if (file === undefined) return;
    parseError = null;
    if (file.size > IMPORT_MAX_BYTES) {
      parseError = `Layout file too large (${formatMB(file.size)}). Maximum is ${formatMB(IMPORT_MAX_BYTES)}.`;
      input.value = '';
      return;
    }
    try {
      const text = await file.text();
      const parsed = parseEnvelope(JSON.parse(text));
      envelope = parsed;
      sourceFilename = file.name;
      targetName = suggestName(parsed, file.name);
      validateTargetName(targetName);
      stage = 'preview';
    } catch (err) {
      if (err instanceof EnvelopeParseError) {
        parseError = err.message;
      } else if (err instanceof SyntaxError) {
        parseError = 'File is not valid JSON.';
      } else {
        parseError = err instanceof Error ? err.message : String(err);
      }
    } finally {
      // reset input so re-picking the same file fires onchange.
      input.value = '';
    }
  }

  function validateTargetName(s: string): void {
    if (s.length === 0) {
      nameError = 'Name required.';
    } else if (!SESSION_NAME_REGEX.test(s)) {
      nameError = 'Only A–Z, a–z, 0–9, _, - (1–64 chars).';
    } else {
      nameError = null;
    }
  }

  function onNameInput(e: Event): void {
    targetName = (e.currentTarget as HTMLInputElement).value;
    validateTargetName(targetName);
  }

  async function onImport(): Promise<void> {
    if (envelope === null || nameError !== null) return;
    stage = 'importing';
    try {
      const res = await importSession(targetName, envelope.layout);
      importedName = res.name;
      stage = 'done';
    } catch (err) {
      stage = 'preview';
      if (err instanceof UnauthorizedError) {
        window.location.href = '/auth';
        return;
      }
      if (err instanceof ImportNameConflictError) {
        nameError = `"${err.name}" already exists. Pick a different name.`;
        return;
      }
      if (err instanceof ImportSchemaError) {
        parseError = `Schema invalid (${err.field}): ${err.details}`;
        return;
      }
      toastStore.show({
        message: `Import failed: ${err instanceof Error ? err.message : String(err)}`,
        tone: 'error',
      });
    }
  }

  async function onOpenImported(): Promise<void> {
    if (importedName === null || activeSwapping) return;
    activeSwapping = true;
    try {
      // 기존 active 가 있으면 silent detach (사용자 confirm modal 은 이미 본
      // 화면이 confirm 역할). 실패는 swallow — attach 가 다음 검증.
      if (sessionStore.active !== null) {
        try {
          await detachSession(sessionStore.active.name);
        } catch {
          // silent
        }
      }
      const wsConnId = getWebpageId();
      const res = await attachSession(importedName, { ws_conn_id: wsConnId });
      if (res.kind === 'ok') {
        sessionStore.setActiveSession({
          name: importedName,
          effectiveWorkspaceRoot: res.workspace_root,
        });
        sessionStore.loadLayout(res.layout);
        toastStore.show({
          message: `Opened imported layout "${importedName}".`,
          tone: 'success',
        });
        sessionIODialog.close();
        return;
      }
      if (res.kind === 'confirm_required') {
        // imported terminal items 가 unmatched → AttachConfirmModal 흐름.
        sessionStore.setActiveSession({
          name: importedName,
          effectiveWorkspaceRoot: res.workspace_root,
        });
        workspaceSwitcher.goAttachConfirm(importedName, res.summary);
        sessionIODialog.close();
        return;
      }
      // conflict
      toastStore.show({
        message: `Could not attach: another webpage is using "${importedName}".`,
        tone: 'warning',
      });
    } catch (err) {
      if (err instanceof UnauthorizedError) {
        window.location.href = '/auth';
        return;
      }
      toastStore.show({
        message: `Attach failed: ${err instanceof Error ? err.message : String(err)}`,
        tone: 'error',
      });
    } finally {
      activeSwapping = false;
    }
  }

  function onStayHere(): void {
    if (importedName !== null) {
      toastStore.show({
        message: `Imported layout "${importedName}" — open it from the workspace switcher when ready.`,
        tone: 'success',
      });
    }
    sessionIODialog.close();
  }
</script>

<Modal
  {open}
  onclose={close}
  title={stage === 'done' ? 'Imported' : 'Import layout'}
  dismissOnBackdrop={stage !== 'importing' && !activeSwapping}
  dismissOnEsc={stage !== 'importing' && !activeSwapping}
>
  {#snippet body()}
    {#if stage === 'pick'}
      <p class="modal-lead">
        Pick a gtmux layout export file (<code>.gtmux-session.json</code>).
      </p>
      <label class="file-pick">
        <input
          type="file"
          accept=".json,.gtmux-session.json,application/json"
          onchange={(e: Event) => void onFileChange(e)}
        />
        <span class="file-pick-label">Choose file…</span>
      </label>
      {#if parseError !== null}
        <div class="error" role="alert">{parseError}</div>
      {/if}
      <div class="hint">
        Imports are side-effect-free — only a new layout-backed session file is created.
        Terminal panels start fresh when first attached.
        Max file size: 16 MB.
      </div>
    {:else if stage === 'preview' || stage === 'importing'}
      {#if envelope !== null && preview !== null}
        <div class="section">
          <div class="kv">
            <span class="k">Source file</span>
            <span class="v mono" title={sourceFilename}>{sourceFilename}</span>
          </div>
          <div class="kv">
            <span class="k">Original name</span>
            <span class="v mono">{envelope.session_name}</span>
          </div>
          <div class="kv">
            <span class="k">Exported at</span>
            <span class="v mono">{envelope.exported_at}</span>
          </div>
        </div>

        <div class="counts">
          <div class="count-row"><span class="ct">total items</span><span class="cn">{preview.total}</span></div>
          {#if preview.groups > 0}<div class="count-row"><span class="ct">groups</span><span class="cn">{preview.groups}</span></div>{/if}
          {#if preview.terminal > 0}<div class="count-row"><span class="ct">terminal panels</span><span class="cn">{preview.terminal}</span></div>{/if}
          {#if preview.text > 0}<div class="count-row"><span class="ct">text</span><span class="cn">{preview.text}</span></div>{/if}
          {#if preview.note > 0}<div class="count-row"><span class="ct">notes</span><span class="cn">{preview.note}</span></div>{/if}
          {#if preview.inlineDocument > 0}<div class="count-row"><span class="ct">inline documents</span><span class="cn">{preview.inlineDocument}</span></div>{/if}
          {#if preview.assetReference > 0}<div class="count-row"><span class="ct">asset references</span><span class="cn">{preview.assetReference}</span></div>{/if}
          {#if preview.file_path > 0}<div class="count-row"><span class="ct">file paths</span><span class="cn">{preview.file_path}</span></div>{/if}
          {#if preview.shape > 0}<div class="count-row"><span class="ct">shapes</span><span class="cn">{preview.shape}</span></div>{/if}
          {#if preview.line > 0}<div class="count-row"><span class="ct">lines</span><span class="cn">{preview.line}</span></div>{/if}
          {#if preview.other > 0}<div class="count-row"><span class="ct">other</span><span class="cn">{preview.other}</span></div>{/if}
        </div>

        <div class="section">
          <label class="field">
            <span class="k">Import as</span>
            <input
              type="text"
              class="text-input mono"
              class:has-error={nameError !== null}
              value={targetName}
              maxlength={64}
              autocomplete="off"
              disabled={stage === 'importing'}
              oninput={onNameInput}
            />
          </label>
          {#if nameError !== null}
            <div class="error" role="alert">{nameError}</div>
          {/if}
        </div>

        {#if preview.terminal > 0}
          <div class="caveat">
            Terminal panels will be re-spawned (fresh shell) when the imported
            layout is first opened.
          </div>
        {/if}
        {#if preview.assetReference > 0}
          <div class="caveat">
            Binary assets are not bundled in layout export files. Asset-backed
            images/documents may appear dangling unless this workspace already
            has the referenced assets.
          </div>
        {/if}
      {/if}
    {:else if stage === 'done' && importedName !== null}
      <p class="modal-lead">
        Imported as <strong class="mono">{importedName}</strong>.
        {#if sessionStore.active !== null}
          Opening it will close the current session
          <span class="mono">{sessionStore.active.name}</span>.
        {/if}
      </p>
    {/if}
  {/snippet}
  {#snippet footer()}
    {#if stage === 'pick'}
      <Button variant="ghost" onclick={close}>Cancel</Button>
    {:else if stage === 'preview'}
      <Button variant="ghost" onclick={close}>Cancel</Button>
      <Button
        variant="primary"
        onclick={() => void onImport()}
        disabled={nameError !== null}
      >Import</Button>
    {:else if stage === 'importing'}
      <Button variant="ghost" onclick={close} disabled>Importing…</Button>
    {:else if stage === 'done'}
      <Button variant="ghost" onclick={onStayHere} disabled={activeSwapping}>Stay here</Button>
      <Button
        variant="primary"
        onclick={() => void onOpenImported()}
        disabled={activeSwapping}
      >{activeSwapping ? 'Opening…' : 'Open imported layout'}</Button>
    {/if}
  {/snippet}
</Modal>

<style>
  .modal-lead {
    margin-bottom: var(--space-12);
    color: var(--color-fg);
  }

  .modal-lead code {
    font-family: var(--font-mono);
    font-size: 0.95em;
    background: var(--color-surface-2);
    padding: 1px 4px;
    border-radius: var(--radius-sm);
  }

  .hint {
    margin-top: var(--space-12);
    font-size: var(--text-sm);
    color: var(--color-fg-muted);
  }

  .file-pick {
    display: inline-flex;
    align-items: center;
    gap: var(--space-8);
    padding: var(--space-8) var(--space-12);
    background: var(--color-surface-2);
    border: 1px dashed var(--color-border-strong);
    border-radius: var(--radius-md);
    cursor: pointer;
    transition: background var(--motion-fast) var(--motion-easing);
  }

  .file-pick:hover {
    background: var(--color-glass-1);
  }

  .file-pick input[type='file'] {
    display: none;
  }

  .file-pick-label {
    color: var(--color-fg);
    font-size: var(--text-md);
  }

  .section {
    display: grid;
    gap: var(--space-6);
    margin-bottom: var(--space-12);
  }

  .kv {
    display: grid;
    grid-template-columns: 110px 1fr;
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

  .v.mono,
  .text-input.mono {
    font-family: var(--font-mono);
    font-size: var(--text-base);
    color: var(--color-fg);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .counts {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--space-4) var(--space-12);
    margin-bottom: var(--space-12);
    padding: var(--space-8) var(--space-12);
    background: var(--color-surface-2);
    border-radius: var(--radius-md);
  }

  .count-row {
    display: flex;
    justify-content: space-between;
    font-size: var(--text-sm);
    color: var(--color-fg-muted);
  }

  .count-row .cn {
    font-family: var(--font-mono);
    color: var(--color-fg);
  }

  .field {
    display: grid;
    grid-template-columns: 110px 1fr;
    align-items: center;
    gap: var(--space-8);
  }

  .field .k {
    color: var(--color-fg-muted);
    text-transform: uppercase;
    font-size: 10px;
    letter-spacing: 0.5px;
  }

  .text-input {
    box-sizing: border-box;
    height: 32px;
    padding: 0 var(--space-12);
    background: var(--color-surface);
    color: var(--color-fg);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-md);
    width: 100%;
    font-family: inherit;
    font-size: var(--text-base);
    line-height: var(--leading-normal);
  }

  .text-input:focus {
    outline: 2px solid var(--color-accent);
    outline-offset: 0;
    border-color: var(--color-accent);
  }

  .text-input.has-error {
    border-color: var(--color-danger);
  }

  .error {
    margin-top: var(--space-4);
    font-size: var(--text-sm);
    color: var(--color-danger);
  }

  .caveat {
    margin-top: var(--space-8);
    padding: var(--space-8) var(--space-12);
    font-size: var(--text-sm);
    color: var(--color-fg-muted);
    background: var(--color-surface-2);
    border-left: 2px solid var(--color-warning);
    border-radius: var(--radius-sm);
  }
</style>
