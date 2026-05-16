import './styles/global.css';
import { mount } from 'svelte';
import AppPage from './routes/+page.svelte';
import AuthPage from './routes/auth/+page.svelte';

// SPA 진입점. SSR 없음 (routes/+layout.ts 의 ssr=false 정책과 정합).
//
// 라우팅 (SvelteKit 미사용 — vite + svelte 단독):
//   - `/`, `/api/*` 등 BE 가 routing — SPA fallback 으로 본 bundle 도달.
//   - `/auth` — ADR-0020 D13: BE 의 server-rendered handler 폐기 후 SPA
//     fallback (index.html) 으로 진입. 본 분기가 AuthPage 를 mount.
//   - `/auth-preview` — 디자인 데모 alias (동일 AuthPage). 시안
//     ref/frontend-design/auth.html 를 직접 비교할 때 사용.
//
// BE land (plan-0009 §2) 가 안 된 상태에서는 `/auth` GET 이 여전히 BE-rendered
// HTML 을 반환하므로 본 분기가 *효과 없음*. BE land 후 자동 활성.
const target = document.getElementById('app');
if (!target) {
  throw new Error('#app root element not found');
}

function pickPage(pathname: string): typeof AppPage {
  if (
    pathname === '/auth' ||
    pathname.startsWith('/auth/') ||
    pathname === '/auth-preview' ||
    pathname.startsWith('/auth-preview/')
  ) {
    return AuthPage;
  }
  return AppPage;
}

const app = mount(pickPage(window.location.pathname), { target });

export default app;
