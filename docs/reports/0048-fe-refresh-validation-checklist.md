# 0048 — FE refresh / Phase 2 검증 체크리스트

- 작성일: 2026-05-16
- 종류: FE manual / E2E 검증 체크리스트 (BE 0046 ship 후 사용)
- 관련: `docs/reports/0045-refresh-session-reconnect-loop-analysis.md` (분석), `0046-be-attach-handler-idempotent.md` (BE 의존), `0047-be-next-session-handover.md` (BE 전달), `0043-fe-integrated-session-handover.md` §1.14 (FE land)

---

## 0. 사용 시점

BE 측 0046 (attach_handler same-cookie idempotent) ship 직후. FE 측 묶음 E 의 land (`da7663b` + 묶음 F 확장) 검증 + 0045 §9 완료 기준 7항 충족 확인.

```bash
# BE 0046 ship 확인
cd codebase/backend && cargo test -p gtmux-http-api attach_idempotent_for_same_cookie_same_session 2>&1 | tail -5
# 기대: 1 passed

# FE 최신 빌드 확인
ls -lt codebase/frontend/dist/assets/*.js | head -3
grep -o 'index-[A-Za-z0-9_-]*\.js' codebase/frontend/dist/index.html
# 새 hash 가 dist/assets/ 안에 있어야 — 옛 hash 만 있으면 build 다시
```

---

## 1. Dev instrumentation 활성화

브라우저 DevTools 콘솔:

```js
// 활성
__gtmuxDebug.enable();
// localStorage flag set 됨 — 다음 reload 도 활성 유지

// 확인
localStorage.getItem('gtmux-debug-counts');  // → "1"

// 스냅샷 (현재 누적 카운터)
__gtmuxDebug.snapshot();
// → { "canvas.mount": 1, "flowNodes.rebuild": 5, "flowNodes.cache.hit": 47, ... }

// 리셋 (시나리오 사이 cleanup)
__gtmuxDebug.reset();

// 비활성
__gtmuxDebug.disable();
```

카운터 출력 throttle: 매 1초마다 console.debug 로 summary dump. 직접 snapshot() 호출도 가능.

---

## 2. 검증 시나리오 (0045 §8.2 정합)

각 시나리오 사이 `__gtmuxDebug.reset()` 호출 + DevTools Network 탭 clear.

### S1 — Session hint 없는 최초 진입

**준비**:
1. DevTools → Application → Storage → Clear site data (또는 사용자가 명시 logout 후 cookie 도 cleared 상태)
2. magic-link URL 로 진입 `http://127.0.0.1:9999/auth/bootstrap?token=<X>`

**기대**:
- BE → 303 chain → / 진입
- SPA load → boot screen "Preparing workspace…" 잠시
- WorkspaceSwitcher modal 자동 열림 (사용자가 session 선택해야 진행)
- Canvas 가 *modal 뒤에* 빈 상태로 보임 — empty (트레이드 오프, ADR-0019 D5.4 의 accept)
- 사용자가 session 선택 → attach → setActiveSession + loadLayout + reconnectGate.markReady → Canvas 의 items hydrate

**카운터 (snapshot 후)**:
| 카운터 | 기대 |
|---|---|
| `canvas.mount` | **1** (refresh 당 1회) |
| `sessionStore.loadLayout` | 1 (attach 성공 시) |
| `flowNodes.rebuild` | 적당 (item 수 + reactive pass 1~3회) |
| `flowNodes.cache.hit` / `miss` | hit 가 다수, miss 는 새 item 만 |
| `canvas.setViewport` | 1~2 (loadLayout 직후 1회 + onmove 의 reflexive) |
| `canvas.onmove.skip-applying` | setViewport 호출 직후만 1~2 |
| `xterm.fit` | 0 (terminal panel 없으면) |

**Console 에러 없음** (특히 `effect_update_depth_exceeded`).

### S2 — 유효한 session hint 가 있는 새로고침

**준비**:
1. S1 의 attach 끝낸 상태에서 사용자가 작업 (item 생성 / drag / etc.)
2. 브라우저 reload (Cmd+R / F5)

**기대**:
- WS close → BE 의 `release_lock_for_cookie` 비동기 발화
- 새 SPA load → boot screen 잠시 ("Reconnecting…")
- hint 검사 → reconnectGate.start(hint) → attemptReattach → 200 (**0046 ship 후 idempotent 보장**)
- loadLayout 완료 → markReady → Canvas mount + 이전 viewport / panel 배치 복원
- ReconnectModal 노출 *없음*

