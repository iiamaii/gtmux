//! gtmux-config — figment 기반 TOML + env + CLI 오버레이 로더.
//!
//! D22 schema(`schema_version` / `server` / `runtime` / `security` / `cloud`)를
//! 그대로 미러하고, ADR-0003 / `docs/ssot/security-defaults.md` §1·§3의 fail-closed
//! 기본값을 반영한다. ADR-0011 R7-T6의 결정대로 figment 0.10을 채택하고
//! `deny_unknown_fields`로 오타를 startup에 거부한다.
//!
//! 선행순위 (R7-T6 §7 sketch와 동일):
//!   CLI `path` 인자 → `GTMUX_*` env (`__` 구분자) → TOML 파일 → 빌트인 디폴트
//!
//! 추가로 `session` 인자가 `[server].session`을 무조건 덮어쓴다 (D21 c6 —
//! port-only lookup 또는 `--session` flag가 config의 session 값을 *불일치 가드*
//! 없이 *대체*해야 하는 경우).
//!
//! 본 crate는 *문법 + 자료형* 검증과 mode 추론까지만 책임진다. 토큰 perm /
//! TLS 종단 / EUID==0 등의 *런타임 환경* 검증(ADR-0003 §5 startup 체크리스트)
//! 은 `auth` / `lifecycle` crate의 몫이다.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::path::{Path, PathBuf};

use figment::{
    providers::{Env, Format, Serialized, Toml},
    Figment,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// 본 crate가 이해하는 config 스키마 버전. SSoT(`security-defaults.md`)의
/// `schema_version`과 동일. 다른 값을 만나면 startup에서 거부한다.
pub const SCHEMA_VERSION: u32 = 1;

/// privileged port (`< 1024`)는 거부한다 — R(rej)6 EUID==0 정책과 정합.
const MIN_PORT: u16 = 1024;

/// `bind` 값을 보고 mode 추론에 쓰이는 loopback host 목록 — SSoT §4 정의 그대로.
const LOOPBACK_HOSTS: &[&str] = &["127.0.0.1", "::1", "localhost"];

// ─────────────────────────────────────────────────────────────────────────────
//  Mode
// ─────────────────────────────────────────────────────────────────────────────

/// `bind` 값에서 자동 추론되는 운영 모드. D22 — 별도 필드가 아니라 *derive*.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// loopback / unix 소켓 bind. 토큰 매시작 재발급, TLS·HSTS 미부착.
    Local,
    /// 외부 IP / 도메인 bind. 영속 토큰 + 명시 회전, TLS·HSTS 강제.
    Cloud,
}

/// `bind` 문자열에서 mode를 추론한다. SSoT §4의 정본 규칙.
///
/// loopback IPv4 / IPv6 / `localhost` / `unix:` prefix → Local, 그 외(0.0.0.0,
/// 외부 IP, 도메인 등) → Cloud. 사용자 실수로 외부 노출되었을 때 cloud 정책
/// (TLS·HSTS·rate limit)이 자동 발동되도록 fail-closed로 설계됐다.
pub fn derive_mode(bind: &str) -> Mode {
    if bind.starts_with("unix:") {
        return Mode::Local;
    }
    if LOOPBACK_HOSTS.iter().any(|h| h.eq_ignore_ascii_case(bind)) {
        return Mode::Local;
    }
    Mode::Cloud
}

// ─────────────────────────────────────────────────────────────────────────────
//  Schema structs
// ─────────────────────────────────────────────────────────────────────────────

/// gtmux Server 최상위 config. D22 schema의 1:1 mirror.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// 본 파일이 따르는 schema 버전. `SCHEMA_VERSION`과 일치해야 한다.
    pub schema_version: u32,
    /// 서버 identity (session·port·bind). ADR-0007 1:1:1.
    pub server: ServerConfig,
    /// 런타임 튜닝 값. 기본은 모두 SSoT가 정한 안전한 값.
    #[serde(default)]
    pub runtime: RuntimeConfig,
    /// 보안 화이트리스트. 기본은 빈 셋이며 `auth` crate가 mode·port와 합쳐 합성.
    #[serde(default)]
    pub security: SecurityConfig,
    /// Cloud 모드에서만 활성. Local 모드면 `None` 이어야 한다.
    #[serde(default)]
    pub cloud: Option<CloudConfig>,
    /// 옵션: 빌드된 SPA 디렉터리 경로. 설정되면 http-api 라우터가 fallback
    /// 으로 정적 파일을 서빙하여 동일 포트에서 API + UI를 함께 노출한다.
    /// `gtmux start` 시 `GTMUX_FRONTEND_DIST` env 또는 TOML 필드로 지정.
    #[serde(default)]
    pub frontend_dist: Option<std::path::PathBuf>,
    /// Workspace storage 디렉터리의 절대 경로 (ADR-0019 D2). `None` 이면
    /// boot 시 `${XDG_DATA_HOME:-~/.local/share}/gtmux/workspace/` 가 기본값.
    /// CLI `--workspace <path>` 는 본 필드보다 우선 — runtime 변경 불가
    /// (ADR-0019 D11, boot-immutable).
    #[serde(default)]
    pub workspace_path: Option<std::path::PathBuf>,
    /// Cookie-기반 인증 lifecycle 설정 (ADR-0020). 생략 시 default —
    /// `token` mode, 7d rolling, rate limit 5/5min.
    #[serde(default)]
    pub auth: AuthConfig,
}

