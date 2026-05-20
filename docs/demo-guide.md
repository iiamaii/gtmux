# gtmux 데모 사용자 가이드

본 문서는 **현 시점(2026-05-14)** 기준 gtmux를 로컬에서 재현해 보고, 어떤 부분이 동작하고 어떤 부분이 아직 미구현인지 그대로 확인하는 방법을 안내한다.

> **중요 — 현재 상태**: gtmux는 **pre-implementation 부트스트랩** 단계다. 빌드·코드젠·테스트 파이프라인은 정상 동작하지만, 실제 CLI 서브명령(`start`/`stop`/`teardown` 등)과 백엔드 함수 본문은 모두 `todo!()` 상태다. 따라서 본 가이드의 "데모"는 **(A) 빌드·코드젠·테스트가 깨끗하게 도는 것**과 **(B) smoke 시나리오 9단계 중 어디까지 GATE에 막히는지 시각화하는 것**을 의미한다. 살아있는 tmux 패널을 캔버스에서 다루는 진짜 UX 데모는 Sprint 0~3 구현 완료 후 가능하다(§6 참조).

## 1. 사전 준비

| 항목 | 권장 버전 | 비고 |
|---|---|---|
| OS | macOS / Linux | tmux가 unix 전용이므로 Windows 비지원 (MVP) |
| Rust toolchain | 1.78+ (stable) | `rustup` 권장 |
| Node.js | 20 LTS 이상 | 프런트엔드 빌드용 |
| tmux | **3.4 이상** (최소 3.2) | ADR-0001 §최소 버전. macOS 기본 tmux는 3.5+ 권장 |
| Git | 최근 버전 | 저장소 클론 |
| 셸 | bash 또는 zsh | smoke 스크립트는 bash 4+ 기능 사용 |

확인:
```bash
rustc --version
node --version
tmux -V
```

## 2. 저장소 클론과 디렉터리 둘러보기

```bash
git clone https://github.com/iiamaii/gtmux.git
cd gtmux
```

| 경로 | 역할 |
|---|---|
| `docs/sketch.md` | 1차 설계 spec (KO, source of truth) |
| `docs/adr/` | 9개 ADR (Accepted) |
| `docs/ssot/` | 3개 SSoT — 와이어 프로토콜·보안 디폴트·캔버스 레이아웃 스키마 |
| `docs/reports/` | 리서치 보고서(R1~R8), 정합성 리뷰, smoke 보고서 |
| `codebase/` | 실제 코드 트리(현재 빌드만 가능) |
| `codebase/Makefile` | 단일 entrypoint — `make help` 부터 시작 |

먼저 시작하기 좋은 읽기 순서:
1. `CLAUDE.md` — 프로젝트 메타·언어 규칙
2. `CONTEXT.md` — 도메인 어휘(Server / Session / Pane / Panel / Group / M / I)
3. `docs/sketch.md` §1, §4, §15 — 범위·설계 원칙·개발 단계
4. `docs/reports/0014-session-handoff.md` — 최신 진행 스냅샷

## 3. 빌드·코드젠·테스트 데모 (지금 동작함)

`codebase/` 하위에서 단일 entrypoint(`make`)로 실행한다.

```bash
cd codebase
make help        # 6개 타겟 확인
make codegen     # Rust utoipa → shared/openapi.yaml → TypeScript 타입
make build       # cargo build --workspace + vite build
make test        # cargo test + svelte-check
```

### 3.1 기대 출력 — `make codegen`

```
wrote /…/codebase/shared/openapi.yaml
✨ openapi-typescript 7.13.0
🚀 ../shared/openapi.yaml → src/lib/types/api.d.ts [10.2ms]
```

산출물:
- `codebase/shared/openapi.yaml` — Rust `utoipa` 매크로에서 생성된 OpenAPI 정의 (현재는 `Group{id}`, `Panel{id}` 스텁)
- `codebase/frontend/src/lib/types/api.d.ts` — TypeScript 타입 (자동 갱신)

