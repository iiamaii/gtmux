# SSoT: Wire Protocol — WebSocket 이진 envelope

- 일자: 2026-05-13
- 정의 ADR: ADR-0002 (전송 계층 = WebSocket + 이진 envelope)
- 변경 정책: 본 SSoT는 gtmux WebSocket endpoint(`/ws`)를 흐르는 모든 이진 프레임의 1차 계약이다. envelope 코드 표 변경은 PR + ADR-0002 갱신 동반. 백엔드(Rust enum)와 프론트엔드(TS discriminated union) 모두 본 표를 직접 참조해 구현해야 한다. *예약 슬롯*(0x08–0x0F, 0x85–0x8F)의 새 할당도 본 SSoT 갱신 + ADR-0002 amend.
- 관련 ADR: ADR-0001 (tmux 통합, `%output` 디코딩 파이프라인 D7·Panel Streaming State D8), ADR-0003 (보안 디폴트, 토큰 전달 경로), ADR-0006 (영속화, HTTP `GET/PUT /api/layout`), ADR-0007 (1:1:1 바인딩), ADR-0008 (single-pane + command allowlist 정본), ADR-0009 (daemon 격리)
- 관련 SSoT: `docs/ssot/canvas-layout-schema.md` §2 (ETag 정규화 — 본 SSoT 0x80 LAYOUT_CHANGED 페이로드와 동일 규칙 인용)
- 관련 보고서: `docs/reports/0004-transport.md` (전송 후보 비교), `docs/reports/0010-grill-amendments.md` D12·D13·D14·D15·D17·D19

## 1. 프레임 구조

### 1.1 외부 프레임 (WebSocket RFC 6455)

gtmux WS는 **이진 프레임(opcode 0x2)만** 사용한다. 텍스트 프레임(opcode 0x1) 수신 시 close code 1003 (Unsupported Data)로 종료. control frame(ping/pong/close)은 RFC 6455 표준 — pong은 라이브니스 체크에만 사용하며 envelope 페이로드를 운반하지 않는다.

### 1.2 envelope 구조

```
+--------+----------------------+----------------------+
| 1B     | varint (1..5B)       | N bytes              |
| type   | paneId (or 0)        | payload (type별 정의) |
+--------+----------------------+----------------------+
```

- **`type`** (1바이트, unsigned): §2 표에서 정의. 0x01–0x0F = tmux-domain, 0x80–0x8F = web-domain. 그 외 값 수신 시 close code 1003.
- **`paneId`** (unsigned LEB128 varint): tmux pane id `%N`에서 정수 `N`만 추출. pane 무관 메시지(0x01·0x07·0x80·0x83·0x84)는 **`paneId = 0`** (tmux는 0번 pane id를 할당하지 않으므로 sentinel로 안전).
- **`payload`** (N바이트, 0 이상): `type`별로 인코딩이 다름. 일부 타입은 페이로드가 0바이트(예: 0x05, 0x06).

전체 envelope 크기는 WebSocket 프레임당 1 메시지로 1:1 매핑된다 — *fragment 사용 안 함* (서버 측 `tokio-tungstenite` 기본 자동 reassemble은 허용). 최대 envelope 크기는 1 MiB 소프트 캡 — 초과 시 서버는 `0x01 CTRL` 에러 응답으로 그 메시지만 거부하고 연결은 유지.

### 1.3 varint (unsigned LEB128) 인코딩

- 0 ≤ N < 128 → 1바이트 (`0x00`–`0x7F`).
- 128 ≤ N < 16384 → 2바이트 (예: 128 → `0x80 0x01`).
- 일반 규칙: little-endian 그룹 7비트씩, 최상위 비트 = continuation(1=more bytes, 0=last).
- gtmux pane id는 실용적 상한 < 2^28 — 최대 5바이트 면 충분. **수신 시 5바이트 초과는 close code 1003** (악의적 length-of-length 공격 방어).

## 2. Envelope 타입 코드 표 (32 슬롯 전부 정의)

### 2.1 tmux-domain (0x01–0x0F)

