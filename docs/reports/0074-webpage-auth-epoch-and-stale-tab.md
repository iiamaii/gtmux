# 0074 — Webpage 인증 epoch 와 stale tab 방지 설계

- 작성일: 2026-05-18
- 작성 주체: agent (architecture discussion report)
- 범위: Token / Cookie auth / Webpage owner / Session attach lifecycle
- 트리거: 사용자 질문 — "web 별 client 를 token 단위로 구분하는 게 좋은가, 아니면 token 은 사용자 인증으로만 쓰는 게 좋은가. 현재 문제는 web 이 열려있는 상태에서 Server 를 닫았다가 다시 열었을 때 다른 탭에서 인증하면 해당 Webpage 가 그대로 사용할 수 있다는 점"
- 정본 cross-link:
  - `CONTEXT.md` — Webpage / Session / Terminal / Reconnect Gate 정의
  - `docs/adr/0019-session-and-workspace-model.md` — Webpage : Session = 1:1, single-attach, owner key
  - `docs/adr/0020-auth-lifecycle.md` — token / cookie lifecycle
  - `docs/adr/0021-terminal-pool-and-mirror.md` — WS heartbeat / attach release
  - `docs/reports/0071-session-terminal-panel-lifecycle-audit.md` — session-terminal-panel lifecycle 감사

---

## 0. Executive summary

**결론**: Token 은 계속 **사용자 인증 credential** 로만 유지해야 한다. Webpage 별 client identity 를 token 으로 쪼개면 auth 와 attach ownership 의 책임이 섞인다. 현재 문제는 token scope 문제가 아니라 **브라우저 cookie jar 공유 + Server 재시작 후 기존 Webpage 의 stale runtime state** 문제다.

권장 해결은 다음 두 계층을 추가하는 것이다.

1. **FE guard**: Webpage 가 기억한 `server_id` 와 현재 WS/HTTP 가 알려주는 `server_id` 가 다르면 active Session 을 즉시 clear 하고 bootstrap / auth / session-choice 흐름으로 되돌린다.
2. **BE guard**: 현재 Server boot 에서 발급된 **per-Webpage boot capability** 없이는 attach / mutation write path 를 거부한다. 즉 새 cookie 를 다른 탭이 발급해도, 오래 열린 Webpage 는 자기 boot capability 가 stale 이므로 그대로 write 할 수 없다.

Token 을 per-Webpage 로 만들 필요는 없다. 대신 `cookie + webpage_id + server_boot_id + webpage_boot_nonce` 의 조합으로 "이 Webpage 가 현재 Server lifetime 에서 정상 bootstrap 을 통과했는가" 를 검증한다.

---

## 1. 현재 문제

### 1.1 시나리오

1. Webpage A 가 열려 있고 Session `alpha` 에 attach 되어 있다.
2. Server 가 종료된다.
3. Server 가 다시 시작된다. BE 의 in-memory `SessionTable`, session lock table, hub owner map 은 모두 초기화된다.
4. Webpage B 가 같은 origin 에서 token/password 로 재인증한다.
5. 브라우저는 새 `gtmux_auth` cookie 를 같은 origin 의 cookie jar 에 저장한다.
6. Webpage A 는 reload 하지 않았지만, 이후 HTTP/WS 요청에는 브라우저가 새 cookie 를 자동으로 붙인다.
7. Webpage A 의 `sessionStorage` 에 남아 있는 `webpage_id`, active Session, layout state 와 새 cookie 가 결합되어 "A 가 직접 인증/bootstrapping 을 통과하지 않았는데도" 다시 사용 가능한 것처럼 보일 수 있다.

### 1.2 문제의 본질

이 문제는 "token 이 Webpage 별로 구분되지 않는다" 가 아니다. 핵심은 다음 둘이다.

- **Cookie 는 origin 단위로 공유된다.** 같은 브라우저 프로필의 탭들은 `gtmux_auth` 를 공유한다.
- **Webpage runtime state 는 탭 메모리/sessionStorage 에 남아 있다.** Server 재시작 후에도 FE store 의 active Session / reconnect hint / webpage_id 는 남을 수 있다.

따라서 다른 탭에서 재인증한 새 cookie 를 오래 열린 탭이 자동으로 얻는 것은 브라우저 보안 모델상 자연스럽다. 우리가 막아야 하는 것은 cookie 공유 자체가 아니라, **stale Webpage 가 현재 Server lifetime 의 bootstrap 을 생략하고 write path 로 진입하는 것**이다.