> 코드젠 방향은 항상 **Rust → OpenAPI → TS** 단방향(ADR-0012 D7). 반대 방향 금지.

### 3.2 기대 출력 — `make build`

```
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.05s
✓ built in 466ms
```

(첫 빌드는 cargo 의존성 컴파일로 2~3분 소요)

### 3.3 기대 출력 — `make test`

```
running 0 tests   …   8 개 doctest suites
svelte-check found 0 errors and 0 warnings
```

테스트 본문은 P0 작업과 함께 추가됨(현재는 스켈레톤만). 실패가 0이면 부트스트랩 PASS.

### 3.4 정리

```bash
make clean       # target/ node_modules/ dist/ shared/openapi.yaml 모두 제거
```

## 4. Smoke 데모 — 9단계 시나리오 (현재 2 PASS / 6 GATE / 1 MANUAL)

```bash
cd codebase
make smoke
# 또는 직접:
bash smoke/01_engine_connect.sh
```

`SMOKE_GATE_RUNTIME=1` (기본값)에서는 step 3~9가 미구현 GATE로 short-circuit되며, 실제 동작 단계만 PASS로 표시된다.

| 단계 | 시나리오 | 현 상태 | 통과 시점 |
|---|---|---|---|
| 1 | `make build` | ✅ **PASS** | 지금 |
| 2 | `make codegen` (openapi.yaml + api.d.ts 산출) | ✅ **PASS** | 지금 |
| 3 | `gtmux start --session smoke --port 9999` | ⏳ GATE | Sprint 1 (P0-LIFE-1 + P0-CLI-1) |
| 4 | 외부 `tmux -L gtmux-smoke a -t smoke` attach | ⏳ GATE | Sprint 1 |
| 5 | `curl -H "Authorization: Bearer …" http://127.0.0.1:9999/` → SPA + CSP | ⏳ GATE | Sprint 2 (P0-HTTP-1) |
| 6 | WS 핸드셰이크 (`Sec-WebSocket-Protocol: gtmux.v1, bearer.<token>`) | ⏳ GATE | Sprint 2 (P0-WS-1) |
| 7 | `GET /api/layout` → `{groups:[],panels:[]}` + ETag | ⏳ GATE | Sprint 2 (P0-HTTP-1) |
| 8 | 브라우저에 xterm.js 1 인스턴스 표시 | 👁 MANUAL | Sprint 3 (FE-1·FE-2) |
| 9 | `gtmux teardown --session smoke` 5단계 정리 | ⏳ GATE | Sprint 2 (P0-LIFE-2 + P0-CLI-3) |

상세 결과는 `docs/reports/0012-bootstrap-smoke.md`에 PASS/GATE/MANUAL 증거와 함께 기록되어 있다.

## 5. 데모 시나리오 (현재 시연 가능한 흐름)

발표나 PR 데모에서 보여줄 수 있는 *현 시점 흐름* 한 줄짜리 시퀀스:

```bash
# 1. 깨끗한 클론에서 한 번에 부트스트랩 사이클 검증
cd codebase
make clean
make codegen   # Rust → OpenAPI → TS 단방향 코드젠
make build     # cargo + vite 동시 빌드
make test      # 0 errors / 0 warnings
make smoke     # 2 PASS / 6 GATE — 의도된 GATE 메시지 확인

# 2. 산출물 보기
cat shared/openapi.yaml | head -30
cat frontend/src/lib/types/api.d.ts | head -20

# 3. 코드 구조 — 6 crates + 2 bins
ls backend/crates/   # auth config http-api lifecycle mux-router ws-server
ls backend/bin/      # gtmux-cli gen-openapi

# 4. 도메인·계약 문서 점검
sed -n '1,80p' ../docs/ssot/wire-protocol.md       # WS envelope 타입 표
sed -n '1,60p' ../docs/ssot/security-defaults.md   # 토큰·CSP·바인드 디폴트
sed -n '1,60p' ../docs/ssot/canvas-layout-schema.md # HTTP PUT /api/layout 스키마
```