| 코드 | 이름 | paneId | payload | 방향 | 비고 |
|---|---|---|---|---|---|
| `0x01` | **CTRL** | 0 | UTF-8 JSON `{"cmd": string, "args": string[]}` 요청 또는 `{"ok": true, "result": …}` / `{"error": string}` 응답 | 양방향 | tmux control mode 명령. **argv 배열만 허용**, shell 문자열 금지 (R4 §"가-c", ADR-0008 allowlist). 응답은 ADR-0001 D4의 command-number 매칭으로 상관. |
| `0x02` | **PANE_OUT** | %N | raw bytes (ANSI/UTF-8 보존, base64 없음) | 서버 → 클라 | ADR-0001 D7 디코딩 결과. `%output`/`%extended-output` 동일 처리 (`%extended-output`의 `age-ms`는 telemetry only, payload에 포함 안 함). **inner payload 정의 = `varint paneId + raw bytes`** — backend `ws-server::Envelope` (outer = `[1B type][4B LE u32 length][inner]`) 와 frontend `decode.ts::decodePaneOut` 가 본 정의를 byte-equal 하게 따른다. R8 §F4 의 별도 `varint length` prefix 변형은 비채택 — outer length 가 inner 바이트 수를 결정하므로 inner 내부에는 length 가 없다. |
| `0x03` | **PANE_IN** | %N (= I) | raw bytes (UTF-8/ANSI) | 클라 → 서버 | 서버는 `send-keys -t %<N> -- <bytes>` argv 분리로 tmux에 전달. paneId는 *Input Target I* — `0x82 I_CHANGED`로 현재 I를 미러받은 클라이언트가 그 값을 사용. |
| `0x04` | **PANE_RESIZE** | %N | `varint cols + varint rows` | 클라 → 서버 | single-pane-window 컨벤션 하에서 window-size 변경 (`resize-window -x <cols> -y <rows>`). 외부 attach 클라이언트가 변경하면 `0x07 NOTIFY_MIRROR`로 미러받음. |
| `0x05` | **PANE_PAUSE** | %N | 0 bytes | 클라 → 서버 | Panel Streaming State Suspended 진입 신호. 서버는 ADR-0001 D8의 `refresh-client -A '%<N>:pause'` (300ms 디바운스) 발급. |
| `0x06` | **PANE_RESUME** | %N | 0 bytes | 클라 → 서버 | `refresh-client -A '%<N>:continue'`. |
| `0x07` | **NOTIFY_MIRROR** | %N or 0 | UTF-8 JSON `{"kind": string, …}` | 서버 → 클라 | tmux 비-`%output` 알림 미러. `kind` enum 정의는 §2.3. 외부 attach·`%pause`(slow 배지)·`%pane-died` 등 모든 상태 변화. |
| `0x08`–`0x0F` | **reserved (tmux-domain)** | — | — | — | 미래 tmux-domain 확장용. 새 할당은 본 SSoT 갱신 + ADR-0002 amend. 사용 후보(미할당): scrollback fetch (`capture-pane` 결과 P1+), session-level subscription 추가, command queue depth telemetry. |

### 2.2 web-domain (0x80–0x8F)