---

## 2. Token 을 Webpage 단위로 쪼개면 안 되는 이유

### 2.1 Token 의 도메인 책임

현재 token 은 Server 접근 권한을 증명하는 credential 이다.

- URL `?t=` / `/auth/login` 의 token mode 입력
- CLI / automation 의 bearer credential
- cookie 발급의 상위 인증 재료
- rotate-token 의 대상

즉 token 은 **사용자 또는 operator 가 Server 에 접근할 수 있는가**를 판정한다. 반면 Webpage 는 **브라우저 탭 단위 편집 채널**이다. 이 둘은 도메인 책임이 다르다.

### 2.2 Token 을 client identity 로 쓰면 생기는 문제

Token 을 Webpage 별 client identity 로 확장하면 다음 문제가 생긴다.

- **책임 혼합**: 인증 credential 과 attach owner identity 가 한 값으로 합쳐진다.
- **운영 복잡도 증가**: 탭마다 token 발급/회수/회전 정책이 필요해진다.
- **CLI / automation 영향**: bearer token 의 의미가 "사용자 인증" 에서 "특정 Webpage" 로 흔들린다.
- **보안 모델 혼선**: token 유출 대응과 stale tab cleanup 이 같은 메커니즘에 묶인다.
- **현재 single-user 모델과 불일치**: multi-user session model 처럼 복잡해지지만 실제 문제는 per-tab runtime freshness 다.

gtmux 의 기존 원칙은 state domain 을 섞지 않는 것이다. Token 을 Webpage identity 로 재사용하는 것은 auth domain 과 Webpage attach domain 을 섞는 결정이다.

---

## 3. 유지해야 할 역할 분리

| 개념 | 책임 | 수명 | 저장 위치 |
|---|---|---|---|
| Server token | 사용자/운영자 인증 credential | Server token rotation 까지 | XDG state token file |
| `gtmux_auth` cookie | 브라우저가 인증을 통과했음을 나타내는 opaque session id | cookie max-age / logout / Server restart 의 in-memory table | browser cookie jar + BE memory |
| `webpage_id` | 탭 단위 Webpage 식별자 | 탭 sessionStorage lifetime | browser sessionStorage |
| owner key | attach lock owner 식별 | current Server memory | BE memory, `cookie + 0x1f + webpage_id` |
| Server boot id | 현재 Server lifetime 식별 | process lifetime | BE memory |
| Webpage boot capability | 이 Webpage 가 현재 Server boot 에서 bootstrap 을 통과했는지 증명 | process lifetime 또는 짧은 TTL | BE memory + FE memory/sessionStorage |

이 중 현재 빠져 있는 것은 마지막 두 계층의 강제성이다. `server_id` 는 존재하지만 stale Webpage 를 강제로 탈락시키는 일관된 protocol 로 아직 충분히 쓰이지 않는다. Webpage boot capability 는 아직 별도 도메인 객체로 없다.

---

## 4. 권장 해결안

### 4.1 FE: Server boot mismatch 감지

FE 는 다음 이벤트에서 현재 Server 의 `server_id` 를 관측해야 한다.

- `/api/sessions` auth gate 응답 또는 별도 `/api/bootstrap` 응답
- `POST /api/sessions/:name/attach` 응답
- WS hello 또는 초기 `LAYOUT_CHANGED` 계열 handshake

FE 는 `sessionStorage` 또는 in-memory 에 저장한 `server_id` 와 새로 관측한 값이 다르면 다음을 수행한다.

1. `sessionStore.clear()`
2. `terminalPool` UUID↔PaneId binding clear
3. `danglingTerminals` clear
4. `reconnectGate.markIdle()` 또는 auth gate 재진입
5. `workspaceSwitcher.open()` 또는 `/auth` redirect

목표는 오래 열린 Webpage A 가 새 Server lifetime 을 감지한 순간 기존 active Session 을 더 이상 신뢰하지 않도록 만드는 것이다.

이 계층은 UX 안정성에 중요하지만, 보안적으로는 단독으로 충분하지 않다. FE 는 우회 가능하고, 오래 열린 JS state 가 race 를 만들 수 있기 때문이다.

