<script lang="ts">
  // ReconnectBanner — D21 c2 (grace 1s) + c3 (zombie badge) + close code 분기.
  //
  // 동작:
  // 1. WS가 끊긴(state ∈ {closing, closed, reconnecting}) 직후 1초는 banner 미표시
  //    — transient disconnect(예: WiFi 순간 끊김, 재연결 < 1s)를 사용자에게 노출하지 않음.
  // 2. 1초 경과해도 끊겨 있으면 close code별 메시지를 sticky 배너로 표시.
  // 3. state가 'open'으로 복귀하면 connectionStore.setState가 disconnectedAt을
  //    null로 리셋하므로 derived `show`가 즉시 false → 배너 사라짐.
  //
  // close code 분기 (wire-protocol.md §3 + ADR-0002 D5 + D21 auto-decide):
  //   1000 normal     → 배너 미표시 (의도적 종료, sketch §7.4 정합)
  //   1008 policy     → "Server rejected this connection." (recoverable 아님)
  //   1011 internal   → "Server error. Reconnecting..." + attempt 표시
  //   4001 token rev  → "Authentication revoked." + re-auth 링크
  //   기타            → "Connection lost. Reconnecting..." (default 재연결 안내)
  //
  // ARIA: role="status" + aria-live="polite"로 스크린리더에 비공격적 안내.
  //       grace 1s 덕분에 transient flicker로 인한 announcement 폭주 없음.

  import { connectionStore } from '$lib/stores/connection.svelte';

  // 0077 follow-up — grace 1000ms → 300ms. 사용자 보고: action 막힌 webpage
  // (pre-session 또는 disconnect 직후) 에서 *왜* 막혀있는지 인지 못 함.
  // grace 단축으로 *banner 가 즉시 surface*. transient flicker (예: WiFi 0.x s
  // 끊김) 시 보일 수 있으나 user-perceived "blank action" 의 cost 보다 작음.
  const GRACE_MS = 300;
  const TICK_MS = 150;

  let elapsedMs = $state(0);

  // disconnectedAt이 set되면 250ms tick으로 elapsedMs를 갱신해
  // grace 1s 경계에 정확히 reactive. open 복귀(null) 시 tick 즉시 해제.
  $effect(() => {
    const t0 = connectionStore.disconnectedAt;
    if (t0 === null) {
      elapsedMs = 0;
      return;
    }
    elapsedMs = performance.now() - t0;
    const handle = setInterval(() => {
      elapsedMs = performance.now() - t0;
    }, TICK_MS);
    return () => {
      clearInterval(handle);
    };
  });

  type Tone = 'warn' | 'error';
  type Message = { tone: Tone; text: string; action?: 'reauth' };

  const show = $derived(
    connectionStore.disconnectedAt !== null && elapsedMs >= GRACE_MS,
  );

  const message: Message | null = $derived.by(() => {
    if (!show) return null;
    const code = connectionStore.closeCode;
    const attempt = connectionStore.attempt;
    switch (code) {
      case 1000:
        // 정상 종료. 두 시나리오:
        //   1) `wsClient.stop()` 호출 (페이지 unmount) — 사용자 노출 무의미
        //   2) backend graceful_shutdown (Session shutdown 액션의 결과)
        //      — 사용자에게 "ended" 알림 + 재진입 안내 표시 (ADR-0017 §D3
        //      step 6). 두 시나리오를 구분할 connection-level 정보가 없으므로
        //      *항상 표시* 하되 reason 으로 분기 — 정상 종료 사유는 보통
        //      비어 있거나 "client-stop". backend 가 보낸 1000 은 reason 빈
        //      문자열로 도착하므로 그쪽만 banner.
        if (
          connectionStore.closeReason &&
          connectionStore.closeReason.length > 0 &&
          connectionStore.closeReason.startsWith('client-stop')
        ) {
          return null;
        }
        return {
          tone: 'warn',
          text: 'Session ended. Re-run `gtmux start --session <name>` to reconnect.',
        };
      case 1008:
        return {
          tone: 'error',
          text: 'Server rejected this connection. Refresh the page and reconnect.',
        };
      case 1011:
        return {
          tone: 'warn',
          text:
            attempt > 0
              ? `Server error. Reconnecting (attempt ${attempt})...`
              : 'Server error. Reconnecting...',
        };
      case 4001:
        return {
          tone: 'error',
          text: 'Authentication revoked.',
          action: 'reauth',
        };
      default:
        return {
          tone: 'warn',
          text:
            attempt > 0
              ? `Connection lost. Reconnecting (attempt ${attempt})...`
              : 'Connection lost. Reconnecting...',
        };
    }
  });

  const hasZombie = $derived(connectionStore.zombiePaneIds.length > 0);
  const zombieCount = $derived(connectionStore.zombiePaneIds.length);
</script>

{#if show && message}
  <div
    class="reconnect-banner"
    data-tone={message.tone}
    role="status"
    aria-live="polite"
  >
    <span class="text">{message.text}</span>
    {#if message.action === 'reauth'}
      <a class="action" href="/auth/bootstrap">Re-authenticate</a>
    {/if}
    {#if hasZombie}
      <span
        class="zombie-badge"
        title="One or more panes have exited (pane_dead=1)"
        aria-label={`${zombieCount} zombie pane${zombieCount === 1 ? '' : 's'}`}
      >
        {zombieCount} zombie
      </span>
    {/if}
  </div>
{/if}

<style>
  .reconnect-banner {
    position: sticky;
    top: 0;
    z-index: var(--z-banner);
    padding: var(--space-8) var(--space-16);
    display: flex;
    align-items: center;
    gap: var(--space-12);
    background: var(--banner-warn-bg);
    color: var(--banner-warn-fg);
    border-bottom: 1px solid var(--banner-warn-border);
    font-size: 14px;
    line-height: 1.4;
  }

  .reconnect-banner[data-tone='error'] {
    background: var(--banner-error-bg);
    color: var(--banner-error-fg);
    border-bottom-color: var(--banner-error-border);
  }

  .text {
    flex: 0 1 auto;
  }

  .action {
    color: inherit;
    text-decoration: underline;
    font-weight: 600;
  }

  .action:hover,
  .action:focus-visible {
    text-decoration: none;
    outline: 2px solid currentColor;
    outline-offset: 2px;
  }

  .zombie-badge {
    margin-left: auto;
    padding: 2px var(--space-8);
    border-radius: 12px;
    background: var(--zombie-bg);
    color: var(--zombie-fg);
    font-size: 12px;
    font-weight: 600;
    white-space: nowrap;
  }
</style>
