// Toast store — append-only queue with auto-dismiss timers.
//
// Usage:
//   import { toastStore } from '$lib/ui/toast-store.svelte';
//   toastStore.show({ message: 'Layout out of sync — refreshed', tone: 'info' });
//
// Toast.svelte mounts a single host; module-level singleton keeps callers
// decoupled from rendering.

export type ToastTone = 'info' | 'success' | 'warning' | 'error';

export interface ToastItem {
  readonly id: number;
  readonly message: string;
  readonly tone: ToastTone;
}

class ToastStore {
  items = $state<ToastItem[]>([]);
  #nextId = 1;
  #timers = new Map<number, ReturnType<typeof setTimeout>>();

  show(opts: { message: string; tone?: ToastTone; durationMs?: number }): number {
    const id = this.#nextId++;
    const item: ToastItem = {
      id,
      message: opts.message,
      tone: opts.tone ?? 'info',
    };
    this.items = [...this.items, item];
    const duration = opts.durationMs ?? 4_000;
    if (duration > 0) {
      const timer = setTimeout(() => this.dismiss(id), duration);
      this.#timers.set(id, timer);
    }
    return id;
  }

  dismiss(id: number): void {
    const timer = this.#timers.get(id);
    if (timer !== undefined) {
      clearTimeout(timer);
      this.#timers.delete(id);
    }
    this.items = this.items.filter((it) => it.id !== id);
  }

  clear(): void {
    for (const timer of this.#timers.values()) clearTimeout(timer);
    this.#timers.clear();
    this.items = [];
  }
}

export const toastStore = new ToastStore();
