//! gtmux-config — figment 기반 TOML + env 오버레이 로더.
//!
//! D22 schema (schema_version / server / runtime / security / cloud) 그대로
//! 미러한다. 본 스캐폴드는 struct 윤곽만 잡고 implementation은 후속 task로 미룬다.
//!
//! `#[serde(deny_unknown_fields)]`는 오타 방지를 위해 실제 구현 단계에서
//! 활성화한다 (지금은 모든 필드가 Option 또는 stub 이므로 미적용).

#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};

/// 부트스트랩 placeholder — D22 최상위 schema.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Config {
    pub schema_version: Option<u32>,
    pub server: Option<ServerCfg>,
    pub runtime: Option<RuntimeCfg>,
    pub security: Option<SecurityCfg>,
    pub cloud: Option<CloudCfg>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ServerCfg {}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct RuntimeCfg {}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct SecurityCfg {}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct CloudCfg {}

/// 부트스트랩 placeholder — figment 로더 시그니처.
pub fn load() -> anyhow::Result<Config> {
    todo!("config::load — figment chain to be implemented")
}