**카운터**:
| 카운터 | 기대 |
|---|---|
| `canvas.mount` | **1** |
| `sessionStore.loadLayout` | 1 (attempt success 시) |
| `flowNodes.rebuild` | item 수 비례 + reactive pass 적당 |
| `flowNodes.cache.hit/miss` | 새 cache start 라 첫 pass 는 모두 miss, 후속 pass 는 hit |
| `xterm.fit` | terminal panel 수만큼 (mount 1회 + ResizeObserver 첫 fire) |

⚠️ **회귀 검출**: `canvas.mount > 1` 이거나 `flowNodes.rebuild > items 수 * 5` 이면 loop 의심.

### S3 — session 이 사라진 상태의 새로고침

**준비**:
1. S2 와 동일 상태
2. 다른 cookie / CLI 로 session 삭제 (또는 BE restart + workspace_dir 비움)
3. 브라우저 reload

**기대**:
- hint → reconnectGate.start → attemptReattach → 404 → state = 'not_found' + hint clear
- ReconnectModal "Session no longer exists" 노출 + [Switch session…] / [Open session list]
- Canvas mount 안 됨

### S4 — session in-use 상태의 새로고침

**준비**:
1. S2 attach 상태에서 *다른 브라우저 / 다른 cookie* 로 같은 session attach
2. 첫 브라우저에서 reload

**기대**:
- WS close → release_lock (cookie A) → BE 의 같은 cookie 매핑 비움
- 첫 브라우저 새 SPA → hint → attemptReattach → BE 의 lock 은 cookie B 가 보유 → **409 in_use** (다른 cookie 가 정당하게 보유)
- ReconnectModal "Session is in use by another webpage. holder pid=…" + [Retry] / [Switch session…]
- Canvas mount 안 됨

⚠️ **이 시나리오만** 'in_use' 가 정당. S2 / S5 / S6 에서 'in_use' 가 떴으면 0046 fix 미적용 또는 BE 회귀.

### S5 — terminal panel 없는 layout

**준비**:
1. session 안에 shape/text/note 만 (terminal 0개)
2. reload

**기대**:
- S2 와 동일 + `xterm.fit` = 0
- `effect_update_depth_exceeded` 없음

### S6 — terminal panel 있는 layout

**준비**:
1. session 안에 terminal 1~3개 + shape/text 혼합
2. reload

**기대**:
- S2 와 동일 + `xterm.fit` = terminal 수만큼 (mount 직후 1회/panel)
- ResizeObserver entry-level dedup 작동 — 같은 width/height 재측정 시 fit() 추가 호출 0
- 사용자가 panel resize 시 fit() 호출 횟수 = 사용자 gesture 수 비례 (debounced 150ms 후 1회)

### S7 — text / figure 가 포함된 layout

**준비**:
1. text + rect + ellipse + line + file_path + note 6 type 혼합
2. reload + 각 item 의 inline edit 시도 (더블 클릭)

**기대**:
- 각 type 의 edit 가 정상 commit (text/note/file_path/line)
- 0046 ship 후 edit commit 시 in-flight reattach 없으면 ensureMutationOk 즉시 통과
- 0046 미 ship 상태에선 silentReattach 가 in-use 인 경우 toast "Text edit aborted — session reconnect failed."

### S8 — Idle 후 visibility 재진입 (Phase 2)

**준비**:
1. attach 후 15초+ idle (마우스/키보드 입력 X)
2. 탭을 background 로 보내고 5초+ 대기 후 다시 active

**기대 (0046 ship 후)**:
- visibilitychange listener → maybeSilentReattach 트리거 (isIdle && visible && active)
- silentReattach 호출 → POST /attach → **200 idempotent (0046 효과)** → loadLayout → heartbeatStore.reset()
- viewport 사용자 변경 보존 (silent 의도 — 사용자 모르게 통과)
- toast 노출 *없음*

**기대 (0046 미 ship 상태 — 회귀 확인용)**:
- silentReattach → 409 → toast "Session is in use by another webpage" (정상 회귀 확인)
- 이후 mutation 시도 → ensureMutationOk → !guard.ok → "Session reconnect failed — action aborted" toast + early return
- → 0046 의 필요성 입증

### S9 — Mutation guard 의 모든 site (sanity)

**준비**: attach 후 → DevTools 콘솔에서 `sessionStore.lastSilentReattachResult = { kind: 'in_use' }` 수동 주입 (Phase 2 fail 시뮬레이션)

**기대**: 다음 사용자 액션이 모두 toast 후 aborted:
- ContextMenu Add → "Item creation aborted — session reconnect failed."
- Canvas drag → "Drag commit aborted — session reconnect failed."
- PanelNode resize → "Resize aborted — session reconnect failed."
- PanelNode label rename → "Label rename aborted — session reconnect failed."
- LayerTreeView visibility/lock toggle → "Layer mutation aborted — session reconnect failed."
- LayerTreeView drag reorder → "Layer mutation aborted — session reconnect failed."
- ContextMenu Bring/Send (z) → "Z order change aborted — session reconnect failed."
- TextNode / NoteNode / ShapeNode / FilePathNode / LineNode inline edit → 각 type 의 메시지
- TerminalListView attach / kill → "Session reconnect failed — attach/kill aborted."

