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

/// `[cloud]` 섹션 — Cloud 모드에서만 의미가 있는 키. Local 모드는 `None`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CloudConfig {
    /// PEM 인증서 경로. 파일 존재 + 0600 perm 검증은 lifecycle crate가 수행.
    pub tls_cert: PathBuf,
    /// PEM 비밀키 경로.
    pub tls_key: PathBuf,
    /// 분당 인증 실패 허용 횟수 (SSoT §1.10). 기본 10 — code-server 대비
    /// 약간 관대하나 grill D22에서 명시 키로 두기로 결정.
    pub rate_limit_auth_failures_per_minute: u32,
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
    // 1) 빌트인 디폴트 — runtime / security 만 안전 디폴트 보유, server는 사용자
    //    명시 필수라 dummy로 채워두고 검증에서 잡는다.
    let defaults = DefaultsSeed {
        schema_version: SCHEMA_VERSION,
        server: ServerSeed::default(),
        runtime: RuntimeConfig::default(),
        security: SecurityConfig::default(),
        cloud: None,
        frontend_dist: None,
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

    // mode-section 정합. cloud 모드는 cloud 섹션 + 파일 경로가 *명시*되어야
    // lifecycle crate가 부팅 시 cert/key 검증으로 fail-closed 진입 가능.
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
            if c.tls_cert.as_os_str().is_empty() || c.tls_key.as_os_str().is_empty() {
                return Err(ConfigError::Validation(
                    "[cloud].tls_cert and tls_key must be set for cloud mode".to_string(),
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
tls_cert = "/etc/gtmux/cert.pem"
tls_key  = "/etc/gtmux/key.pem"
rate_limit_auth_failures_per_minute = 10
"#,
            )?;
            let cfg = load(Some(Path::new("gtmux.toml")), "").unwrap();
            assert_eq!(cfg.mode(), Mode::Cloud);
            let cloud = cfg.cloud.expect("cloud section present");
            assert_eq!(cloud.rate_limit_auth_failures_per_minute, 10);
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
