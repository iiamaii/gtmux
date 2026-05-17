<script lang="ts">
  /**
   * Auth page — token / password 로그인.
   *
   * 정본:
   * - ADR-0020 D4 (token mode = URL query `?t=...` 자동 처리)
   * - ADR-0020 D5 (password mode = form)
   * - ADR-0020 D8 (rate limit 5/5min — 응답 `rate_limited` 처리)
   * - plan-0007 §13 BE-1 (auth handler) + plan-0007 §14.20 (UX rules)
   * - ref/frontend-design/auth.html (디자인 차용)
   *
   * 흐름:
   * 1. URL `?t=<token>` 있으면 자동 token mode 로 token 채움 + 자동 submit
   * 2. 사용자 form 제출 → `POST /auth/login` → Set-Cookie 자동
   * 3. 성공 시 `goto('/')` (root 가 AuthDialog 진입)
   * 4. invalid → form error 표시
   * 5. rate_limited → form 비활성 + countdown
   */

  import { onMount, untrack } from 'svelte';
  import { themeStore } from '$lib/stores/theme.svelte';
  import { login } from '$lib/http/auth';
  import brandLogoUrl from '$lib/assets/brand.png';

  // ADR-0020 D13 — 본 컴포넌트가 `/auth` page 의 *단일 source*. BE 는
  // SPA fallback (index.html) 만 응답 — main.ts 의 pickPage 가 `/auth` →
  // AuthPage mount. `/auth-preview` 는 디자인 demo alias (동일 컴포넌트).
  // 시안 ref/frontend-design/auth.html — 동일 디자인 + 실제 login 동작.
  //
  // Mode 는 *서버 config.auth.mode* 가 결정 — 클라이언트가 선택하지 않는다.
  // 두 mode UI 모두 노출 — 활성 모드의 credential 만 BE 에 보냄. 실제 submit
  // 은 활성 모드의 credential 만 BE 에 보낸다.
  type LocalMode = 'token' | 'password';
  let mode = $state<LocalMode>('token');
  let tokenValue = $state('');
  let passwordValue = $state('');
  let showSecret = $state(false);

  let submitting = $state(false);
  let errorMessage = $state<string | null>(null);
  let retryAfterSec = $state<number | null>(null);
  let countdownInterval: ReturnType<typeof setInterval> | null = null;

  /** Form 비활성 — rate-limit 중이거나 in-flight 요청 중. */
  let disabled = $derived(submitting || retryAfterSec !== null);

  onMount(() => {
    themeStore.apply();
    // ADR-0020 D4 — URL `?t=<token>` 자동 처리. 한 번만 실행.
    try {
      const params = new URLSearchParams(window.location.search);
      const t = params.get('t');
      if (t && t.length > 0) {
        untrack(() => {
          mode = 'token';
          tokenValue = t;
        });
        // Token 채운 직후 자동 submit — 사용자 click 없이 통과 (Magic link UX).
        void submit();
      }
    } catch (e) {
      console.debug('[gtmux] auth query read failed', e);
    }
    return () => {
      if (countdownInterval !== null) clearInterval(countdownInterval);
    };
  });

  function selectMode(next: LocalMode): void {
    mode = next;
    errorMessage = null;
    // 다음 mode 의 input 으로 focus 이동
    queueMicrotask(() => {
      const sel = next === 'token' ? '#token-input' : '#password-input';
      document.querySelector<HTMLInputElement>(sel)?.focus();
    });
  }

  function startCountdown(seconds: number): void {
    retryAfterSec = seconds;
    if (countdownInterval !== null) clearInterval(countdownInterval);
    countdownInterval = setInterval(() => {
      if (retryAfterSec === null) {
        if (countdownInterval !== null) clearInterval(countdownInterval);
        return;
      }
      if (retryAfterSec <= 1) {
        retryAfterSec = null;
        errorMessage = null;
        if (countdownInterval !== null) {
          clearInterval(countdownInterval);
          countdownInterval = null;
        }
      } else {
        retryAfterSec = retryAfterSec - 1;
      }
    }, 1000);
  }

  async function submit(e?: Event): Promise<void> {
    e?.preventDefault();
    if (disabled) return;
    errorMessage = null;
    const value = mode === 'token' ? tokenValue.trim() : passwordValue;
    if (mode === 'token' && value.length < 8) {
      errorMessage = 'Token looks too short. Check and paste again.';
      return;
    }
    if (mode === 'password' && value.length < 6) {
      errorMessage = 'Password must be at least 6 characters.';
      return;
    }
    submitting = true;
    try {
      const req =
        mode === 'token'
          ? { token: value, redirect: '/' }
          : { password: value, redirect: '/' };
      const res = await login(req);
      if (res.kind === 'ok') {
        // Vite+Svelte SPA (no SvelteKit) — 전체 페이지 reload 로 root mount.
        // BE 가 sanitize 한 redirect 를 사용 (open-redirect 방지).
        window.location.href = res.redirect || '/';
        return;
      }
      if (res.kind === 'rate_limited') {
        errorMessage = 'Too many attempts. Please wait before trying again.';
        startCountdown(res.retry_after_secs);
        return;
      }
      if (res.kind === 'unavailable') {
        errorMessage =
          res.message ??
          'Password mode is configured but no password is set on the server.';
        return;
      }
      // invalid / bad_request
      errorMessage =
        res.message ?? 'Authentication failed. Check your credentials.';
    } catch (err) {
      errorMessage = `Network error: ${err instanceof Error ? err.message : String(err)}`;
    } finally {
      submitting = false;
    }
  }