`sessionStore.lastSilentReattachResult = null` 로 복구 후 모든 mutation 다시 정상 작동 확인.

### S10 — WS reconnect → Phase 2 silent (transient network)

**준비**: attach 후 → DevTools Network 탭 → "Offline" 5초 → "No throttling"

**기대 (0046 ship 후)**:
- WS reconnecting → open 전이 → dispatcher 의 silentReattach trigger
- reconnectGate.canMountApp 가드 (idle/ready 면 통과)
- silentReattach → 200 → 사용자 모르게 통과
- ReconnectBanner 의 transient close 만 잠시 표시 후 사라짐

---

## 3. 카운터 회귀 판정 기준 (0045 §8.3 정합)

새로고침 1회 기준:

| 카운터 | 정상 | 회귀 의심 |
|---|---|---|
| `canvas.mount` | 1 | ≥ 2 (mount loop) |
| `canvas.unmount` | 0 (refresh 첫 진입 후) | ≥ 1 (mount-unmount churn) |
| `sessionStore.loadLayout` | 1 | ≥ 2 (hydrate 다중 호출) |
| `flowNodes.rebuild` | item 수 + 3~5 | item 수 * 10+ (identity churn 의심) |
| `flowNodes.cache.hit / miss` ratio | hit 가 다수 (≥ 80%) | miss 가 다수 (signature 누락 의심) |
| `canvas.setViewport` | 1~2 | ≥ 10 (loop 의심) |
| `canvas.onmove` (사용자 입력 없음) | 1~3 (SvelteFlow init 시 반사적) | ≥ 50 |
| `canvas.onmove.skip-applying` | onmove 의 일부 (apply 중 가드) | onmove ≈ skip (모든 onmove 가 reflexive — 0045 P0-B 실패 의심) |
| `xterm.fit` | terminal 수 | terminal 수 * 5+ (RAF dedup 실패 의심) |

`__gtmuxDebug.snapshot()` 결과를 commit 시 첨부 가능.

---

## 4. 0045 §9 완료 기준 매핑

| 0045 기준 | 본 체크리스트 |
|---|---|
| 새로고침 후 자동 복구 | S2 |
| 빈/partial canvas 미노출 | S1 (workspaceSwitcher modal 이 cover), S2 (boot screen 후 직진) |
| attach 실패 시 reconnect/session switch UX | S3 / S4 |
| 최신 빌드에서 update-depth 미재현 | S2 / S5 / S6 / S7 console 검증 |
| text / figure / terminal 혼합 정상 | S7 |
| viewport 동기화 + 초기 루프 X | S2 카운터 (setViewport ≤ 2) + S8 viewport 보존 |
| 기존 편집 UX 후퇴 X | S7 inline edit + Layer V2 drag (별 검증) |

---

## 5. 검증 결과 보고 양식

```markdown
### 0048 검증 결과 — YYYY-MM-DD

- 빌드: index-XXXXX.js (gzip ~XX.X KB)
- BE 0046 ship: ✅ / ❌

| S | 시나리오 | 통과 | console 에러 | snapshot 발췌 |
|---|---|---|---|---|
| 1 | Fresh entry | ✅ | 없음 | `{ canvas.mount: 1, ... }` |
| 2 | Valid hint reload | ✅ | 없음 | `{ canvas.mount: 1, loadLayout: 1, ... }` |
| 3 | Stale session | ✅ | 없음 (modal 표시) | — |
| 4 | In-use (다른 cookie) | ✅ | 없음 (정당 in_use) | — |
| 5 | Shape only | ✅ | 없음 | `{ xterm.fit: 0 }` |
| 6 | Terminal mixed | ✅ | 없음 | `{ xterm.fit: <term count> }` |
| 7 | Inline edit 전 type | ✅ | 없음 | — |
| 8 | Idle visibility | ✅ | 없음 | — |
| 9 | Mutation guard sanity | ✅ | 없음 | — |
| 10 | WS reconnect | ✅ | 없음 | — |

### 회귀 (있으면)

- [ ] 항목 1
- [ ] 항목 2

### 다음 작업

- ...
```

---

## 6. 변경 이력

- 2026-05-16: 초안 — 0045 분석 + 묶음 E ship + 묶음 F (mutation guard 확장) 후 BE 0046 ship 대기 시점의 FE 검증 체크리스트. 10 시나리오 + 카운터 판정 기준 + 보고 양식.
