<script lang="ts">
  import { terminalActivity } from '$lib/stores/terminalActivity.svelte';
  import { DEFAULT_ACTIVITY_PREFERENCES, ACTIVITY_FORMAT_MAX_LENGTH, validateActivityFormat, formatActivityTitle,
    type ActivityToggle, type ActivityFormat } from '$lib/stores/terminalActivity';
  import SettingsSwitch from './SettingsSwitch.svelte';
  import { tabTitleStore } from '$lib/stores/tabTitle.svelte';
  import { formatTabTitle } from '$lib/stores/tabTitle';
  import { sessionStore } from '$lib/stores/sessionStore.svelte';
  const formats: { key: ActivityFormat; label: string; help: string }[] = [
    { key: 'titleFormat', label: 'Activity title format', help: 'Use {title} for your browser title and {activity} for pending states.' },
    { key: 'completedFormat', label: 'Completion label', help: 'Use {count} for the number of completed terminals.' },
    { key: 'inputFormat', label: 'Input-needed label', help: 'Use {count} for the number of terminals needing input.' },
    { key: 'unreadFormat', label: 'Unread label', help: 'Use {count} for the number of unread terminals.' },
  ];
  let drafts = $state({ titleFormat: '', completedFormat: '', inputFormat: '', unreadFormat: '' });
  let errors = $state<Partial<Record<ActivityFormat, string | null>>>({});
  $effect(() => {
    const p = terminalActivity.preferences;
    drafts = { titleFormat: p.titleFormat, completedFormat: p.completedFormat, inputFormat: p.inputFormat, unreadFormat: p.unreadFormat };
    errors = {};
  });
  const formatDisabled = $derived(!terminalActivity.preferences.enabled || !terminalActivity.preferences.tab);
  const preview = $derived(formatActivityTitle(
    formatTabTitle(sessionStore.active?.name ?? 'my-session', tabTitleStore.preferences),
    { waiting: terminalActivity.preferences.needs_input ? 1 : 0, done: terminalActivity.preferences.completed ? 2 : 0,
      unread: terminalActivity.preferences.unread ? 3 : 0 },
    { ...terminalActivity.preferences, ...drafts },
  ));
  function saveFormat(key: ActivityFormat): void {
    errors[key] = validateActivityFormat(key, drafts[key]);
    if (!errors[key]) terminalActivity.update({ [key]: drafts[key] });
  }
  function resetFormats(): void {
    const { titleFormat, completedFormat, inputFormat, unreadFormat } = DEFAULT_ACTIVITY_PREFERENCES;
    terminalActivity.update({ titleFormat, completedFormat, inputFormat, unreadFormat });
  }
  const options: { key: ActivityToggle; label: string; description: string }[] = [
    { key: 'enabled', label: 'Terminal activity', description: 'Monitor terminal activity, response completion and input prompts. Off by default.' },
    { key: 'completed', label: 'Response completion', description: 'Show reported completion and estimated ready prompts.' },
    { key: 'needs_input', label: 'Input needed', description: 'Show reported input waits and estimated confirmation prompts.' },
    { key: 'unread', label: 'Unread output', description: 'Track output you have not acknowledged in this browser tab.' },
    { key: 'list', label: 'Activity list', description: 'Add Activity beside Layers, Terminals and Files.' },
    { key: 'tabUnread', label: 'Unread in browser tab', description: 'Include unread counts in the tab title. Off by default; the Activity list keeps its own unread indicators.' },
    { key: 'tab', label: 'Browser tab indicators', description: 'Add pending counts for this session to its browser tab title.' },
  ];
  function toggle(key: ActivityToggle, input: HTMLInputElement): void {
    if (!terminalActivity.update({ [key]: input.checked })) input.checked = terminalActivity.preferences[key];
  }