/// `[server]` 섹션 — Server identity 영역.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerConfig {
    /// tmux session 이름 (ADR-0007). 부팅 immutable.
    pub session: String,
    /// 포트 번호. D21 c6의 영속 식별자. 1024–65535만 허용.
    pub port: u16,
    /// bind 주소. 값은 IPv4 / IPv6 / `unix:/path/...`. `derive_mode`의 입력.
    pub bind: String,
}

/// `[runtime]` 섹션 — 디바운스·로그·ring buffer 등 동작 파라미터.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeConfig {
    /// Per-pane ring buffer 크기 (KB). D15 기본 128 KB / pane.
    pub ring_buffer_size_kb: u32,
    /// Canvas Layout PUT 디바운스 (ms). D12 기본 300.
    pub layout_debounce_ms: u32,
    /// Panel Streaming State 토글 디바운스 (ms). D16 기본 300.
    pub panel_state_debounce_ms: u32,
    /// `tracing` log level — `trace|debug|info|warn|error|off`.
    pub log_level: String,
    /// log format — `auto` (tty=text / pipe=json) | `text` | `json`.
    /// `auto`의 실제 분기는 logger init이 결정한다 (D20 §"구현 디테일").
    pub log_format: String,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            ring_buffer_size_kb: 128,
            layout_debounce_ms: 300,
            panel_state_debounce_ms: 300,
            log_level: "info".to_string(),
            log_format: "auto".to_string(),
        }
    }
}

/// `[security]` 섹션 — CORS·Host 화이트리스트. 다른 보안 키는 SSoT가 정한
/// 디폴트를 `auth` crate가 부팅 시 합성한다.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecurityConfig {
    /// `Access-Control-Allow-Origin` 화이트리스트. wildcard 거부 (D3).
    pub cors_origins: Vec<String>,
    /// `Host` 헤더 화이트리스트. 비어 있으면 startup에서 bind 호스트로 보강.
    pub host_allowlist: Vec<String>,
}

/// `[auth]` 섹션 — Cookie 기반 인증 lifecycle (ADR-0020).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthConfig {
    /// `"token"` 또는 `"password"`. Default: `"token"` (현 정책 유지, ADR-0020 D1).
    #[serde(default = "default_auth_mode")]
    pub mode: String,
    /// Cookie 의 max-age in days (ADR-0020 D2/D3). Default 7, range 1–30.
    /// Rolling renewal: 매 valid request 마다 `last_seen` + `expires_at` 갱신.
    #[serde(default = "default_cookie_max_age_days")]
    pub cookie_max_age_days: u32,
    /// Per-IP rate limit (실패 시도 / 5분) — Password mode 의 brute-force 방어
    /// (ADR-0020 D5). Default 5.
    #[serde(default = "default_rate_limit_per_5min")]
    pub rate_limit_per_5min: u32,
}

fn default_auth_mode() -> String {
    "token".to_string()
}
fn default_cookie_max_age_days() -> u32 {
    7
}
fn default_rate_limit_per_5min() -> u32 {
    5
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            mode: default_auth_mode(),
            cookie_max_age_days: default_cookie_max_age_days(),
            rate_limit_per_5min: default_rate_limit_per_5min(),
        }
    }
}

/// `[cloud]` 섹션 — Cloud 모드에서만 의미가 있는 키. Local 모드는 `None`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CloudConfig {
    /// Cloud mode 에서 HTTPS/WSS 종단을 요구할지 여부. 생략 시 `true`.
    /// `false` 는 신뢰된 네트워크에서 평문 HTTP 를 명시적으로 허용하는 escape hatch.
    #[serde(default = "default_tls_required")]
    pub tls_required: bool,
    /// PEM 인증서 경로. 파일 존재 + 0600 perm 검증은 lifecycle crate가 수행.
    #[serde(default)]
    pub tls_cert: PathBuf,
    /// PEM 비밀키 경로.
    #[serde(default)]
    pub tls_key: PathBuf,
    /// 분당 인증 실패 허용 횟수 (SSoT §1.10). 기본 10 — code-server 대비
    /// 약간 관대하나 grill D22에서 명시 키로 두기로 결정.
    pub rate_limit_auth_failures_per_minute: u32,
}

