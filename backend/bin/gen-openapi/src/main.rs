//! gen-openapi: emit an OpenAPI 3.1 YAML document built from utoipa-derived
//! stub structs to stdout.
//!
//! Bootstrap-grade. The `Group` and `Panel` schemas here are placeholders
//! covering only an `id` field -- their job is to prove that the
//! `utoipa` derive -> serde_yaml -> `openapi-typescript` chain works
//! end to end. The real Canvas Layout schema (groups[] + panels[] tree,
//! ADR-0010 G-hybrid, `docs/ssot/canvas-layout-schema.md`) is populated
//! incrementally once the `http-api` crate lands.

use serde::{Deserialize, Serialize};
use utoipa::{OpenApi, ToSchema};

/// Placeholder Group payload. ADR-0010 G-hybrid will extend this with
/// `parent_id`, `label`, `color`, `visibility`, `locked`, `order`.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct Group {
    /// Stable identifier for the group node within a Canvas Layout tree.
    pub id: String,
}

/// Placeholder Panel payload. The real schema adds geometry, z-order,
/// visibility, lock, label, note, and parent linkage (ADR-0010).
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct Panel {
    /// Stable identifier for the panel node within a Canvas Layout tree.
    pub id: String,
}

#[derive(OpenApi)]
#[openapi(
    info(
        title       = "gtmux API",
        version     = "0.0.0",
        description = "Bootstrap OpenAPI stub for Task C3 codegen pipeline. Real surface lands with http-api crate.",
    ),
    components(schemas(Group, Panel)),
)]
struct ApiDoc;

fn main() -> anyhow::Result<()> {
    let doc = ApiDoc::openapi();
    let yaml = serde_yaml::to_string(&doc)?;
    print!("{yaml}");
    Ok(())
}