### 4.2 BE: per-Webpage boot capability 도입

BE 는 현재 Server boot 에서 Webpage 별 capability 를 발급하고 검증해야 한다.

제안 wire:

```text
POST /auth/login
  -> Set-Cookie: gtmux_auth=...

GET /api/bootstrap
  headers:
    Cookie: gtmux_auth=...
    X-Gtmux-Webpage-Id: <webpage_id>
  -> 200 {
       server_id,
       webpage_boot_nonce,
       auth_mode,
       expires_at_hint?
     }
```

FE 는 이후 write-sensitive 요청에 다음 헤더를 포함한다.

```text
X-Gtmux-Webpage-Id: <webpage_id>
X-Gtmux-Webpage-Boot: <webpage_boot_nonce>
```

BE 는 다음 조건을 검증한다.

```text
cookie is valid in SessionTable
AND webpage_id is syntactically valid
AND webpage_boot_nonce exists in current Server memory
AND nonce belongs to (cookie, webpage_id, server_id)
```

검증 대상 endpoint:

- `POST /api/sessions/:name/attach`
- `DELETE /api/sessions/:name/attach`
- `POST /api/sessions/:name/attach/confirm`
- `PUT /api/sessions/:name/layout`
- `DELETE /api/sessions/:name/items/:id`
- `POST /api/sessions/:name/terminals`
- `POST /api/terminals/:id/kill`
- `POST /api/terminals/:id/respawn`
- WS upgrade query or subprotocol equivalent

읽기 endpoint 는 단계적으로 나눌 수 있다.

- `/api/sessions`, `/api/settings` 등 auth gate 성격의 read 는 cookie 만으로 허용 가능.
- layout read 는 제품 결정 필요. strict 하게 가려면 layout read 도 boot capability 요구. UX 우선이면 read 는 허용하고 write 만 막는다.

### 4.3 Capability 저장 정책

BE memory 에 다음 map 을 둔다.

```text
webpage_boots: HashMap<WebpageBootNonce, {
  cookie_hash_or_id,
  webpage_id,
  server_id,
  issued_at,
  last_seen_at
}>
```

권장:

- nonce 는 128-bit 이상 CSPRNG.
- raw cookie 를 로그에 남기지 않는다. map key 로 cookie value 를 직접 써도 memory 내부라 가능하지만, trace 에는 길이/해시 prefix 만 남긴다.
- `SessionTable::validate` 가 cookie rolling renewal 을 하듯, bootstrap nonce 도 `last_seen_at` 을 갱신할 수 있다.
- Server restart 시 map 은 비어야 한다. 이게 stale tab 차단의 핵심이다.

---

## 5. 기대 동작

### 5.1 정상 새 탭

1. 사용자가 auth 통과.
2. FE 가 `GET /api/bootstrap` 호출.
3. BE 가 현재 `server_id` 와 `webpage_boot_nonce` 발급.
4. FE 가 session 선택/attach 진행.
5. attach/mutation 은 cookie + webpage_id + nonce 검증을 통과.

### 5.2 오래 열린 탭 + Server restart + 다른 탭 재인증

1. Webpage A 는 old `server_id`, old nonce 를 가진다.
2. Server 재시작으로 BE memory 의 `SessionTable`, `webpage_boots`, attach lock table 이 초기화된다.
3. Webpage B 가 재인증해서 새 cookie 를 발급받는다.
4. 브라우저 cookie jar 공유 때문에 A 도 새 cookie 를 자동으로 싣는다.
5. 그러나 A 의 old nonce 는 새 Server 의 `webpage_boots` 에 없다.
6. A 의 attach/mutation/WS upgrade 는 `401` 또는 `409/428` 계열의 typed error 로 거부된다.
7. FE 는 stale bootstrap 으로 인식하고 `sessionStore.clear()` 후 auth/session-choice flow 로 돌아간다.

이때 B 의 재인증은 A 를 "자동 권한 상승" 시키지 않는다. A 는 현재 Server boot 에서 bootstrap 을 다시 통과해야 한다.

### 5.3 같은 탭 reload

reload 는 sessionStorage 의 `webpage_id` 를 유지할 수 있다. 하지만 reload 된 JS 는 bootstrap 을 다시 호출해 새 nonce 를 받아야 한다. 같은 cookie + 같은 webpage_id 라도 nonce 가 current boot 에서 재발급되므로 정상 진입 가능하다.