| 코드 | 이름 | paneId | payload | 방향 | 비고 |
|---|---|---|---|---|---|
| `0x80` | **LAYOUT_CHANGED** | 0 | `etag (16B raw)` | 서버 → 모든 연결 | HTTP `PUT /api/layout` 성공 시 broadcast. 클라이언트는 신호 수신 → `GET /api/layout` 재발급으로 새 상태 확보 (Pull-through-notify). ETag 정규화는 `docs/ssot/canvas-layout-schema.md` §2 인용 — *16바이트 raw 정본, WS 구간 raw 그대로*. |
| `0x81` | **M_CHANGED** | 0 | `varint count + varint panel_ids[]` | 양방향 broadcast (MT-3) | Manipulation Selection 단일 진실. `count` = 선택된 panel 수, 이어서 그만큼의 varint pane id (`%N`의 N). 빈 선택은 `count = 0`. |
| `0x82` | **I_CHANGED** | 0 | `varint pane_id` (`0` = null = I 미설정) | 양방향 broadcast (MT-3) | Input Target 단일 진실. `0`은 *I 미설정 상태* sentinel (tmux는 0번 pane을 할당하지 않음). |
| `0x83` | **VIEWPORT_CHANGED** | 0 | `int32 x (LE) + int32 y (LE) + float32 zoom (IEEE-754 LE)` | 양방향 broadcast (MT-3) | Canvas viewport pan/zoom. x/y는 캔버스 좌표(픽셀), zoom은 multiplier(1.0 = 100%). 모든 연결 sync (멀티 모니터 분리 viewport 미지원, Grill D13). |
| `0x84` | **FOCUS_MODE_CHANGED** | 0 | `1B enabled (0 or 1) + varint target_panel_id` | 양방향 broadcast (MT-3) | Focus mode 토글. `enabled = 0`이면 `target_panel_id`는 의미 없음(관습적으로 0). `enabled = 1`이면 `target_panel_id` = `%N`의 N. |
| `0x85`–`0x8F` | **reserved (web-domain)** | — | — | — | 미래 web-domain 확장용. 새 할당은 본 SSoT 갱신 + ADR-0002 amend. 사용 후보(미할당): mini-map 상태(P1+), undo/redo 신호(P2), snap-to-grid 설정(P2), focus mode target group(P1+), keyboard shortcut state(P1+). |

### 2.3 `0x07 NOTIFY_MIRROR` 의 `kind` enum

`kind`는 ASCII 영문 소문자 + 하이픈 형식 (kebab-case). MVP 정의:

| `kind` 값 | paneId 의미 | 추가 JSON 필드 | tmux 원본 알림 |
|---|---|---|---|
| `"window-add"` | 0 | `{"window_id": "@N", "name": string}` | `%window-add` |
| `"window-renamed"` | 0 | `{"window_id": "@N", "name": string}` | `%window-renamed` |
| `"window-close"` | 0 | `{"window_id": "@N"}` | `%window-close` |
| `"session-changed"` | 0 | `{"session_id": "$N", "name": string}` | `%session-changed` |
| `"pane-died"` | %N | `{"exit_code": int?}` | `%pane-died` (pane_dead = 1) |
| `"pane-mode-changed"` | %N | `{"mode": string}` | `%pane-mode-changed` |
| `"layout-change"` | 0 | `{"window_id": "@N", "layout": string}` | `%layout-change` — *layout 문자열은 그대로 전달되나 클라이언트는 trigger로만 사용, 캔버스 좌표로 변환하지 않음* (불변식 #3 강제, ADR-0001 §검증). |
| `"subscription-changed"` | %N or 0 | `{"name": string, "value": string}` | `%subscription-changed` (D3 step 3 등록 포맷) |
| `"slow-pane"` | %N | `{}` | `%pause` (ADR-0001 D10 — UI는 panel header에 "느림" 배지) |

미정의 `kind` 수신 시 클라이언트는 *조용히 무시*(forward-compat). 서버는 미정의 `kind`를 *발신하지 않음* — 새 알림 추가 시 본 표를 먼저 갱신.

### 2.4 `0x01 CTRL` payload JSON 스키마

요청 (클라 → 서버):

```json
{
  "id": "uuid-v4",                  // client-generated, 응답 매칭용
  "cmd": "new-window",              // ADR-0008 §command allowlist 표 안 값만 허용
  "args": ["-t", "session-name"]    // 문자열 배열. shell 문자열·자유 형식 금지.
}
```

응답 (서버 → 클라):

```json
{ "id": "uuid-v4", "ok": true,  "result": { … } }
{ "id": "uuid-v4", "ok": false, "error": "string", "code": "ERR_ENUM" }
```

- 서버는 `cmd` 값이 ADR-0008 allowlist에 없으면 즉시 `{"ok":false, "code":"ERR_NOT_ALLOWED"}`로 거부 — tmux로 전달하지 않음.
- `args`는 *문자열 배열만*. 객체·중첩 배열·숫자는 거부.
- `id`는 client-generated UUID-v4. 서버는 `id`로 응답을 매칭만 하며 디버깅 추적 외 의미 없음.

## 3. 디코더 의사코드 (검증 + 라우팅)

```
fn decode(frame: &[u8]) -> Result<Envelope, CloseError> {
    if frame.is_empty() { return Err(CloseError::Code(1003)); }
    let type_byte = frame[0];
    let (pane_id, rest) = read_varint(&frame[1..])?;  // 5바이트 초과 시 Err 1003

    match type_byte {
        // tmux-domain
        0x01 => Envelope::Ctrl(parse_json_ctrl(rest)?),       // paneId must == 0
        0x02 => Envelope::PaneOut { pane_id, bytes: rest },
        0x03 => Envelope::PaneIn  { pane_id, bytes: rest },
        0x04 => Envelope::PaneResize { pane_id, cols: vread(rest)?, rows: vread(rest)? },
        0x05 => Envelope::PanePause  { pane_id },              // rest must be empty
        0x06 => Envelope::PaneResume { pane_id },              // rest must be empty
        0x07 => Envelope::NotifyMirror(parse_json_notify(rest)?),

        // web-domain
        0x80 => Envelope::LayoutChanged { etag: rest[..16].try_into()? },  // paneId == 0
        0x81 => Envelope::MChanged { panel_ids: read_varint_array(rest)? },
        0x82 => Envelope::IChanged { pane_id: vread(rest)? },
        0x83 => Envelope::ViewportChanged {
            x:    i32::from_le_bytes(rest[..4].try_into()?),
            y:    i32::from_le_bytes(rest[4..8].try_into()?),
            zoom: f32::from_le_bytes(rest[8..12].try_into()?),
        },
        0x84 => Envelope::FocusModeChanged {
            enabled: rest[0] != 0,
            target_panel_id: vread(&rest[1..])?,
        },

        // reserved
        0x08..=0x0F | 0x85..=0x8F => Err(CloseError::Code(1003)),  // 미할당
        _                          => Err(CloseError::Code(1003)),  // 정의되지 않은 카테고리
    }
}
```

- 라우팅 분기: `0x01..=0x07`은 *tmux-domain 핸들러*로, `0x80..=0x84`는 *web-domain broadcast 핸들러*로 dispatch — 두 핸들러는 코드/모듈을 공유하지 않음 (불변식 #1 강제).
- payload 길이 검증: 0x05/0x06은 rest = 0바이트, 0x80은 rest = 16바이트, 0x83은 rest = 12바이트, 0x84는 rest ≥ 2바이트 — 어긋나면 그 메시지만 거부 + `0x01 CTRL` 에러 응답.

## 4. "이 프로토콜에 절대 나타나서는 안 되는 데이터" 목록 (불변식 강제)

다음 데이터는 *어떤 envelope 타입의 페이로드에도* 등장하지 않는다. 등장하면 ADR-0002 위반 + 5대 불변식 위반.

| 데이터 | 채널 | 근거 |
|---|---|---|
| **Canvas geometry 문자열** — panel x/y/w/h/z 직렬화 | HTTP `PUT /api/layout`만 | ADR-0002 D9·D10, Grill D12. WS는 `0x80 LAYOUT_CHANGED` notify만. |
| **tmux Layout 문자열** — `select-layout`이 받는 split layout 표현 | 어디에도 없음 (운반 메시지 정의되지 않음) | ADR-0008 `select-layout` 발급 금지. 외부에서 layout이 바뀌면 `0x07 NOTIFY_MIRROR { kind: "layout-change" }`로 trigger만 미러, 문자열은 *불투명 식별자로 취급*. |
| **Shell 명령 문자열** — `bash -c '…'` 등 자유 형식 | 어디에도 없음 | ADR-0001 D5·ADR-0008 allowlist. 0x01 CTRL은 argv 배열만. |
| **자유 형식 tmux 명령 문자열** — `tmux new-window -t foo` 같은 한 줄 | 어디에도 없음 | 0x01 CTRL의 JSON `{"cmd": "new-window", "args": ["-t", "foo"]}` argv 분리만 허용. `cmd`는 ADR-0008 allowlist 안 값만, `args[]`는 문자열 배열만. |
| **Group 트리, Group label/color/visibility/locked/order** | HTTP `PUT /api/layout`만 | Grill D11·D12. WS는 LAYOUT_CHANGED notify만. |
| **Panel label, Panel note** | HTTP `PUT /api/layout`만 | Grill D11. 사용자 입력 → tmux로 절대 흐르지 않음 (인젝션 표면 차단). |
| **평문 인증 토큰 값** | `Sec-WebSocket-Protocol` 핸드셰이크 헤더만 | ADR-0002 D5·R5 거절. 핸드셰이크 외 envelope 페이로드 어디에도 토큰을 두지 않음. |
| **`client_id`, `origin_id`, `connection_id`** | 어디에도 없음 (정의되지 않음) | Grill D13 MT-3. 모든 web-domain 메시지는 connection identity 없이 broadcast. |
| **Sequence number, resume token** | 어디에도 없음 (MVP) | ADR-0002 D11. MT-3 idempotent + ring buffer replay + HTTP ETag로 대체. P1+ 재방문. |
| **`%output` 페이로드의 base64/escape 인코딩** | 0x02 PANE_OUT은 raw bytes만 | ADR-0001 D7 + R4 §4. 8진수 이스케이프 역치환은 *서버 측에서 완료*, WS 페이로드는 원바이트. |

## 5. 사용 예제 (디버깅용 헥스덤프)

**예제 1**: pane `%37`에서 "hello\n" 출력
```
type    paneId  payload
0x02    0x25    0x68 0x65 0x6c 0x6c 0x6f 0x0a
(7 bytes total: 1 + varint(37) + 6 ANSI bytes)
```

**예제 2**: 클라이언트가 pane `%37`을 I로 지정
```
type    paneId  payload
0x82    0x00    0x25
(3 bytes total: 1 + varint(0) + varint(37))
```

**예제 3**: 새 layout 적용 후 서버 broadcast (etag = 16바이트 raw)
```
type    paneId  payload (16B etag)
0x80    0x00    aa bb cc dd ee ff 00 11 22 33 44 55 66 77 88 99
(18 bytes total)
```

**예제 4**: viewport 갱신 (x=1024, y=-512, zoom=1.5)
```
type    paneId  x(LE)         y(LE)         zoom(LE)
0x83    0x00    00 04 00 00   00 fe ff ff   00 00 c0 3f
(14 bytes total)
```

**예제 5**: 클라이언트 → 서버 CTRL 명령 (new-window)
```
type    paneId  payload (UTF-8 JSON)
0x01    0x00    7b 22 69 64 22 3a … 7d
(payload = {"id":"…","cmd":"new-window","args":["-t","foo"]})
```

## 6. 호환성 정책

- **추가 호환성**: `0x08`–`0x0F`·`0x85`–`0x8F` 예약 슬롯에 새 타입을 할당할 때 본 SSoT를 먼저 갱신 + ADR-0002 amend. 클라이언트는 *알지 못하는 타입*을 수신하면 close하지 않고 *그 메시지만 무시*해야 함 (forward-compat) — *예약 슬롯 범위 안에서만*. 카테고리 밖(예: 0x10, 0xC0)은 close code 1003.
- **MT-3 변경**: connection identity 도입 시 (단일 사용자 정책 해제 P2+) 본 SSoT의 web-domain 표·"절대 나타나면 안 되는 데이터" 표를 동반 갱신.
- **WebTransport 마이그레이션 (P2+)**: 본 envelope 형식은 *transport 무관* — WebTransport 양방향 스트림 1개에 그대로 실어도 동작. ADR-0002 O4 참조.

## 7. 변경 이력

- 2026-05-13: 초안 (ADR-0002 부속, Grill D12·D13·D14·D15·D17·D19 입력 반영). 32개 슬롯 전부 정의: tmux-domain 0x01–0x07 + reserved 0x08–0x0F, web-domain 0x80–0x84 + reserved 0x85–0x8F.
