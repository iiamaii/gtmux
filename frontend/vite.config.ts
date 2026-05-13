import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

// gtmux frontend Vite 설정. R8 §F7 manualChunks 정책 — xterm/svelteflow 분리.
export default defineConfig({
  plugins: [svelte()],
  build: {
    target: 'es2022',
    minify: 'esbuild',
    cssCodeSplit: true,
    rollupOptions: {
      output: {
        manualChunks: {
          xterm: ['@xterm/xterm', '@xterm/addon-fit', '@xterm/addon-unicode11'],
          svelteflow: ['@xyflow/svelte'],
        },
      },
    },
  },
});