### 5.4 cookie rotation / logout all

cookie 가 rotate 되면 기존 nonce 는 revoke 되어야 한다. 최소 정책:

- `/auth/logout`: 해당 cookie 에 속한 boot nonce 모두 제거
- `/auth/rotate`: old cookie 에 속한 boot nonce 모두 제거, caller 는 새 cookie 로 `/api/bootstrap` 재호출
- `revoke_all`: 모든 boot nonce 제거

---

## 6. 대안 비교

| 대안 | 장점 | 단점 | 판단 |
|---|---|---|---|
| Token 을 Webpage 별로 발급 | stale tab 을 직관적으로 막는 것처럼 보임 | token 책임 오염, CLI/automation 영향, rotation 복잡도 증가 | 거절 |
| FE server_id mismatch 만 처리 | 구현 작음, UX 즉시 개선 | 보안/정합 guard 로는 약함, stale write 를 BE 에서 차단 못 함 | 보조책 |
| Cookie 를 tab 별로 분리 | Webpage 단위 auth 분리 | 브라우저 cookie 모델과 충돌, SameSite/HttpOnly 장점 감소 | 거절 |
| owner_key 에 server_id 추가 | 현재 attach lock 과 자연 결합 | Server restart 후 새 cookie 를 받은 stale tab 이 새 owner 로 attach 가능할 수 있음 | 불충분 |
| per-Webpage boot capability | 문제를 직접 해결, 책임 분리 유지 | 새 endpoint/store/header 필요 | 채택 권장 |

---

## 7. 구현 제안

### Phase 1 — FE detection only

- `GET /api/sessions` 또는 새 `GET /api/bootstrap` 에 `server_id` 를 포함한다.
- FE 는 observed `server_id` 를 `sessionStorage` 에 저장한다.
- mismatch 시 `sessionStore.clear()` + `terminalPool.clear()` + `reconnectGate.markIdle()` + session 선택 modal 진입.

Acceptance:

- Server restart 후 기존 Webpage 가 WS reconnect / auth gate 를 만나면 빈 기존 Canvas 를 계속 쓰지 않고 session 선택 흐름으로 돌아간다.

### Phase 2 — BE boot capability

- `AppState` 에 `webpage_boots` store 추가.
- `GET /api/bootstrap` 추가.
- FE bootstrap 에서 `/api/sessions` auth gate 대신 `/api/bootstrap` 호출.
- FE `webpageHeaders()` 또는 새 helper 가 `X-Gtmux-Webpage-Boot` 도 포함.
- BE write-sensitive handlers 에 middleware/helper 로 capability 검증 추가.
- WS upgrade 에도 `webpage_id` + nonce 를 query 로 싣거나 subprotocol token 으로 싣는다.

Acceptance:

- 오래 열린 Webpage A 가 Server restart 후 다른 탭 B 의 재인증 cookie 를 자동으로 얻어도, A 의 `PUT /layout`, `POST /attach`, WS upgrade 는 stale boot capability 로 거부된다.
- A 가 reload 또는 bootstrap 재진입하면 새 nonce 를 받고 정상 동작한다.

### Phase 3 — lifecycle cleanup

- logout / rotate / revoke_all 에서 해당 boot nonce 제거.
- disconnect / leave 시 nonce 를 즉시 제거할지 여부 결정.
  - 보수적: leave 시 nonce 유지, attach lock 만 release. 같은 탭 reconnect 가 부드럽다.
  - 엄격: leave 시 nonce 제거. 다시 쓰려면 bootstrap 필요.
- 추천: nonce 는 auth bootstrap capability 이므로 leave 에서 제거하지 않는다. attach lock 과 nonce lifecycle 을 섞지 않는다.

---

## 8. Open questions

1. **Read endpoint 도 boot capability 를 요구할 것인가?**
   - 권장: write path 먼저. layout read 는 UX와 privacy 기준으로 별도 결정.

2. **WS upgrade 에 nonce 를 어디에 실을 것인가?**
   - Query: `?webpage_id=...&webpage_boot=...` 구현이 단순하지만 URL 로그 노출 가능성.
   - Subprotocol: 기존 `gtmux.v1`, `bearer.*` 패턴과 맞출 수 있으나 parsing 복잡도 증가.
   - Header 는 browser WebSocket API 에서 custom header 불가.
   - 권장: nonce 는 opaque + short-lived 라면 query 허용 가능. 로그 redaction 필요.