</script>
<section aria-labelledby="activity-settings-heading">
  <h4 id="activity-settings-heading">Terminal activity</h4>
  <p class="hint">Preferences apply to this browser and sync across tabs. Read acknowledgements stay in each tab. Focusing a visible terminal marks it read. Silence alone is shown as “Output quiet”.</p>
  {#each options as option (option.key)}
    <label class="row">
      <span><span class="label">{option.label}</span><span class="hint">{option.description}</span></span>
      <SettingsSwitch checked={terminalActivity.preferences[option.key]}
        disabled={(option.key !== 'enabled' && !terminalActivity.preferences.enabled) || (option.key === 'tabUnread' && (!terminalActivity.preferences.tab || !terminalActivity.preferences.unread))}
        onchange={(event) => toggle(option.key, event.currentTarget)} />
    </label>
  {/each}
  <p class="hint">Hidden the Activity list or tab indicators? Return to Settings → Appearance → Terminal activity to turn them back on. Your formats and sidebar widths are kept.</p>
  <div class="formats">
    {#each formats as format (format.key)}
      <div class="format-row">
        <label class="label" for={`activity-${format.key}`}>{format.label}</label>
        <span class="hint" id={`activity-${format.key}-help`}>{format.help} Saves on Enter or when leaving the field.</span>
        <input class="format-input" id={`activity-${format.key}`} type="text" bind:value={drafts[format.key]}
          disabled={formatDisabled} maxlength={ACTIVITY_FORMAT_MAX_LENGTH} spellcheck="false"
          aria-invalid={!!errors[format.key]} aria-describedby={`activity-${format.key}-help activity-${format.key}-error`}
          onchange={() => saveFormat(format.key)}
          onkeydown={(event) => { if (event.key === 'Enter') { event.preventDefault(); saveFormat(format.key); } }} />
        <span class="error" id={`activity-${format.key}-error`} aria-live="polite">{errors[format.key] ?? ''}</span>
      </div>
    {/each}
    <p class="hint preview" aria-live="polite">Example (pending counts): <span>{preview}</span></p>
    <button class="reset" type="button" disabled={formatDisabled} onclick={resetFormats}>Reset activity formats</button>
  </div>
  {#if terminalActivity.saveError}<p role="alert" class="error">{terminalActivity.saveError}</p>{/if}
</section>
<style>
  h4 { color: var(--color-fg-muted); font: var(--text-xs) var(--font-mono); text-transform: uppercase; letter-spacing: .6px; padding-top: 26px; font-weight: normal; }
  .row { display: flex; gap: 28px; justify-content: space-between; align-items: center; padding: 14px 0; border-bottom: 1px solid var(--color-border); }
  .label { display: block; font-size: var(--text-md); font-weight: var(--weight-medium); color: var(--color-fg); }
  .hint { display: block; color: var(--color-fg-muted); font-size: var(--text-base); line-height: 1.5; max-width: 60ch; margin-top: 3px; }
  .formats { padding: 14px 0; }
  .format-row { display: grid; gap: 5px; margin-bottom: 14px; }
  .format-input { box-sizing: border-box; width: 100%; height: 32px; padding: 0 10px;
    border: 1px solid var(--color-border); border-radius: var(--radius-md); background: var(--color-surface);
    color: var(--color-fg); font: inherit; font-size: var(--text-base); }
  .format-input:hover:not(:disabled) { border-color: var(--color-border-strong); }
  .format-input[aria-invalid='true'] { border-color: var(--color-danger); }
  .format-input:focus-visible, .reset:focus-visible { outline: 2px solid var(--color-info); outline-offset: 2px; }
  .format-input:disabled, .reset:disabled { opacity: .5; cursor: not-allowed; }
  .preview { overflow-wrap: anywhere; }
  .preview span { color: var(--color-fg); }
  .reset { height: 32px; padding: 0 12px; border: 1px solid var(--color-border); border-radius: var(--radius-md);
    color: var(--color-fg); background: var(--color-surface-2); font: inherit; font-size: var(--text-base); cursor: pointer; }
  .reset:hover:not(:disabled) { border-color: var(--color-border-strong); }
  .error { color: var(--color-danger); font-size: var(--text-base); }
</style>
