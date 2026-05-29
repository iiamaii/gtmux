import { defineConfig } from 'vitest/config';
import { fileURLToPath } from 'node:url';

// plan-0017 Phase 0 — FE 단위 테스트 하네스. vite.config.ts(프로덕션 빌드)와
// 분리해 build/check 파이프라인을 건드리지 않는다. `$lib` alias 는 tsconfig
// paths 와 1:1 정합(vite.config.ts 와 동일 규칙). 현재 테스트는 순수 TS 모듈만
// 대상이라 svelte 플러그인/jsdom 불요 — node 환경. component test 가 필요해지면
// 그때 @testing-library/svelte + jsdom 추가.
export default defineConfig({
  resolve: {
    alias: {
      $lib: fileURLToPath(new URL('./src/lib', import.meta.url)),
    },
  },
  test: {
    environment: 'node',
    include: ['src/**/*.{test,spec}.ts'],
  },
});
