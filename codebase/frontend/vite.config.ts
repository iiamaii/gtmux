import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { fileURLToPath } from 'node:url';

// gtmux frontend Vite 설정. R8 §F7 manualChunks 정책 — xterm/svelteflow 분리.
// `$lib` alias 는 tsconfig.json `paths` 와 1:1 정합 — svelte-check 와 vite build 가
// 동일 module resolution 을 쓰도록 명시 (TS는 paths, Rollup은 resolve.alias 필요).
export default defineConfig({
  // 게이트웨이 서브패스 배포용. GTMUX_BASE=/gtmux/ 로 빌드하면 asset/chunk 경로가
  // 해당 프리픽스로 나온다. 미설정 시 '/' — 기존 루트 배포와 동일.
  base: process.env.GTMUX_BASE || '/',
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
          markdown: ['marked', 'dompurify'],
        },
      },
    },
  },
});