3. **nonce TTL 은 cookie max-age 와 같아야 하는가?**
   - 권장: Server lifetime + idle TTL. cookie 는 사용자 인증, nonce 는 current boot Webpage freshness 이므로 같은 TTL 일 필요 없다.

4. **다른 탭 재인증이 기존 탭을 즉시 logout 시켜야 하는가?**
   - 권장: 아니오. 기존 탭은 다음 server interaction 에서 stale capability 로 bootstrap flow 에 들어가면 충분하다.

---

## 9. 결론

Token 은 Webpage 별 client identity 가 아니라 **사용자 인증 credential** 로 유지한다. Webpage 별 구분은 이미 `webpage_id` 와 owner key 가 담당하고 있으며, 여기에 부족한 것은 **현재 Server boot 에서 발급된 Webpage capability** 다.

따라서 해결 방향은:

```text
Token = 사용자 인증
Cookie = 인증된 브라우저 session
Webpage ID = 탭 식별
Boot capability = 현재 Server lifetime 에서 bootstrap 된 Webpage 증명
Attach lock = Session 편집 권한
```

이 분리를 지키면 stale tab 문제를 직접 막으면서도 token / cookie / attach / terminal stream 의 책임 경계를 유지할 수 있다.

---

## 10. Phase 1 land 보고 (2026-05-19)

`§7.1 Phase 1 — FE detection only` land 완료.

### 10.1 변경

| Layer | 변경 |
|---|---|
| BE `sessions::list_handler` | response 에 `X-Gtmux-Server-Id` header 동봉 (current `AppState::server_id`, UUID v4) |
| FE `lib/session/serverId.ts` (신규) | `observeServerId(id)` (sessionStorage `gtmux_observed_server_id` 와 compare) + `onServerIdMismatch(handler)` (registry) + `peek/reset` test helpers |
| FE `lib/http/sessions.ts` | `listSessions()` 가 response header 의 server_id observe + `attachSession()` 도 body 의 `server_id` observe (두 path 의 entry 모두 cover) |
| FE `routes/+page.svelte` | `onMount` 에서 mismatch handler 등록 — `sessionStorageHint.clear` + `sessionStore.clear` + `reconnectGate.cancel` + `reconnectGate.markIdle` + `workspaceSwitcher.open` + warning toast. `onDestroy` 에서 handler detach. auth-gate raw fetch 의 `res.ok` 분기에서도 직접 `observeServerId` 호출 (wrapper bypass path 정합) |
| ADR-0020 D15 신규 | server boot identity 명시. Phase 2 (boot capability) 는 별 amend 후보 |

### 10.2 검증

- `cargo test --workspace --no-fail-fast`: **429 PASS / 0 FAIL** (baseline 428 + `sessions_list_emits_server_id_header` 신규)
- `pnpm check`: 317 files / 0 errors / 0 warnings
- `pnpm build`: OK (1.63s)

### 10.3 잔여 (Phase 2 / Phase 3)

- **Phase 2** (BE `webpage_boot_nonce` + `/api/bootstrap` + write-sensitive endpoint guard): 별 발주. ADR-0020 amend draft 후 사용자 grill 필요. 본 Phase 1 의 *UX 회복* 만으로는 *보안적 보장 부재* — 단 single-user 환경에서는 race-window 가 짧고 본인 cookie 만 영향이라 *체감* desync 차단으로 충분.
- **Phase 3** (logout/rotate 의 nonce 제거): Phase 2 의존.

### 10.4 동작 시나리오 검증

| 시나리오 | 기대 | 본 commit 동작 |
|---|---|---|
| Server restart 후 stale tab 의 `/api/sessions` 호출 | mismatch → cleanup + session 선택 | ✅ `observeServerId` 가 mismatch 감지 → handler 발화 → workspaceSwitcher.open |
| 같은 boot 의 두 번째 `/api/sessions` | 변경 0 | ✅ `match` return — handler 안 fire |
| 첫 fresh tab 의 첫 `/api/sessions` | id 저장만, handler 안 fire | ✅ `first` return |
| SSR / sessionStorage 차단 | detection skip, no crash | ✅ `no-storage` return |