</script>

<header class="topbar">
  <div class="topbar-actions">
    <button
      class="icon-btn"
      type="button"
      aria-label="Toggle theme"
      title="Toggle theme"
      onclick={() => themeStore.toggle()}
    >
      <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <path d="M13.5 9.5A5.5 5.5 0 016.5 2.5a5.5 5.5 0 107 7z" />
      </svg>
    </button>
  </div>
</header>

<main>
  <section class="card" aria-labelledby="auth-title">
    <div class="brand-header">
      <img class="brand-mark" src={brandLogoUrl} alt="" aria-hidden="true" />
      <h1 class="heading" id="auth-title">gtmux</h1>
    </div>
    <p class="deck">
      Authenticate with a token or password. Your session persists on this device.
    </p>

    <div class="tabs" role="tablist" aria-label="Authentication method">
      <button
        type="button"
        role="tab"
        class="tab"
        aria-selected={mode === 'token'}
        onclick={() => selectMode('token')}
      >
        Token
      </button>
      <button
        type="button"
        role="tab"
        class="tab"
        aria-selected={mode === 'password'}
        onclick={() => selectMode('password')}
      >
        Password
      </button>
    </div>

    {#if mode === 'token'}
      <div class="panel" role="tabpanel">
        <form onsubmit={submit} autocomplete="off">
          <div class="field">
            <div class="label">
              <span>Access token</span>
              <span class="hint">Secret</span>
            </div>
            <div class="input-wrap">
              <!-- svelte-ignore a11y_autofocus -->
              <input
                id="token-input"
                class="input mono has-toggle"
                type={showSecret ? 'text' : 'password'}
                bind:value={tokenValue}
                placeholder="Paste your token"
                required
                minlength="8"
                autofocus
                {disabled}
              />
              <button
                type="button"
                class="toggle-eye"
                aria-label={showSecret ? 'Hide token' : 'Show token'}
                onclick={() => (showSecret = !showSecret)}
              >
                <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                  <path d="M1 8s2.5-5 7-5 7 5 7 5-2.5 5-7 5-7-5-7-5z" />
                  <circle cx="8" cy="8" r="2" />
                </svg>
              </button>
            </div>
          </div>
          <button type="submit" class="submit" {disabled}>
            {#if submitting}Authenticating…{:else if retryAfterSec !== null}Retry in {retryAfterSec}s{:else}Continue{/if}
          </button>
        </form>
      </div>
    {:else}
      <div class="panel" role="tabpanel">
        <form onsubmit={submit} autocomplete="off">
          <div class="field">
            <div class="label">
              <span>Password</span>
            </div>
            <div class="input-wrap">
              <input
                id="password-input"
                class="input has-toggle"
                type={showSecret ? 'text' : 'password'}
                bind:value={passwordValue}
                placeholder="Enter password"
                required
                minlength="6"
                autocomplete="current-password"
                {disabled}
              />
              <button
                type="button"
                class="toggle-eye"
                aria-label={showSecret ? 'Hide password' : 'Show password'}
                onclick={() => (showSecret = !showSecret)}
              >
                <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                  <path d="M1 8s2.5-5 7-5 7 5 7 5-2.5 5-7 5-7-5-7-5z" />
                  <circle cx="8" cy="8" r="2" />
                </svg>
              </button>
            </div>
          </div>
          <button type="submit" class="submit" {disabled}>
            {#if submitting}Authenticating…{:else if retryAfterSec !== null}Retry in {retryAfterSec}s{:else}Continue{/if}
          </button>
        </form>
      </div>
    {/if}

    {#if errorMessage !== null}
      <div class="error" role="alert">{errorMessage}</div>
    {/if}
  </section>
</main>

<div class="page-foot">gtmux · multi-session workspace</div>

<style>
  /* Auth-page-local design — Figma-ish card on full-screen surface.
   * Tokens come from the global tokens.css (ADR-0016 + 2026-05-15 amend).
   */

  .topbar {
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    height: 56px;
    display: flex;
    align-items: center;
    justify-content: flex-end;
    padding: 0 var(--space-24);
    z-index: 10;
  }

  .topbar-actions {
    display: flex;
    align-items: center;
    gap: var(--space-4);
  }

  .brand-header {
    display: flex;
    flex-direction: row;
    align-items: center;
    justify-content: center;
    gap: var(--space-10);
    margin-bottom: var(--space-6);
  }

  .brand-mark {
    width: 56px;
    height: 56px;
    border-radius: var(--radius-md);
    object-fit: cover;
    display: block;
  }

  .icon-btn {
    width: 32px;
    height: 32px;
    border-radius: 50%;
    color: var(--color-fg);
    display: grid;
    place-items: center;
    transition: background var(--motion-fast) var(--motion-easing);
  }

  .icon-btn:hover {
    background: var(--color-glass-1);
  }

  .icon-btn:focus-visible {
    outline: 2px dashed var(--color-accent);
    outline-offset: 1px;
  }

  main {
    display: grid;
    place-items: center;
    min-height: 100vh;
    padding: 88px var(--space-24) 48px;
  }

  .card {
    width: 100%;
    max-width: 420px;
    background: var(--color-surface);
    border-radius: 16px;
    box-shadow: var(--shadow-lg);
    padding: 40px 36px 32px;
  }

  .heading {
    font-size: 40px;
    font-weight: var(--weight-semibold);
    letter-spacing: -0.8px;
    line-height: 1.1;
    margin: 0;
    text-align: center;
  }

  .deck {
    font-size: 14px;
    font-weight: 330;
    color: var(--color-fg-muted);
    letter-spacing: -0.1px;
    margin: 0 0 28px;
    line-height: 1.45;
    text-align: center;
  }

  .tabs {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--space-4);
    padding: var(--space-4);
    background: var(--color-surface-2);
    border-radius: var(--radius-pill);
    margin-bottom: var(--space-24);
  }

  .tab {
    height: 32px;
    border-radius: var(--radius-pill);
    color: var(--color-fg-muted);
    font-size: 13px;
    font-weight: 480;
    letter-spacing: -0.1px;
    display: grid;
    place-items: center;
    transition:
      background var(--motion-fast) var(--motion-easing),
      color var(--motion-fast) var(--motion-easing);
  }

  .tab[aria-selected='true'] {
    background: var(--color-surface);
    color: var(--color-fg);
    box-shadow: var(--shadow-sm);
  }

  :global(:root.dark) .tab[aria-selected='true'] {
    background: var(--color-surface-2);
  }

  .tab:focus-visible {
    outline: 2px dashed var(--color-accent);
    outline-offset: 2px;
  }

  .panel {
    display: block;
  }

  .field {
    margin-bottom: var(--space-16);
  }

  .label {
    display: flex;
    align-items: center;
    justify-content: space-between;
    font-size: var(--text-md);
    font-weight: 480;
    color: var(--color-fg);
    margin-bottom: var(--space-6);
  }

  .label .hint {
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    letter-spacing: 0.4px;
    text-transform: uppercase;
    color: var(--color-fg-subtle);
  }

  .input-wrap {
    position: relative;
  }

  .input {
    width: 100%;
    height: 44px;
    padding: 0 14px;
    background: var(--color-bg);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    font-family: inherit;
    font-size: 14px;
    letter-spacing: -0.1px;
    color: var(--color-fg);
    transition:
      border-color var(--motion-fast) var(--motion-easing),
      box-shadow var(--motion-fast) var(--motion-easing);
  }

  .input.mono {
    font-family: var(--font-mono);
    font-size: 13px;
    letter-spacing: 0;
  }

  .input:focus {
    outline: none;
    border-color: var(--color-border-strong);
    box-shadow: 0 0 0 3px rgba(13, 153, 255, 0.15);
  }

  .input::placeholder {
    color: var(--color-fg-subtle);
  }

  .input.has-toggle {
    padding-right: 44px;
  }

  .input:disabled {
    opacity: 0.55;
    cursor: not-allowed;
  }

  .toggle-eye {
    position: absolute;
    right: 6px;
    top: 50%;
    transform: translateY(-50%);
    width: 32px;
    height: 32px;
    border-radius: 50%;
    color: var(--color-fg-muted);
    display: grid;
    place-items: center;
  }

  .toggle-eye:hover {
    background: var(--color-glass-1);
    color: var(--color-fg);
  }

  .toggle-eye:focus-visible {
    outline: 2px dashed var(--color-accent);
    outline-offset: 1px;
  }

  .submit {
    width: 100%;
    height: 44px;
    border-radius: var(--radius-pill);
    background: var(--color-fg);
    color: var(--color-bg);
    font-family: inherit;
    font-size: 14px;
    font-weight: 480;
    letter-spacing: -0.1px;
    transition:
      opacity var(--motion-fast) var(--motion-easing),
      transform 40ms var(--motion-easing);
  }

  .submit:hover:not(:disabled) {
    opacity: 0.92;
  }

  .submit:active:not(:disabled) {
    transform: scale(0.99);
  }

  .submit:disabled {
    opacity: 0.55;
    cursor: not-allowed;
  }

  .submit:focus-visible {
    outline: 2px dashed var(--color-accent);
    outline-offset: 2px;
  }

  .error {
    margin-top: 14px;
    padding: 10px 12px;
    border-radius: var(--radius-md);
    background: color-mix(in srgb, var(--color-danger) 12%, transparent);
    color: var(--color-danger);
    font-size: 12.5px;
    line-height: 1.4;
  }

  .page-foot {
    position: fixed;
    bottom: 20px;
    left: 0;
    right: 0;
    text-align: center;
    font-family: var(--font-mono);
    font-size: var(--text-base);
    letter-spacing: 0.5px;
    text-transform: uppercase;
    color: var(--color-fg-subtle);
  }
</style>
