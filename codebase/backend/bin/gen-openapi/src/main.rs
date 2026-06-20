//! gen-openapi: emit an OpenAPI 3.1 YAML document for the gtmux API.
//!
//! The Canvas Layout schema (ADR-0018) is sourced directly from the
//! `gtmux-http-api` crate's `schema` module via utoipa `ToSchema` derives
//! (ADR-0042 / plan-0017 Phase 1). This replaces the earlier Group/Panel
//! id-only bootstrap stub so `openapi.yaml` → `api.d.ts` carries the real
//! canvas `Item` discriminated union — making the codegen drift gate
//! meaningful for canvas fields.

use gtmux_http_api::schema::{
    Anchor, FigureStrokeDash, FontFamily, FontWeight, Group, Head, Item, ItemCommon, Layout,
    PathEndpoint, PathWaypoint, Point, Routing, SnippetEntry, TextAlign, TextVerticalAlign,
    Viewport, Visibility,
};
// ADR-0052 D5 — Files-tab recursive search (`GET /api/fs/search`) response
// contract. Surfaced as component schemas so the FE `api.d.ts` carries the
// `FsSearchResponse` / `FsSearchEntry` types (the query params are `IntoParams`,
// not component schemas — this doc is schema-only, `paths: {}`).
use gtmux_http_api::{FsSearchEntry, FsSearchResponse};
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "gtmux API",
        version = "0.0.0",
        description = "gtmux Canvas Layout schema (ADR-0018). Generated from gtmux-http-api schema.rs via utoipa (ADR-0042).",
    ),
    components(schemas(
        Layout,
        Group,
        Viewport,
        ItemCommon,
        Item,
        Point,
        SnippetEntry,
        Visibility,
        TextAlign,
        TextVerticalAlign,
        FontWeight,
        FontFamily,
        FigureStrokeDash,
        Anchor,
        Head,
        Routing,
        PathEndpoint,
        PathWaypoint,
        // ADR-0052 D5 — Files-tab recursive search response.
        FsSearchResponse,
        FsSearchEntry,
    ))
)]
struct ApiDoc;

fn main() -> anyhow::Result<()> {
    let doc = ApiDoc::openapi();
    print!("{}", serde_yaml::to_string(&doc)?);
    Ok(())
}
