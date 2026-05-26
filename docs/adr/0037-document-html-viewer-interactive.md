# ADR-0037: Document HTML viewer — sandboxed rendered mode

- 상태: **Accepted** (2026-05-21 신규, 2026-05-26 대체 amend)
- 관련 ADR: ADR-0018 (D10 amend ③/④/⑤/⑥/⑨), ADR-0030, ADR-0033
- 관련 plan: `docs/plans/0011-component-design-batch-caption-document.md`
- 작성자: agent

## 맥락

Document item 은 markdown/html/pdf/source 렌더링을 지원한다. 2026-05-21 에는 HTML 의 script 실행, 자체 styling, link click 을 보존하기 위해 `interactive` mode 를 별도 mode 로 추가했다. 이후 2026-05-26 사용자 재현 `eggroll_visual_summary.html` 로 다음 설계 gap 이 확정됐다.

- 문제 HTML 은 fragment 가 아니라 standalone HTML 이며 `:root`, `body`, universal selector, `.shell`, `.toc` 등 전역 CSS 를 포함한다.
- 기존 rendered mode 가 HTML 을 parent DOM 에 `{@html}` 로 직접 mount 하면 DOMPurify 로 XSS 는 줄일 수 있어도 CSS scope 오염은 막을 수 없다.
- 그 결과 asset fetch 완료 후 gtmux chrome/canvas 에 외부 CSS 가 적용되어 문서가 "보이다가 갑자기 사라지는" 회귀처럼 보였다.
- `sandbox=""` iframe 은 CSS scope 는 해결하지만 MathJax 같은 script-rendered 수식이 깨진다.
- `interactive` mode 는 link/popup/height-probe UX 가 독립 제품 수준의 정책을 요구했고, 현재 사용자가 기대한 "문서 보기"와 달리 새 탭이 열리거나 빈 화면처럼 보이는 혼란을 만들었다.

따라서 본 ADR 의 최종 정책은 **HTML rendered mode 를 sandbox iframe 으로 격리하고, 별도 interactive mode 는 제거**하는 것이다.

## 결정

### D1. viewMode 는 `rendered | source` 두 상태만 유지

`DocumentViewMode` 는 다음 두 값만 가진다.

```typescript
export type DocumentViewMode = 'rendered' | 'source';
```

| fileType | 사용 가능 mode | toggle UI |
|---|---|---|
| `markdown` | `rendered ↔ source` | binary toggle |
| `html` | `rendered ↔ source` | binary toggle |
| 그 외 | toggle 없음 | hidden |

`getNextViewMode(current, fileType)` 와 `getNextViewModeLabel(current, fileType)` 는 DocumentNode 와 MaximizedItemModal 의 단일 전이 helper 로 유지한다. `fileType` 인자는 call-site API 안정성을 위해 남기지만 HTML 3-mode 분기는 제거한다.

### D2. HTML rendered mode 는 `iframe srcdoc sandbox="allow-scripts"`

HTML file 의 rendered mode 는 parent DOM 에 직접 mount 하지 않는다.

```html
<iframe
  sandbox="allow-scripts"
  referrerpolicy="no-referrer"
  loading="lazy"
  srcdoc={raw}
></iframe>
```

채택한 sandbox flag:

| flag | 채택 여부 | 이유 |
|---|:-:|---|
| `allow-scripts` | ✓ | MathJax 같은 static renderer 허용 |
| `allow-same-origin` | ✗ | parent storage/cookie/DOM 접근 차단 |
| `allow-popups` | ✗ | rendered mode 에서 새 탭/팝업 UX 비활성 |
| `allow-top-navigation` | ✗ | parent route 변경 차단 |
| `allow-forms` | ✗ | form submit/navigation 차단 |

이 정책은 standalone HTML 의 CSS 와 script execution 을 iframe 의 unique opaque origin 안에 가둔다. CSS 는 iframe 내부에만 적용되므로 parent app chrome 을 오염시키지 않는다.

### D3. rendered HTML 에 `<base target="_blank">` 를 주입하지 않는다

2026-05-26 최종 amend 로 `buildRenderedHtmlSrcdoc(raw)` 는 raw HTML 을 그대로 반환한다.

이유:

- 문서 내부 `href="#section"` routing 은 iframe 내부 위치 이동으로 동작해야 한다.
- `<base target="_blank">` 를 주입하면 hash link 도 새 browsing context 로 빠질 수 있어 문서 내 TOC/anchor UX 를 깨뜨린다.
- rendered iframe 은 `allow-popups` 가 없으므로 외부 링크 새 탭 동작을 보장하지 않는다. rendered mode 의 link 목적은 parent app route 를 바꾸지 않는 것과 내부 anchor 보존이다.

외부 link navigation 을 완전한 browser-like UX 로 제공하려면 rendered mode 에 끼워 넣지 않고 별도 "live app/open externally" 기능으로 설계한다.

