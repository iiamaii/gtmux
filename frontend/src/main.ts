import { mount } from 'svelte';
import Page from './routes/+page.svelte';

// SPA 진입점. SSR 없음 (routes/+layout.ts 의 ssr=false 정책과 정합).
const target = document.getElementById('app');
if (!target) {
  throw new Error('#app root element not found');
}

const app = mount(Page, { target });

export default app;