fn default_tls_required() -> bool {
    true
}

// ─────────────────────────────────────────────────────────────────────────────
//  Errors
// ─────────────────────────────────────────────────────────────────────────────

/// `load()` 단계에서 발생할 수 있는 모든 오류. exit code는 호출자(CLI)가
/// 매핑한다 — 본 crate는 분류만 책임진다.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// TOML 파일을 읽거나 파싱하지 못함. (figment가 IO·파싱을 동일 errno로
    /// 묶어 보고하므로 분기 없이 단일 variant로 둔다.)
    #[error("config IO/parse error: {0}")]
    Io(String),

    /// figment provider 체인 단계에서 직렬화·역직렬화 실패.
    #[error("config parse error: {0}")]
    Parse(String),

    /// schema_version·port 범위·필수 필드 등 *값* 수준 검증 실패.
    #[error("config validation error: {0}")]
    Validation(String),

    /// `bind` 값이 cloud인데 `[cloud]` 섹션이 없거나, 반대로 local인데
    /// `[cloud]`가 있는 등 mode-section 불일치.
    #[error("mode mismatch: {0}")]
    ModeMismatch(String),

    /// `deny_unknown_fields`가 거부한 오타 필드.
    #[error("unknown field in config: {0}")]
    UnknownField(String),
}

