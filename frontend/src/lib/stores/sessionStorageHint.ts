// sessionStorage hint — last-attached session name (tab-scoped).
//
// 정본:
// - ADR-0019 D5.4 (Initial entry attach recovery — blocking ReconnectModal)
// - plan-0008 §4.5
//
// Semantics:
// - tab-scoped (`sessionStorage`, not `localStorage`) — multi-tab 충돌 방지.
//   탭 A=session-X / 탭 B=session-Y 가 둘 다 reload 해도 각자 자기 hint 만 본다.
// - Write 시점: attach 성공 (`sessionStore.setActiveSession`) + layout reload.
// - Clear 시점: 명시 detach / logout / [Switch session…] / session [Delete] +
//   attemptReattach 의 404 분기.
// - Read 시점: AppPage onMount 의 hint 검사 분기.
//
// SSR / private-mode safety:
// - Vite SSR 단계에서는 `sessionStorage` 가 없음 → typeof check.
// - Safari private-mode 등은 setItem 시 QuotaExceeded throw 가능 → try-catch.
// - 모든 실패는 best-effort silent fallback — hint 없음 흐름으로 자연 degrade.

const KEY = 'gtmux-last-active-session';

function safeSessionStorage(): Storage | null {
  try {
    if (typeof window === 'undefined') return null;
    return window.sessionStorage;
  } catch {
    return null;
  }
}

export const sessionStorageHint = {
  get(): string | null {
    const store = safeSessionStorage();
    if (store === null) return null;
    try {
      const v = store.getItem(KEY);
      return v !== null && v.length > 0 ? v : null;
    } catch {
      return null;
    }
  },
  set(name: string): void {
    const store = safeSessionStorage();
    if (store === null) return;
    try {
      store.setItem(KEY, name);
    } catch {
      // best-effort — private mode / quota
    }
  },
  clear(): void {
    const store = safeSessionStorage();
    if (store === null) return;
    try {
      store.removeItem(KEY);
    } catch {
      // best-effort
    }
  },
};