이 시퀀스는 **약 3~4분** 안에 끝나며, "엔진 연결 전이지만 ADR·SSoT·코드·코드젠·smoke 게이트가 한 사이클로 정렬되어 있다"를 보여주는 데 적합하다.

## 6. 향후 데모 — Sprint 0~3 완료 후

Sprint 0~3가 끝나면(`docs/reports/0015-progress-status.md` §6 시퀀스), 다음의 *실 사용 흐름* 데모가 가능해진다.

```bash
# Sprint 1~2 완료 후 (1단계 PASS)
gtmux start --session demo --port 9999
# 콘솔: "open http://127.0.0.1:9999/auth/bootstrap?token=…"

# 브라우저로 위 URL 열기 → 1회 cookie 교환 → 메인 캔버스 진입
# (Sprint 3 완료 후) xterm.js 패널이 캔버스에 표시됨

# 외부 attach (선택)
tmux -L gtmux-demo a -t demo

# 정리
gtmux teardown --session demo
```

진짜 데모 가능 항목:
- 단일 패널 터미널 입출력(WS streaming + xterm.js 렌더)
- 캔버스 위 패널 드래그·이동 → HTTP PUT `/api/layout` 영속화
- 새로고침/재접속 → 레이아웃 복원
- 외부 tmux CLI에서 `kill-pane` → gtmux 캔버스에 즉시 반영(MT-3 Live Mirror)

## 7. 흔한 함정 — 데모 시 주의

| 함정 | 해결 |
|---|---|
| `make build`가 `npm install`을 자동 호출하지 않음 | 첫 회 `cd frontend && npm install` 한 번 실행 후 `make build` |
| tmux 버전이 3.2 미만 | gtmux start가 startup-time에 거부(현재 미구현이라 컴파일은 됨). brew/apt로 3.5+ 설치 권장 |
| `?token=…` URL을 즐겨찾기 | **금지** — `/auth/bootstrap`은 1회 cookie 교환 전용(ADR-0003 R(rej)2). cookie 교환 후 URL의 token은 무효화됨 |
| 같은 session에 두 번째 `gtmux start` | 포트 충돌로 두 번째 실행은 종료(ADR-0007 1:1:1) — 의도된 동작 |
| 외부에서 `tmux kill-server`로 daemon 종료 | gtmux Server가 함께 종료. 재시작은 사용자 책임(재바인딩 UI 없음, ADR-0007 D4) |
| 멀티 모니터로 캔버스 다른 영역을 동시에 보고 싶다 | **명시적 미지원**(MT-3 D13). focus mode 또는 mini-map(P1+)으로 우회 |

## 8. 깊이 더 파고들기

- 백엔드 스택 결정 → `docs/adr/0011-backend-stack-rust.md`
- 프런트엔드 스택 결정 → `docs/adr/0012-frontend-stack-svelte.md`
- 와이어 envelope 타입 0x80~0x84 → `docs/ssot/wire-protocol.md`
- 보안 디폴트 12개 체크리스트 → `docs/ssot/security-defaults.md`
- 캔버스 레이아웃 JSON 스키마 → `docs/ssot/canvas-layout-schema.md`
- 23개 핵심 결정(D1~D23) 요약 → `docs/reports/0014-session-handoff.md` §"핵심 결정"
- 현 상태 PM 스냅샷 → `docs/reports/0015-progress-status.md`

## 9. 피드백·이슈 보고

- GitHub Issues: `iiamaii/gtmux` (push가 keychain 문제로 차단 중 — 상황 정상화 후 활성)
- 임시: 본 저장소를 클론한 환경에서 `docs/reports/0016-…` 형식으로 발견 사항을 기록

## 변경 이력

- 2026-05-14: 초안 (Sprint 0 dispatch 직전 부트스트랩 상태 기준)
