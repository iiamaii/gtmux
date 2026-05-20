# SSoT: Security Defaults

- 일자: 2026-05-13
- 정의 ADR: **ADR-0003 (보안 디폴트)**. 부가 입력: ADR-0007 (Server:Session:Port 1:1:1), ADR-0008 (single-pane + Group, command allowlist 정본), ADR-0009 (tmux daemon 격리, 소켓 경로 컨벤션), ADR-0010 (Group 데이터 모델, 식별자 정규식 정본은 `canvas-layout-schema.md`), ADR-0011 (Rust backend, crypto crate).
- 변경 정책: 본 SSoT는 startup에 Rust `auth` + `config` crate가 **직접 로딩**한다. 변경은 PR + ADR-0003 갱신 동반. 키 이름은 안정 식별자로 간주 (config TOML key·환경변수 매핑 모두 본 문서에서 파생).
- 관련 보고서: `docs/reports/0005-security-model.md` (R5, 12-item 정본), `docs/reports/0010-grill-amendments.md` D17·D20·D21·D22.

본 SSoT는 **flat key→value 디폴트 표**와 **모드별 derived value 규칙**을 정의한다. 사용자는 TOML config (`${XDG_CONFIG_HOME}/gtmux/<session>.config.toml`, D22)의 `[security]` 섹션에서 일부 값을 override할 수 있지만, 본 문서의 디폴트가 *명시 override가 없을 때의 fail-closed 동작*이다.

---

## 1. Flat key→value 디폴트 표

각 행은 `<key>` (= `[security].<key>` TOML 경로 또는 derived) | `<default>` | `<type>` | `<source>` | `<note>` 컬럼이다. `derived` 행은 사용자가 override 불가, startup에 `bind`/`port`에서 계산.

### 1.1 네트워크 바인딩

| key | default | type | source | note |
|---|---|---|---|---|
| `bind` | `"127.0.0.1"` | string | `[server].bind` (D22) | local/cloud mode 추론 base. 값 형식: IPv4·IPv6·`unix:/path/to/sock` |
| `port` | (required) | u16 (1024–65535) | `[server].port` (D22) | random-high-port는 사용자 명시. 충돌 시 exit 4 |
| `mode` (derived) | computed | enum `{local, cloud}` | ADR-0003 §D22 통합 | `bind ∈ {127.0.0.1, ::1, unix:/…}` → `local`; 그 외 → `cloud` |
| `socket_path_convention` | `"${XDG_RUNTIME_DIR:-/tmp}/gtmux/control.sock"` | string template | R5 §A.2 + D9 디렉터리 layout | unix socket 모드 시. 파일 perm 0600, 부모 dir 0700 |
| `tmux_socket_path_convention` | `"${TMUX_TMPDIR:-/tmp}/tmux-${uid}/gtmux-<session>"` | string template | ADR-0009 D2 | tmux daemon 측. `-L gtmux-<session>`이 자동 산출 |

### 1.2 헤더 화이트리스트