impl From<figment::Error> for ConfigError {
    fn from(err: figment::Error) -> Self {
        // figment::Error는 kind enum이 풍부하지만 본 crate가 노출하는 분류는
        // 4종이면 충분 — 오타와 일반 파싱만 갈라준다.
        let msg = err.to_string();
        for e in err.into_iter() {
            if matches!(e.kind, figment::error::Kind::UnknownField(_, _)) {
                return ConfigError::UnknownField(msg);
            }
        }
        ConfigError::Parse(msg)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  load()
// ─────────────────────────────────────────────────────────────────────────────

/// figment chain을 빌드해 `Config`를 산출한다.
///
/// * `path` — TOML 파일 경로. `Some`이면 그 파일을 읽고, `None`이면 기본값만으로
///   진행한다. 파일이 *지정됐는데 없으면* `ConfigError::Io`.
/// * `session` — CLI / port-lookup이 산출한 session 이름. 비어 있지 않으면
///   `[server].session`을 무조건 override한다.
///
/// 선행순위 — `Serialized::defaults(...)` (빌트인) ◀ TOML ◀ Env ◀ CLI override.
/// figment merge 순서가 뒤로 갈수록 우선이므로 코드 순서도 동일.
pub fn load(path: Option<&Path>, session: &str) -> Result<Config, ConfigError> {
    load_with_overrides(path, session, None)
}

/// `load`와 동일하지만 CLI `--port` override를 figment chain의 마지막 layer로
/// 합류시킨 뒤 validation을 돌린다. TOML이 없거나 `[server].port`가 비어 있어
/// 빌트인 sentinel(port=0)이 살아남는 경우에도, CLI에서 명시한 port가 있으면
/// 그 값이 validate를 통과한다 — 그렇지 않으면 `--port 9999`를 줘도 load
/// 단계에서 `port must be in [1024, 65535], got 0`으로 죽는다.
pub fn load_with_overrides(
    path: Option<&Path>,
    session: &str,
    port_override: Option<u16>,
) -> Result<Config, ConfigError> {
    // 1) 빌트인 디폴트 — runtime / security 만 안전 디폴트 보유, server는 사용자
    //    명시 필수라 dummy로 채워두고 검증에서 잡는다.
    let defaults = DefaultsSeed {
        schema_version: SCHEMA_VERSION,
        server: ServerSeed::default(),
        runtime: RuntimeConfig::default(),
        security: SecurityConfig::default(),
        cloud: None,
        frontend_dist: None,
        workspace_path: None,
        auth: AuthConfig::default(),
    };

    let mut figment = Figment::from(Serialized::defaults(defaults));

    // 2) TOML 파일 — 없으면 skip (figment::Toml::file은 missing-file 시 빈
    //    provider라 별도 IO 에러를 못 잡는다. 명시 경로 + 부재는 직접 검사.).
    if let Some(p) = path {
        if !p.exists() {
            return Err(ConfigError::Io(format!(
                "config file not found: {}",
                p.display()
            )));
        }
        figment = figment.merge(Toml::file(p));
    }

    // 3) 환경 변수 — `GTMUX_RUNTIME__LOG_LEVEL=debug` → `runtime.log_level`.
    //    `__`를 path separator로 split한다 (R7-T6 §7).
    figment = figment.merge(Env::prefixed("GTMUX_").split("__"));

    // 4) CLI session override — 마지막 단계라 어느 source든 이긴다. 빈 문자열은
    //    "override 없음" 신호로 해석.
    if !session.is_empty() {
        figment = figment.merge(Serialized::default("server.session", session));
    }

    // 5) CLI port override — `--port 9999` 가 figment 안에서 모두를 이긴다.
    //    validate() 가 본 단계 이후에 돌기 때문에 port sentinel(0)을 무사
    //    통과시킨다. None이면 layer를 추가하지 않아 기존 동작 그대로.
    if let Some(p) = port_override {
        figment = figment.merge(Serialized::default("server.port", p));
    }

    let cfg: Config = figment.extract()?;
    validate(&cfg)?;
    Ok(cfg)
}

/// CLI / `gtmux config init` 등이 사용자에게 뿌릴 default TOML 텍스트.
///
/// `<session>` / `<port>` 자리는 사용자가 채워 넣어야 하는 필수 필드라 그대로
/// placeholder로 남긴다 — fail-closed가 startup에서 강제한다.
pub fn defaults_toml() -> &'static str {
    DEFAULTS_TOML
}

const DEFAULTS_TOML: &str = r#"schema_version = 1

[server]
# tmux session name (immutable). ADR-0007 — Server identity의 일부.
session = "<session>"
# 영속 식별자. URL bookmark의 base. D21 c6.
port    = 9001
# bind address. loopback / unix → local mode, 그 외 → cloud mode 자동 추론.
bind    = "127.0.0.1"

[runtime]
# Per-pane ring buffer (KB). D15.
ring_buffer_size_kb     = 128
# Canvas Layout HTTP PUT debounce (ms). D12.
layout_debounce_ms      = 300
# Panel Streaming State 토글 debounce (ms). D16.
panel_state_debounce_ms = 300
# tracing 레벨 — trace|debug|info|warn|error|off.
log_level               = "info"
# log format — auto (tty=text / pipe=json) | text | json.
log_format              = "auto"

[security]
# CORS 화이트리스트. 비우면 startup에서 bind 호스트로 합성된다.
cors_origins   = []
# Host 헤더 화이트리스트.
host_allowlist = []

# [cloud] — bind가 loopback/unix가 아닐 때만 활성화한다.
# 기본 true. 신뢰된 네트워크에서 평문 HTTP 로 실행해야 할 때만 false.
# tls_required = true
# tls_cert = "/path/to/cert.pem"
# tls_key  = "/path/to/key.pem"
# rate_limit_auth_failures_per_minute = 10
"#;

// ─────────────────────────────────────────────────────────────────────────────
//  Internal helpers
// ─────────────────────────────────────────────────────────────────────────────

/// `Serialized::defaults`에 넘기는 seed 구조. `server.session`은 빈 문자열 +
/// `bind`는 안전 디폴트로 채워, TOML 파일이 그 셋을 채우지 않으면 validation이
/// 잡아내도록 한다.
#[derive(Debug, Clone, Serialize)]
struct DefaultsSeed {
    schema_version: u32,
    server: ServerSeed,
    runtime: RuntimeConfig,
    security: SecurityConfig,
    cloud: Option<CloudConfig>,
    frontend_dist: Option<std::path::PathBuf>,
    workspace_path: Option<std::path::PathBuf>,
    auth: AuthConfig,
}

#[derive(Debug, Clone, Serialize)]
struct ServerSeed {
    session: String,
    port: u16,
    bind: String,
}

impl Default for ServerSeed {
    fn default() -> Self {
        // 빈 session·0 port는 *fail closed marker*. validate()가 빈 session 또는
        // 1024 미만 port를 거부하므로 TOML / CLI가 채워주지 않으면 startup이
        // 멈춘다 — silent defaulting을 피하기 위한 의도된 sentinel.
        Self {
            session: String::new(),
            port: 0,
            bind: "127.0.0.1".to_string(),
        }
    }
}

/// 자료형이 통과한 뒤 *값* 차원에서 다시 확인한다. 본 crate가 ADR-0003 §5
/// 체크리스트 중 #1·#3·일부 #2를 담당.
fn validate(cfg: &Config) -> Result<(), ConfigError> {
    if cfg.schema_version != SCHEMA_VERSION {
        return Err(ConfigError::Validation(format!(
            "schema_version mismatch: expected {}, got {}",
            SCHEMA_VERSION, cfg.schema_version
        )));
    }

    if cfg.server.session.trim().is_empty() {
        return Err(ConfigError::Validation(
            "server.session must be non-empty".to_string(),
        ));
    }

    // privileged port를 거부 — root 없이 bind 못 하는 영역을 silent로 시도하지
    // 못하게 한다. 65535 상한은 `u16` 타입이 이미 강제.
    if cfg.server.port < MIN_PORT {
        return Err(ConfigError::Validation(format!(
            "server.port must be in [{}, 65535], got {}",
            MIN_PORT, cfg.server.port
        )));
    }

    if cfg.server.bind.trim().is_empty() {
        return Err(ConfigError::Validation(
            "server.bind must be non-empty".to_string(),
        ));
    }

    // mode-section 정합. cloud 모드는 cloud 섹션이 필요하다. TLS 를 요구하는
    // 기본 경로에서는 cert/key marker 도 명시되어야 lifecycle 검증으로 이어진다.
    let mode = derive_mode(&cfg.server.bind);
    match (mode, &cfg.cloud) {
        (Mode::Cloud, None) => {
            return Err(ConfigError::ModeMismatch(format!(
                "bind={:?} is cloud-mode but [cloud] section is missing",
                cfg.server.bind
            )));
        }
        (Mode::Cloud, Some(c)) => {
            // 경로 *내용* 검증(존재·perm)은 lifecycle crate. 본 crate는 빈
            // 경로만 거부 — fail-closed marker 누락을 컴파일 너머에서 잡는다.
            // `tls_required=false` 는 신뢰된 네트워크에서 평문 HTTP 를 명시적으로
            // 허용하는 경로이므로 cert/key marker 를 요구하지 않는다.
            if c.tls_required
                && (c.tls_cert.as_os_str().is_empty() || c.tls_key.as_os_str().is_empty())
            {
                return Err(ConfigError::Validation(
                    "[cloud].tls_cert and tls_key must be set when cloud.tls_required=true"
                        .to_string(),
                ));
            }
        }
        (Mode::Local, _) => {
            // Local 모드에서 [cloud] 섹션이 있어도 silent ignore — SSoT §4의
            // "cloud.* 키 무시" 정책. 사용자가 mode를 전환할 의도로 둘 수 있다.
        }
    }

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
//  Post-load helpers
// ─────────────────────────────────────────────────────────────────────────────

impl Config {
    /// `bind` 값에서 추론한 mode.
    pub fn mode(&self) -> Mode {
        derive_mode(&self.server.bind)
    }

    /// Cloud mode 에서 TLS 보안 속성을 적용해야 하는지 여부.
    /// Local mode 는 항상 `false`; Cloud mode 는 `[cloud].tls_required` 기본값 `true`.
    pub fn tls_required(&self) -> bool {
        matches!(self.mode(), Mode::Cloud)
            && self.cloud.as_ref().map(|c| c.tls_required).unwrap_or(true)
    }

    /// `host_allowlist`가 비어 있으면 bind 호스트를 보강해 반환한다 — SSoT §5
    /// step 9의 "비어 있지 않음" 조건을 *런타임에* 자동 합성하는 helper. 본
    /// 메서드는 in-place mutate 하지 않고 새 Vec을 돌려준다 (figment 결과를
    /// immutable로 유지).
    pub fn effective_host_allowlist(&self) -> Vec<String> {
        if !self.security.host_allowlist.is_empty() {
            return self.security.host_allowlist.clone();
        }
        // bind가 loopback이면 통상의 3종 세트를 합성. cloud/외부 bind는 사용자
        // 명시가 강제이므로 빈 셋 그대로 반환 — startup이 거부할 것.
        let port = self.server.port;
        match derive_mode(&self.server.bind) {
            Mode::Local => vec![
                format!("127.0.0.1:{port}"),
                format!("localhost:{port}"),
                format!("[::1]:{port}"),
            ],
            Mode::Cloud => Vec::new(),
        }
    }

    /// `cors_origins`가 비어 있으면 bind+port로부터 same-origin 디폴트를
    /// 합성해 반환한다 — `docs/reports/0017-progress-status.md` §3.2 G1.
    /// ADR-0003 D3 (정확 일치 화이트리스트, wildcard 거부) 정합을 깨지 않고,
    /// SPA가 동일 출처(`http://<bind>:<port>`)로 `fetch('/api/...')` 할 때
    /// `origin_forbidden`로 차단되지 않게 한다. `effective_host_allowlist`
    /// 와 동일한 in-place-unsafe + 새 Vec 반환 패턴을 따른다.
    ///
    /// TLS 종단은 cloud 모드의 책임이므로 본 helper는 항상 `http://` 스킴만
    /// 합성한다 (cloud 모드는 외부 reverse proxy의 `wss://` host를 명시해야
    /// 하며, 그 경우 사용자는 `cors_origins`를 직접 채워야 한다).
    pub fn effective_cors_origins(&self) -> Vec<String> {
        if !self.security.cors_origins.is_empty() {
            return self.security.cors_origins.clone();
        }
        let port = self.server.port;
        let bind = self.server.bind.as_str();
        // Loopback alias 셋 — bind가 `127.0.0.1` / `::1` / `localhost` 어느 쪽이든
        // 브라우저 사용자는 셋 중 임의 호스트로 same-origin 접속할 수 있다.
        // ADR-0003 D3 (정확 일치 화이트리스트)와의 정합은 *명시* 화이트리스트가
        // 비어 있을 때만 본 alias 셋을 노출함으로써 유지된다 — cloud 모드/외부
        // hostname은 본 fallback에 닿지 않고, 사용자가 cors_origins를 채워야 한다.
        let bind_lower = bind.to_ascii_lowercase();
        if bind_lower == "127.0.0.1" || bind_lower == "::1" || bind_lower == "localhost" {
            return vec![
                format!("http://127.0.0.1:{port}"),
                format!("http://localhost:{port}"),
                format!("http://[::1]:{port}"),
            ];
        }
        vec![format!("http://{bind}:{port}")]
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use figment::Jail;

    fn minimal_toml() -> &'static str {
        r#"schema_version = 1
[server]
session = "alpha"
port = 9001
bind = "127.0.0.1"
"#
    }

    #[test]
    fn defaults_only() {
        // figment::Jail이 env / cwd / 임시 디렉터리를 process-scoped lock으로
        // 격리한다. session 인자만으로 load 통과 (TOML 없음 → 빌트인 디폴트
        // 값이 사용자 명시 필드를 채우지 못해 validation이 실패해야 한다).
        Jail::expect_with(|jail| {
            jail.clear_env();
            // path=None + 빌트인 디폴트의 session=""·port=0 이면 validate 실패.
            let err = load(None, "alpha").unwrap_err();
            assert!(matches!(err, ConfigError::Validation(_)));
            Ok(())
        });
    }

    #[test]
    fn toml_file_load() {
        Jail::expect_with(|jail| {
            jail.clear_env();
            jail.create_file("gtmux.toml", minimal_toml())?;
            let cfg = load(Some(Path::new("gtmux.toml")), "").unwrap();
            assert_eq!(cfg.schema_version, 1);
            assert_eq!(cfg.server.session, "alpha");
            assert_eq!(cfg.server.port, 9001);
            assert_eq!(cfg.mode(), Mode::Local);
            assert_eq!(cfg.runtime.ring_buffer_size_kb, 128);
            assert_eq!(cfg.runtime.log_level, "info");
            assert!(cfg.cloud.is_none());
            Ok(())
        });
    }

    #[test]
    fn env_override() {
        Jail::expect_with(|jail| {
            jail.clear_env();
            jail.create_file("gtmux.toml", minimal_toml())?;
            jail.set_env("GTMUX_RUNTIME__LOG_LEVEL", "debug");
            jail.set_env("GTMUX_RUNTIME__RING_BUFFER_SIZE_KB", "256");
            let cfg = load(Some(Path::new("gtmux.toml")), "").unwrap();
            assert_eq!(cfg.runtime.log_level, "debug");
            assert_eq!(cfg.runtime.ring_buffer_size_kb, 256);
            Ok(())
        });
    }

    #[test]
    fn cli_session_override() {
        Jail::expect_with(|jail| {
            jail.clear_env();
            jail.create_file("gtmux.toml", minimal_toml())?;
            let cfg = load(Some(Path::new("gtmux.toml")), "beta").unwrap();
            // file said "alpha", CLI said "beta" → CLI wins.
            assert_eq!(cfg.server.session, "beta");
            Ok(())
        });
    }

    #[test]
    fn derive_mode_loopback() {
        assert_eq!(derive_mode("127.0.0.1"), Mode::Local);
        assert_eq!(derive_mode("::1"), Mode::Local);
        assert_eq!(derive_mode("localhost"), Mode::Local);
        assert_eq!(derive_mode("LocalHost"), Mode::Local); // case-insensitive
        assert_eq!(derive_mode("unix:/tmp/gtmux.sock"), Mode::Local);
    }

    #[test]
    fn derive_mode_public() {
        assert_eq!(derive_mode("0.0.0.0"), Mode::Cloud);
        assert_eq!(derive_mode("192.168.1.10"), Mode::Cloud);
        assert_eq!(derive_mode("gtmux.example.com"), Mode::Cloud);
        assert_eq!(derive_mode("10.0.0.1"), Mode::Cloud);
    }

    #[test]
    fn cloud_mode_requires_section() {
        Jail::expect_with(|jail| {
            jail.clear_env();
            jail.create_file(
                "gtmux.toml",
                r#"schema_version = 1
[server]
session = "alpha"
port = 9001
bind = "0.0.0.0"
"#,
            )?;
            let err = load(Some(Path::new("gtmux.toml")), "").unwrap_err();
            assert!(
                matches!(err, ConfigError::ModeMismatch(_)),
                "expected ModeMismatch, got {err:?}"
            );
            Ok(())
        });
    }

    #[test]
    fn cloud_mode_with_section_ok() {
        Jail::expect_with(|jail| {
            jail.clear_env();
            jail.create_file(
                "gtmux.toml",
                r#"schema_version = 1
[server]
session = "alpha"
port = 9001
bind = "0.0.0.0"
[cloud]
tls_required = true
tls_cert = "/etc/gtmux/cert.pem"
tls_key  = "/etc/gtmux/key.pem"
rate_limit_auth_failures_per_minute = 10
"#,
            )?;
            let cfg = load(Some(Path::new("gtmux.toml")), "").unwrap();
            assert_eq!(cfg.mode(), Mode::Cloud);
            let cloud = cfg.cloud.expect("cloud section present");
            assert!(cloud.tls_required);
            assert_eq!(cloud.rate_limit_auth_failures_per_minute, 10);
            Ok(())
        });
    }

    #[test]
    fn cloud_tls_required_false_allows_missing_cert_paths() {
        Jail::expect_with(|jail| {
            jail.clear_env();
            jail.create_file(
                "gtmux.toml",
                r#"schema_version = 1
[server]
session = "alpha"
port = 9001
bind = "0.0.0.0"
[cloud]
tls_required = false
rate_limit_auth_failures_per_minute = 10
"#,
            )?;
            let cfg = load(Some(Path::new("gtmux.toml")), "").unwrap();
            assert_eq!(cfg.mode(), Mode::Cloud);
            assert!(!cfg.tls_required());
            let cloud = cfg.cloud.expect("cloud section present");
            assert!(!cloud.tls_required);
            assert!(cloud.tls_cert.as_os_str().is_empty());
            assert!(cloud.tls_key.as_os_str().is_empty());
            Ok(())
        });
    }

    #[test]
    fn cloud_tls_required_defaults_true_and_requires_cert_paths() {
        Jail::expect_with(|jail| {
            jail.clear_env();
            jail.create_file(
                "gtmux.toml",
                r#"schema_version = 1
[server]
session = "alpha"
port = 9001
bind = "0.0.0.0"
[cloud]
rate_limit_auth_failures_per_minute = 10
"#,
            )?;
            let err = load(Some(Path::new("gtmux.toml")), "").unwrap_err();
            assert!(
                matches!(err, ConfigError::Validation(_)),
                "expected Validation, got {err:?}"
            );
            Ok(())
        });
    }

    #[test]
    fn unknown_field_rejected() {
        Jail::expect_with(|jail| {
            jail.clear_env();
            jail.create_file(
                "gtmux.toml",
                r#"schema_version = 1
[server]
session = "alpha"
port = 9001
bind = "127.0.0.1"
[runtime]
ring_buffer_kb = 256
"#,
            )?;
            let err = load(Some(Path::new("gtmux.toml")), "").unwrap_err();
            assert!(
                matches!(err, ConfigError::UnknownField(_)),
                "expected UnknownField, got {err:?}"
            );
            Ok(())
        });
    }

    #[test]
    fn port_range_validation() {
        Jail::expect_with(|jail| {
            jail.clear_env();
            jail.create_file(
                "gtmux.toml",
                r#"schema_version = 1
[server]
session = "alpha"
port = 80
bind = "127.0.0.1"
"#,
            )?;
            let err = load(Some(Path::new("gtmux.toml")), "").unwrap_err();
            assert!(
                matches!(err, ConfigError::Validation(_)),
                "expected Validation for privileged port, got {err:?}"
            );
            Ok(())
        });

        Jail::expect_with(|jail| {
            jail.clear_env();
            jail.create_file(
                "gtmux.toml",
                r#"schema_version = 1
[server]
session = "alpha"
port = 8080
bind = "127.0.0.1"
"#,
            )?;
            let cfg = load(Some(Path::new("gtmux.toml")), "").unwrap();
            assert_eq!(cfg.server.port, 8080);
            Ok(())
        });
    }

    #[test]
    fn missing_config_file_errs() {
        Jail::expect_with(|jail| {
            jail.clear_env();
            let err = load(Some(Path::new("nope.toml")), "alpha").unwrap_err();
            assert!(matches!(err, ConfigError::Io(_)));
            Ok(())
        });
    }

    #[test]
    fn schema_version_mismatch_rejected() {
        Jail::expect_with(|jail| {
            jail.clear_env();
            jail.create_file(
                "gtmux.toml",
                r#"schema_version = 99
[server]
session = "alpha"
port = 9001
bind = "127.0.0.1"
"#,
            )?;
            let err = load(Some(Path::new("gtmux.toml")), "").unwrap_err();
            assert!(matches!(err, ConfigError::Validation(_)));
            Ok(())
        });
    }

    #[test]
    fn effective_cors_origins_synthesises_loopback_alias_set() {
        // 빈 cors_origins + loopback bind → 127.0.0.1 / localhost / [::1] 3개
        // alias 합성. 브라우저가 localhost로 접속해도 origin_forbidden로 차단
        // 되지 않게 한다 (G1 + localhost ↔ 127.0.0.1 mismatch fix).
        Jail::expect_with(|jail| {
            jail.clear_env();
            jail.create_file("gtmux.toml", minimal_toml())?;
            let cfg = load(Some(Path::new("gtmux.toml")), "").unwrap();
            assert!(cfg.security.cors_origins.is_empty(), "precondition");
            let origins = cfg.effective_cors_origins();
            assert_eq!(origins.len(), 3);
            assert!(origins.contains(&"http://127.0.0.1:9001".to_string()));
            assert!(origins.contains(&"http://localhost:9001".to_string()));
            assert!(origins.contains(&"http://[::1]:9001".to_string()));
            Ok(())
        });
    }

    #[test]
    fn effective_cors_origins_non_loopback_bind_synthesises_single() {
        // bind가 loopback이 아니면 alias 확장 없음 — cloud 모드는 사용자가
        // cors_origins를 직접 채워야 한다는 ADR-0003 D3 의도 유지. 본 테스트는
        // 그 fallback 동작만 본다 (cloud section은 별도 검증).
        Jail::expect_with(|jail| {
            jail.clear_env();
            jail.create_file(
                "gtmux.toml",
                r#"schema_version = 1
[server]
session = "alpha"
port = 9001
bind = "192.168.1.10"
[cloud]
tls_cert = "/etc/gtmux/cert.pem"
tls_key  = "/etc/gtmux/key.pem"
rate_limit_auth_failures_per_minute = 10
"#,
            )?;
            let cfg = load(Some(Path::new("gtmux.toml")), "").unwrap();
            let origins = cfg.effective_cors_origins();
            assert_eq!(origins, vec!["http://192.168.1.10:9001".to_string()]);
            Ok(())
        });
    }

    #[test]
    fn load_with_port_override_passes_validation_for_empty_port() {
        // TOML / env 어느 쪽도 port를 채우지 않은 상태에서 CLI `--port 9999`만
        // 줬을 때 validate가 통과해야 한다 — `--port` 옵션이 실제로 동작하는지
        // 검증. 본 테스트가 fail하면 사용자가 `gtmux start --port 9999` 했을 때
        // "server.port must be in [1024, 65535], got 0" 오류로 죽는다.
        Jail::expect_with(|jail| {
            jail.clear_env();
            jail.create_file(
                "gtmux.toml",
                r#"schema_version = 1
[server]
session = "alpha"
bind = "127.0.0.1"
"#,
            )?;
            // port_override None이면 sentinel 0이 살아 validation에서 실패해야 한다.
            let err = load_with_overrides(Some(Path::new("gtmux.toml")), "", None).unwrap_err();
            assert!(matches!(err, ConfigError::Validation(_)));
            // port_override Some(9999)이면 validation 통과.
            let cfg = load_with_overrides(Some(Path::new("gtmux.toml")), "", Some(9999)).unwrap();
            assert_eq!(cfg.server.port, 9999);
            Ok(())
        });
    }

    #[test]
    fn effective_cors_origins_passes_explicit_through_unchanged() {
        // 사용자가 명시한 값은 그대로 — backward compatibility 보장.
        Jail::expect_with(|jail| {
            jail.clear_env();
            jail.create_file(
                "gtmux.toml",
                r#"schema_version = 1
[server]
session = "alpha"
port = 9001
bind = "127.0.0.1"
[security]
cors_origins = ["http://example.test:8443", "http://127.0.0.1:9001"]
"#,
            )?;
            let cfg = load(Some(Path::new("gtmux.toml")), "").unwrap();
            let origins = cfg.effective_cors_origins();
            assert_eq!(
                origins,
                vec![
                    "http://example.test:8443".to_string(),
                    "http://127.0.0.1:9001".to_string(),
                ]
            );
            Ok(())
        });
    }

    #[test]
    fn effective_host_allowlist_synthesis() {
        Jail::expect_with(|jail| {
            jail.clear_env();
            jail.create_file("gtmux.toml", minimal_toml())?;
            let cfg = load(Some(Path::new("gtmux.toml")), "").unwrap();
            let hosts = cfg.effective_host_allowlist();
            assert!(hosts.iter().any(|h| h == "127.0.0.1:9001"));
            assert!(hosts.iter().any(|h| h == "localhost:9001"));
            assert!(hosts.iter().any(|h| h == "[::1]:9001"));
            Ok(())
        });
    }

    #[test]
    fn defaults_toml_round_trips() {
        // 사용자 onboarding template은 placeholder만 채우면 그대로 parse 통과해야
        // 한다 — gtmux config init 산출물의 self-test.
        let filled = defaults_toml().replace("\"<session>\"", "\"demo\"");
        let parsed: Config = ::toml::from_str(&filled).expect("defaults_toml parses");
        assert_eq!(parsed.schema_version, SCHEMA_VERSION);
        assert_eq!(parsed.server.session, "demo");
    }
}
