// EphemeralStore — MT-3 라이브 갱신용 4종 (M / I / Viewport / FocusMode).
// R8 §F3.
import { SvelteSet } from 'svelte/reactivity';

class EphemeralStore {
  m = $state(new SvelteSet<string>());
  i = $state<string | null>(null);
  viewport = $state({ x: 0, y: 0, zoom: 1 });
  focusMode = $state<{ enabled: boolean; targetPanelId: string | null }>({
    enabled: false,
    targetPanelId: null,
  });
}

export const ephemeralStore = new EphemeralStore();