### D4. Markdown rendered mode 는 DOMPurify parent mount 유지

Markdown 은 `marked` 후 DOMPurify 를 거쳐 `.doc-md` / `.document-md` 에 mount 한다.

HTML 에도 남아있는 `renderHtml(raw)` helper 는 non-iframe fallback 이 아니라 source/legacy helper 이며, HTML rendered path 의 정본은 `buildRenderedHtmlSrcdoc(raw)` + iframe 이다.

DOMPurify 정책은 다음 intent 를 유지한다.

- `<script>`, `on*`, `javascript:`, `<iframe>`, `<object>`, `<embed>` 는 제거.
- SVG/MathML/media allowlist 는 markdown/legacy rendered helper 에 남긴다.

### D5. interactive mode 는 제거하고 별도 설계 전까지 비채택

2026-05-21 의 `interactive` mode (`sandbox="allow-scripts allow-popups"` + height probe)는 제거한다.

제거 이유:

- rendered iframe 이 CSS scope 와 MathJax 요구를 동시에 충족해, 기존 interactive mode 의 주요 필요성이 줄었다.
- interactive mode 의 새 탭/팝업/내부 route/height auto-fit 정책이 사용자의 현재 문서 보기 목적과 충돌했다.
- raw HTML 을 "실행 가능한 앱"으로 다루려면 relative asset resolution, popup policy, form/download policy, keyboard isolation, CSP, open-external affordance 를 별도 제품 surface 로 설계해야 한다.

향후 필요 시 `Document HTML live app mode` 같은 새 ADR 로 다룬다. 기존 `interactive` 문자열과 iframe height-probe helper 는 정본 API 에서 제거한다.

### D6. iframe drag isolation 은 HTML rendered + PDF 에 적용

Document item 의 iframe 은 drag 중 pointer event 를 capture 할 수 있다. 기존 ADR-0018 D10 amend ⑧ 의 iframe isolation invariant 는 유지하되 대상은 `.doc-html-frame` 과 `.doc-pdf` 로 정리한다.

Maximized modal 은 drag surface 가 아니므로 drag isolation class 가 필요 없다.

## 거절된 대안

### R1. HTML rendered 를 parent DOM 에 sanitize mount

거절. DOMPurify 는 XSS sanitizer 이며 CSS scope isolator 가 아니다. standalone HTML 의 `<style>` 만으로도 parent app layout/chrome 이 깨질 수 있다.

### R2. `sandbox=""` iframe

거절. CSS scope 는 해결하지만 MathJax 등 script-rendered static output 이 깨진다. 사용자 재현 파일의 수식 표시 요구와 맞지 않는다.

### R3. `interactive` mode 유지

거절. 현재 요구는 문서 표시 안정화이며, interactive 는 새 탭/빈 탭/route navigation 의 별도 UX 문제를 만든다. 필요한 경우 독립 설계로 재도입한다.

### R4. rendered iframe 에 `<base target="_blank">` 주입

거절. 내부 `#hash` routing 이 문서 내부 이동으로 동작해야 하는데, base target 주입은 이 동작을 깨뜨릴 수 있다.

## 검증 / Acceptance

- [x] `DocumentViewMode` 는 `rendered | source` 두 값만 가진다.
- [x] DocumentNode + MaximizedItemModal 이 같은 `documentRender.ts` helper 를 사용한다.
- [x] HTML rendered mode 는 `iframe srcdoc sandbox="allow-scripts"` 로 parent DOM 과 CSS scope 를 분리한다.
- [x] HTML rendered mode 는 `allow-same-origin`, `allow-popups`, `allow-top-navigation`, `allow-forms` 를 주지 않는다.
- [x] rendered HTML 에 `<base target="_blank">` 를 주입하지 않아 내부 `#hash` anchor 가 iframe 내부에서 동작한다.
- [x] markdown rendered/source toggle 은 기존 동작을 유지한다.
- [x] `pnpm --dir codebase/frontend check` 통과.
- [x] `pnpm --dir codebase/frontend build` 통과.

## 변경 이력

- 2026-05-21: 신규 — HTML viewer 의 sandboxed interactive mode 와 SVG/MathML/media allowlist 확장.
- 2026-05-22: viewMode persist store, iframe height auto-fit probe, drag-time iframe isolation 보강.
- 2026-05-26: rendered HTML 의 parent DOM 직접 mount 폐기. `sandbox="allow-scripts"` iframe 으로 CSS scope 와 MathJax 를 동시에 해결.
- 2026-05-26: interactive mode 제거. `DocumentViewMode` 를 `rendered | source` 로 축소하고, HTML rendered iframe 의 `<base target>` 주입도 제거해 내부 routing link 를 iframe 안에 보존.
