import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';

// Svelte 5 + vitePreprocess. SPA 모드 (SvelteKit adapter는 사용하지 않음 — vite plugin 단독).
export default {
  preprocess: vitePreprocess(),
  compilerOptions: {
    // Svelte 5 runes 모드 강제 적용.
    runes: true,
  },
};