| key | default | type | source | note |
|---|---|---|---|---|
| `host_allowlist` | `["127.0.0.1:<port>", "localhost:<port>", "[::1]:<port>"]` | list<string> | D2 (R5 #2) | local 모드 자동 합성. cloud 모드는 사용자 명시 필수 (예: `["gtmux.example.com"]`) |
| `cors_origins` | `["http://localhost:<port>"]` | list<string> | D22 `[security].cors_origins` | local 디폴트. cloud는 `https://<public_host>` 명시 |
| `origin_allowlist` | = `cors_origins` | list<string> | D3 (R5 #3) | WebSocket handshake `Origin` 정확 일치. `null` 거부. wildcard 거부 |
| `trusted_proxy_ips` | `[]` (local) / required (cloud) | list<string CIDR> | D12 + R5 §F.2 | cloud 모드에서 `X-Forwarded-*` 신뢰 IP. 미설정 시 헤더 무시 |

### 1.3 토큰 (D13)

| key | default | type | source | note |
|---|---|---|---|---|
| `token_byte_length` | `32` | u8 | D13.2 (R5 #4) | 256-bit CSPRNG raw 길이 |
| `token_encoding` | `"base64url"` | enum `{base64url}` | D13.2 | 결과 문자열 길이 43 (패딩 제외) |
| `token_file_path` | `"${XDG_STATE_HOME:-~/.local/state}/gtmux/<session>.token"` | string template | D13.3 + D20 디렉터리 layout | 파일 perm 0600, 부모 dir 0700. 더 넓은 권한 시 startup exit 5 |
| `token_file_required_perm` | `0o600` | octal int | D13.3 (R5 §E.1) | startup check |
| `token_dir_required_perm` | `0o700` | octal int | D13.3 | startup check |
| `token_rotation_policy` (derived) | local→`"on_start"`, cloud→`"persist_explicit_rotate"` | enum | D13.1 (D17) | `gtmux rotate-token --session <name>`로 cloud 회전 |
| `token_compare` | `"constant_time"` | enum `{constant_time}` | D13.4 (ADR-0011 D8) | `ring::constant_time::verify_slices_are_equal` |
| `token_csprng` | `"ring::rand::SystemRandom"` | string | ADR-0011 D8 | 1순위. R7-T2 결과로 대안 확정 |
| `ws_subprotocol_advertise` | `["gtmux.v1", "bearer.<base64url-token>"]` | string list | D5 | 클라가 advertise하는 두 값 (콤마-구분, RFC 6455 §11.3.4) |
| `ws_subprotocol_echo` | `"gtmux.v1"` | string | D5 | 서버가 echo하는 단일 토큰 (Kubernetes PR #47740 패턴) |
| `http_auth_scheme` | `"Bearer"` | enum `{Bearer}` | D6 | `Authorization: Bearer <token>` |
| `http_cookie_name` | `"gtmux_session"` | string | D6 | 보조 cookie 이름 |
| `http_cookie_attrs` | `["HttpOnly", "Secure", "SameSite=Strict", "Path=/"]` | list<string> | D6 (R5 #6) | cloud는 `Secure` 강제. local HTTP는 `Secure` 생략 가능 (mode-derived) |
| `ws_close_code_token_revoked` | `4001` | u16 | D13.1 + D21 c7 | RFC 6455 + custom. 토큰 회전 시 즉시 close |
| `ws_close_code_session_killed` | `4002` | u16 | D21 grill amendments | 외부 session kill 시 |

### 1.4 CSP (D11 · D15)

| key | default | type | source | note |
|---|---|---|---|---|
| `csp_template_local` | (아래 §1.4.1) | string | D15.1 + R5 §D.5 | nonce는 응답마다 동적 치환 |
| `csp_template_cloud` | (아래 §1.4.2) | string | D15.2 | `<configured-host>`는 startup에 1회 치환 (host_allowlist[0] 또는 향후 `[cloud].public_host`) |
| `csp_nonce_byte_length` | `16` | u8 | D15.3 | `ring::rand` 16 byte → base64 (22자) |
| `csp_use_strict_dynamic` | `true` | bool | D11 (R5 §D.5) | `'strict-dynamic'` 활성 |

#### 1.4.1 `csp_template_local`

```
default-src 'none'; script-src 'nonce-{NONCE}' 'strict-dynamic'; style-src 'self' 'nonce-{NONCE}'; img-src 'self' data:; font-src 'self'; connect-src 'self'; worker-src 'self'; base-uri 'none'; object-src 'none'; frame-ancestors 'none'; form-action 'none'
```

#### 1.4.2 `csp_template_cloud`

```
default-src 'none'; script-src 'nonce-{NONCE}' 'strict-dynamic'; style-src 'self' 'nonce-{NONCE}'; img-src 'self' data:; font-src 'self'; connect-src 'self' wss://{CONFIGURED_HOST}; worker-src 'self'; base-uri 'none'; object-src 'none'; frame-ancestors 'none'; form-action 'none'
```

Placeholder 치환 규칙:
- `{NONCE}` — 응답당 1회 동적 (csp_nonce_byte_length → base64)
- `{CONFIGURED_HOST}` — startup 1회. `[cloud].public_host` 우선, 없으면 `host_allowlist[0]`

### 1.5 응답 헤더 (D11 표 정본)

| key | default value | type | mode 조건 | source |
|---|---|---|---|---|
| `header.x_content_type_options` | `"nosniff"` | string | both | D11 |
| `header.referrer_policy` | `"no-referrer"` | string | both | D11 |
| `header.cross_origin_opener_policy` | `"same-origin"` | string | both | D11 |
| `header.cross_origin_resource_policy` | `"same-origin"` | string | both | D11 |
| `header.permissions_policy` | `"camera=(), microphone=(), geolocation=(), interest-cohort=()"` | string | both | D11 |
| `header.strict_transport_security` | `"max-age=31536000; includeSubDomains"` | string | **cloud only** | D11 (R5 §F.1) |
| `header.cross_origin_embedder_policy` | unset (default) | string\|null | both | R5 §F.1 — SharedArrayBuffer 필요 시만 `require-corp` 추가. MVP 미부착 |

### 1.6 xterm.js 옵션 (D9)

| key | default | type | source | note |
|---|---|---|---|---|
| `xterm.allow_proposed_api` | `false` | bool | D9 (R5 §D.1) | v5+ 기본 유지 |
| `xterm.scrollback` | `1000` | u32 | D9 | 메모리 압박 시 더 작게 |
| `xterm.osc8.enabled` | `true` | bool | D9 (R5 §D.2) | linkHandler 등록은 필수 |
| `xterm.osc8.allow_non_http_protocols` | `false` | bool | D9 | `http`/`https`만 허용 |
| `xterm.osc8.modifier_required` | `"Ctrl_or_Cmd"` | enum | D9 | 클릭 시 모디파이어 필요 |
| `xterm.osc8.show_full_url_on_hover` | `true` | bool | D9 | |
| `xterm.osc52.clipboard_write` | `false` | bool | D9 (R5 §D.3) | **auto-enable 금지** (R(rej)5). 명시 사용자 동의 시에만 |
| `xterm.disable_stdin_logging` | `true` | bool | R5 §F.3 | stdin 페이로드 절대 로깅 안 함 |

### 1.7 tmux command blocklist · allowlist (ADR-0008 정본 인용)

발급 가능 (allowlist — ADR-0008 §"tmux command allowlist 표" 정본):
```
new-window -t <session>
kill-pane -t %<pid>
kill-window -t @<wid>
rename-window -t @<wid> <label>
send-keys -t %<pid>
refresh-client -A '%<pid>:pause' | '%<pid>:continue'
refresh-client -B <subscription>
capture-pane -p -e -J -S -<lines>
list-sessions -F <fmt>
list-windows -a -F <fmt>
list-panes -a -F <fmt>
```

발급 금지 (blocklist — ADR-0008 D2 + R5 §C.4 + 본 SSoT 추가):

| 명령 | 차단 사유 | 출처 |
|---|---|---|
| `split-window` | single-pane-per-window 컨벤션 위반 | ADR-0008 D2 |
| `resize-pane` | window-size = pane-size, 별도 명령 불필요 | ADR-0008 D2 |
| `select-layout` | tmux Layout 능동 변경 금지 (§4.1.3 정신) | ADR-0008 D2 |
| `if-shell` | 임의 셸 명령 실행 — R5 §C.4 high-risk | R5 §C.4 |
| `run-shell` | 임의 셸 명령 실행 | R5 §C.4 |
| `source-file` | 임의 tmux 설정 로드 → 우회 표면 | R5 §C.4 |
| `pipe-pane` | pane 출력 임의 명령 파이프 | R5 §C.4 |
| `-CC` (control mode variant) | DCS 래핑은 터미널 에뮬레이터용; backend는 `-C`만 사용 | ADR-0008 |

`-F` 포맷 문자열은 *서버 상수만* 허용. 사용자 입력이 `-F` 인자 위치에 도달하는 코드 경로는 컴파일 타임에 차단(D8).

### 1.8 argv injection guard (D14 fallback)

| key | default | type | source | note |
|---|---|---|---|---|
| `argv.use_end_of_options_separator` | `true` | bool | D14.3 | 모든 tmux 호출 시 `--` 삽입 |
| `argv.reject_dash_leading_user_input` | `true` | bool | D14.3 | 사용자 유래 위치 인자가 `-` 또는 `--`로 시작하면 거부 (D14.2 실측 전 보수 정책) |
| `argv.per_command_schema_fallback` | `true` | bool | D14.1 | D14.2 실측에서 `--` honor 실패 명령 발견 시 strict per-command schema 자동 활성 |
| `argv.shell_invocation` | `"forbidden"` | enum | D7 (R(rej)1) | `tokio::process::Command::arg`만 허용. `sh -c "…"` 컴파일 타임 금지 |
| `argv.format_string_user_input` | `"forbidden"` | enum | D8 | `-F` 위치에 user input 도달 컴파일 타임 차단 |

### 1.9 식별자 정규식 (D8, canvas-layout-schema §1 정합)

| key | regex | source |
|---|---|---|
| `regex.tmux_session_id` | `^\\$[0-9]+$` | D8 (R5 §C.3) |
| `regex.tmux_window_id` | `^@[0-9]+$` | D8 |
| `regex.tmux_pane_id` | `^%[0-9]+$` | D8 + canvas-layout-schema §1 |
| `regex.tmux_session_name` | `^[^\\x00-\\x1f\\x7f]{1,64}$` | D8 (제어문자 금지, 길이 1–64) |
| `regex.tmux_window_name` | `^[^\\x00-\\x1f\\x7f]{1,64}$` | D8 |
| `regex.canvas_group_id` | `^g[0-9a-zA-Z]{1,32}$` | canvas-layout-schema §1 |
| `regex.canvas_panel_id` | `^p[0-9a-zA-Z]{1,32}$` | canvas-layout-schema §1 |
| `regex.canvas_color_hex` | `^#[0-9a-fA-F]{6}$` | canvas-layout-schema §1 |
| `regex.etag_hex32` | `^[0-9a-f]{32}$` | canvas-layout-schema §2 (lowercase 강제) |

### 1.10 운영·기동 가드

| key | default | type | source | note |
|---|---|---|---|---|
| `process.allow_root` | `false` | bool | R(rej)6 (R5 §E.2) | EUID==0 + `--allow-root` 명시 없으면 exit 5 |
| `process.umask` | `0o077` | octal | R5 §E.1 | 새로 만든 파일/디렉터리 자동 0600/0700 |
| `auth.rate_limit_failures_per_minute` (cloud only) | `2` | u8 | D12 (R5 §F.4, code-server 수준) | local 모드 미적용 |
| `auth.rate_limit_failures_per_hour` (cloud only) | `14` | u8 | D12 | |
| `logging.redact_fields` | `["token", "authorization", "cookie", "stdin_payload"]` | list<string> | R5 §F.3 + ADR-0011 D7 | `***REDACTED***` 마스킹 |
| `logging.audit_min_events` | `["auth_failure", "command_palette_invocation", "external_bind_activation"]` | list<string> | R5 §F.3 + ADR-0003 O6 | 최소 감사 — 구체 이벤트 set은 R7에서 확정 |
| `xdg.config_home` | `"${XDG_CONFIG_HOME:-~/.config}/gtmux"` | string template | D20 + R5 §E.1 | dir perm 0700 |
| `xdg.state_home` | `"${XDG_STATE_HOME:-~/.local/state}/gtmux"` | string template | D20 | token, layout |
| `xdg.runtime_dir` | `"${XDG_RUNTIME_DIR:-/tmp}/gtmux"` | string template | D20 | PID, unix socket |

### 1.11 cloud 모드 추가 (D12)

| key | default | type | source | note |
|---|---|---|---|---|
| `cloud.tls_required` | `true` | bool | D12 (R5 §A.4) | HTTPS·WSS만. HTTP 부팅 시 stderr 경고 + 명시 `--allow-cloud-without-tls` 없으면 거부 |
| `cloud.reverse_proxy_required` | `true` | bool | D12 (R5 §A.4) | Caddy/nginx + ACME 권장. gtmux 자체 TLS 종단 미지원 (MVP) |
| `cloud.hsts_enabled` | `true` | bool | D12 + R5 §F.1 | `Strict-Transport-Security` 자동 부착 |
| `cloud.trusted_proxy_ips_required` | `true` | bool | D12 (R5 §F.2) | 미설정 시 `X-Forwarded-*` 헤더 전체 무시 (fail-closed) |
| `cloud.public_host` | unset (P1+) | string\|null | ADR-0003 O4 | 향후 확장. 현 MVP는 `host_allowlist[0]` 차용 |

---

## 2. TOML 직접 로딩 형태 (`[security]` 섹션 정본)

본 섹션은 D22 config 파일에서 *사용자가 override 가능한* 키를 보인다. 미설정 시 §1의 디폴트가 그대로 적용된다. 알 수 없는 필드는 startup에서 거부(`figment` + serde `deny_unknown_fields`, ADR-0011 R7-T6).

```toml
[security]
# 자동 합성 디폴트:
# host_allowlist = ["127.0.0.1:<port>", "localhost:<port>", "[::1]:<port>"]
# cors_origins   = ["http://localhost:<port>"]
host_allowlist = ["localhost:9001", "127.0.0.1:9001"]
cors_origins   = ["http://localhost:9001"]
# origin_allowlist는 미설정 시 cors_origins로 합성

# 토큰 (대부분 derived — local/cloud 자동)
# token_file_path = "${XDG_STATE_HOME}/gtmux/<session>.token"  # 디폴트 유지 권장

# CSP — 템플릿은 ADR-0003 SSoT(`docs/ssot/security-defaults.md` §1.4)가 정본.
# 사용자가 override하면 startup에 위 정본과 diff를 stderr에 경고로 출력.
# csp_local_override = "…"
# csp_cloud_override = "…"

# 응답 헤더 override (예: COEP 켤 때)
# header_overrides = { cross_origin_embedder_policy = "require-corp" }

# 운영
# allow_root              = false   # 컴파일 타임 기본; CLI `--allow-root`만이 override
# rate_limit_failures_per_minute = 2  # cloud 모드 전용
# rate_limit_failures_per_hour   = 14

[cloud]
# bind가 loopback/unix가 아닐 때만 활성.
# tls_required = true
# trusted_proxy_ips = ["192.0.2.10/32"]
# public_host = "gtmux.example.com"
```

---

## 3. JSON 직렬화 형태 (Rust `auth` crate가 직접 `serde_json::from_str` 가능)

D22가 사용자 편집 포맷으로 TOML을 채택했지만, Rust `auth` crate가 startup에 `config` crate로 TOML → struct 변환 후 *런타임 내부 형태*는 JSON 친화적 struct로 잡힌다. 본 §3은 그 정본 schema의 JSON 시드(테스트 픽스처·디폴트 직렬화)이다.

```json
{
  "schema_version": 1,
  "mode": "local",
  "bind": "127.0.0.1",
  "port": 9001,
  "socket_path_template": "${XDG_RUNTIME_DIR}/gtmux/control.sock",

  "host_allowlist": ["127.0.0.1:9001", "localhost:9001", "[::1]:9001"],
  "origin_allowlist": ["http://localhost:9001"],
  "cors_origins": ["http://localhost:9001"],
  "trusted_proxy_ips": [],

  "token": {
    "byte_length": 32,
    "encoding": "base64url",
    "file_path_template": "${XDG_STATE_HOME}/gtmux/<session>.token",
    "file_required_perm": 384,
    "dir_required_perm": 448,
    "rotation_policy": "on_start",
    "compare": "constant_time",
    "csprng": "ring::rand::SystemRandom"
  },

  "ws": {
    "subprotocol_advertise": ["gtmux.v1", "bearer.<base64url-token>"],
    "subprotocol_echo": "gtmux.v1",
    "close_code_token_revoked": 4001,
    "close_code_session_killed": 4002
  },

  "http_auth": {
    "scheme": "Bearer",
    "cookie_name": "gtmux_session",
    "cookie_attrs": ["HttpOnly", "Secure", "SameSite=Strict", "Path=/"]
  },

  "csp": {
    "template_local": "default-src 'none'; script-src 'nonce-{NONCE}' 'strict-dynamic'; style-src 'self' 'nonce-{NONCE}'; img-src 'self' data:; font-src 'self'; connect-src 'self'; worker-src 'self'; base-uri 'none'; object-src 'none'; frame-ancestors 'none'; form-action 'none'",
    "template_cloud": "default-src 'none'; script-src 'nonce-{NONCE}' 'strict-dynamic'; style-src 'self' 'nonce-{NONCE}'; img-src 'self' data:; font-src 'self'; connect-src 'self' wss://{CONFIGURED_HOST}; worker-src 'self'; base-uri 'none'; object-src 'none'; frame-ancestors 'none'; form-action 'none'",
    "nonce_byte_length": 16,
    "use_strict_dynamic": true
  },

  "headers": {
    "x_content_type_options": "nosniff",
    "referrer_policy": "no-referrer",
    "cross_origin_opener_policy": "same-origin",
    "cross_origin_resource_policy": "same-origin",
    "permissions_policy": "camera=(), microphone=(), geolocation=(), interest-cohort=()",
    "strict_transport_security": null,
    "cross_origin_embedder_policy": null
  },

  "xterm": {
    "allow_proposed_api": false,
    "scrollback": 1000,
    "osc8": {
      "enabled": true,
      "allow_non_http_protocols": false,
      "modifier_required": "Ctrl_or_Cmd",
      "show_full_url_on_hover": true
    },
    "osc52": { "clipboard_write": false },
    "disable_stdin_logging": true
  },

  "tmux_commands": {
    "allowlist": [
      "new-window", "kill-pane", "kill-window", "rename-window",
      "send-keys", "refresh-client", "capture-pane",
      "list-sessions", "list-windows", "list-panes"
    ],
    "blocklist": [
      "split-window", "resize-pane", "select-layout",
      "if-shell", "run-shell", "source-file", "pipe-pane"
    ]
  },

  "argv_guard": {
    "use_end_of_options_separator": true,
    "reject_dash_leading_user_input": true,
    "per_command_schema_fallback": true,
    "shell_invocation": "forbidden",
    "format_string_user_input": "forbidden"
  },

  "regex": {
    "tmux_session_id":   "^\\$[0-9]+$",
    "tmux_window_id":    "^@[0-9]+$",
    "tmux_pane_id":      "^%[0-9]+$",
    "tmux_session_name": "^[^\\x00-\\x1f\\x7f]{1,64}$",
    "tmux_window_name":  "^[^\\x00-\\x1f\\x7f]{1,64}$",
    "canvas_group_id":   "^g[0-9a-zA-Z]{1,32}$",
    "canvas_panel_id":   "^p[0-9a-zA-Z]{1,32}$",
    "canvas_color_hex":  "^#[0-9a-fA-F]{6}$",
    "etag_hex32":        "^[0-9a-f]{32}$"
  },

  "process": {
    "allow_root": false,
    "umask": 63
  },

  "auth_rate_limit": {
    "failures_per_minute": 2,
    "failures_per_hour": 14
  },

  "logging": {
    "redact_fields": ["token", "authorization", "cookie", "stdin_payload"],
    "audit_min_events": ["auth_failure", "command_palette_invocation", "external_bind_activation"]
  },

  "xdg": {
    "config_home_template":  "${XDG_CONFIG_HOME:-~/.config}/gtmux",
    "state_home_template":   "${XDG_STATE_HOME:-~/.local/state}/gtmux",
    "runtime_dir_template":  "${XDG_RUNTIME_DIR:-/tmp}/gtmux"
  },

  "cloud": {
    "tls_required": true,
    "reverse_proxy_required": true,
    "hsts_enabled": true,
    "trusted_proxy_ips_required": true,
    "public_host": null
  }
}
```

권한 숫자 표기(`384`=0o600, `448`=0o700, `63`=0o077)는 JSON이 octal 리터럴을 지원하지 않아 decimal로 변환한 결과. Rust struct 디코딩 시 `mode_t` 또는 wrapper type으로 해석한다.

---

## 4. Mode 자동 추론 규칙 (D22)

```
fn derive_mode(bind: &str) -> Mode {
    if bind == "127.0.0.1"
        || bind == "::1"
        || bind.starts_with("unix:")
    {
        Mode::Local
    } else {
        // 0.0.0.0, 외부 IP, 도메인 이름 등 모두 cloud
        Mode::Cloud
    }
}
```

Derived effect:
- `mode == Local`:
  - `token.rotation_policy = on_start`
  - `cloud.*` 키 무시
  - `headers.strict_transport_security` 부착 안 함
  - CSP = `csp.template_local`
  - rate limit 미적용
  - HTTP cookie `Secure` 속성 생략 가능 (TLS 없으므로)
- `mode == Cloud`:
  - `token.rotation_policy = persist_explicit_rotate`
  - `cloud.tls_required = true` → TLS 없으면 startup에서 `--allow-cloud-without-tls` 검사 후 거부
  - `cloud.trusted_proxy_ips_required = true` → 미설정 시 `X-Forwarded-*` 헤더 무시
  - HSTS 자동 부착
  - CSP = `csp.template_cloud` (`{CONFIGURED_HOST}` 치환)
  - 인증 실패 레이트 리밋 활성

---

## 5. Startup 검증 체크리스트 (구현 reference)

Rust `auth` + `config` crate가 startup에 *순서대로* 검증:

1. **schema_version == 1** — 다르면 exit 2 (사용법 오류)
2. **`bind` parse 성공** — IPv4/IPv6/unix path. 실패 시 exit 2
3. **`port` 1024–65535 + 미사용** — 충돌 시 exit 4
4. **`token_file_path` 존재 시 perm == 0o600** — 더 넓으면 exit 5 (R5 §E.1, fail-closed)
5. **`token_file_path` 부모 dir perm == 0o700** — 더 넓으면 exit 5
6. **EUID != 0** 또는 `--allow-root` 플래그 명시 — 위반 시 exit 5
7. **mode 도출** (§4)
8. **mode == Cloud 추가 검증**:
   - `cloud.tls_required` AND TLS 종단 없음 → exit 2 (배포 가이드 안내) unless `--allow-cloud-without-tls`
   - `cloud.trusted_proxy_ips_required` AND `trusted_proxy_ips == []` → stderr 경고 + `X-Forwarded-*` 무시 모드로 진행
   - `host_allowlist == ["127.0.0.1:<port>", …]` 자동 디폴트 그대로 → exit 2 (cloud는 명시 필수)
9. **`host_allowlist` 비어 있지 않음** — 비어 있으면 모든 HTTP 거부 → 의도된 거부면 명시 `[]` 허용, 빈 디폴트는 exit 2
10. **`origin_allowlist` `null` / wildcard 포함 안 함** — 위반 시 exit 2
11. **CSP 템플릿에 placeholder 1개 이상 ({NONCE})** — 미포함이면 exit 2
12. **regex 컴파일 성공** — 실패 시 exit 1
13. **token 발급/로드** — `on_start`는 새 token + 파일 작성, `persist_explicit_rotate`는 파일 로드, 없으면 첫 발급. 모두 0600·constant-time
14. **부팅 콘솔 출력** (D21 c1):
    - tty: banner(session·port·url·token·log path·cold start time)
    - pipe 또는 `--log-format json`: `{"event":"ready", …}`

검증 실패 시 *fail-closed* — 절대 부분 기동하지 않는다.

---

## 6. 변경 이력

- 2026-05-13: 초안 (ADR-0003 동반 발행). R5 12-item + D17/D20/D21/D22 통합 + reviewer flag 2개 결정 흡수. schema_version = 1.
