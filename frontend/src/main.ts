import './styles/global.css';
import { mount } from 'svelte';
import AppPage from './routes/+page.svelte';
import AuthPage from './routes/auth/+page.svelte';

// SPA 진입점. SSR 없음 (routes/+layout.ts 의 ssr=false 정책과 정합).
//
// 라우팅 (SvelteKit 미사용 — vite + svelte 단독):
//   - `/`, `/api/*` 등 BE 가 routing — SPA fallback 으로 본 bundle 도달.
//   - `/auth` 는 BE 가 server-rendered HTML 로 직접 처리 (auth.rs:408,
//     JS bundle 무관 의도). SPA 가 거기서 실행되지 않음.
//   - `/auth-preview` — ref/frontend-design/auth.html 디자인 데모용.
const target = document.getElementById('app');
if (!target) {
  throw new Error('#app root element not found');
}

function pickPage(pathname: string): typeof AppPage {
  if (pathname === '/auth-preview' || pathname.startsWith('/auth-preview/')) {
    return AuthPage;
  }
  return AppPage;
}

const app = mount(pickPage(window.location.pathname), { target });

export default app;
