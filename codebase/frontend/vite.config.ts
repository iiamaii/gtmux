import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { fileURLToPath } from 'node:url';

// gtmux frontend Vite 설정. R8 §F7 manualChunks 정책 — xterm/svelteflow 분리.
// `$lib` alias 는 tsconfig.json `paths` 와 1:1 정합 — svelte-check 와 vite build 가
// 동일 module resolution 을 쓰도록 명시 (TS는 paths, Rollup은 resolve.alias 필요).
export default defineConfig({
  plugins: [svelte()],
  resolve: {
    alias: {
      $lib: fileURLToPath(new URL('./src/lib', import.meta.url))
    }
  },
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
