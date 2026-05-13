//! gtmux-ws-server — axum `/ws` upgrade + subprotocol auth + binary envelope codec.
//!
//! Surface (Sprint 2 P0-WS-1):
//! - [`router`] mounts `GET /ws` with Origin/Host gating and subprotocol-based
//!   bearer auth (ADR-0002 D5 + ADR-0003 D5, `docs/ssot/security-defaults.md` §6).
//! - [`Envelope`] is the wire object that flows through the binary frame.
//!   See `docs/ssot/wire-protocol.md` §1.2 for the byte layout (the on-wire
//!   `paneId` varint lives *inside* the payload bytes — this crate carries it
//!   transparently; envelope encode/decode is the framing-level concern only).
//! - [`parse_subprotocol`] implements the RFC 6455 §11.3.4 comma-separated
//!   list semantics ADR-0003 D5 mandates: `"gtmux.v1, bearer.<token>"`.
//!
//! Out of scope for this sprint:
//! - Data plane: per-pane output pump, ring buffer replay (Sprint 3 + LIFE→WS
//!   wiring task). The handler currently echoes/logs web-domain frames and
//!   maintains the heartbeat so the *protocol shape* can be exercised
//!   end-to-end before the data pump lands.
//! - HTTP cookie bootstrap exchange (P0-HTTP-2, owned by `gtmux-http-api`).

#![forbid(unsafe_code)]
#![deny(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use bytes::{BufMut, Bytes, BytesMut};
use futures::{SinkExt, StreamExt};
use gtmux_auth::TokenString;
use gtmux_config::Config;
use thiserror::Error;
use tokio::time::Instant;
use tracing::{debug, info, warn};

// ─────────────────────────────────────────────────────────────────────────────
//  Constants — calibrated against `docs/ssot/wire-protocol.md` §1.2 and
//  `docs/reports/0010-grill-amendments.md` D15.
// ─────────────────────────────────────────────────────────────────────────────

/// Maximum payload bytes per envelope. The SSoT pins a 1 MiB soft cap for the
/// whole WS message (§1.2), but the codec lives in front of any framing
/// reassembly: a 4 MiB hard ceiling here gives us a defensive 4× headroom so
/// a single attacker-controlled length prefix cannot OOM the decoder. The
/// soft cap (1 MiB) is enforced upstream by the WS framer + a future broadcast
/// path; this constant is the *codec's* terminal guard.
pub const MAX_PAYLOAD: usize = 4 * 1024 * 1024;

/// Envelope header: 1-byte type + 4-byte little-endian length.
const HEADER_LEN: usize = 5;

/// Heartbeat ping cadence. SSoT §4 leaves the exact value to the implementation;
/// 30 s mirrors Kubernetes / nginx defaults and the ADR-0002 R7 §5 sketch.
const PING_INTERVAL: Duration = Duration::from_secs(30);

/// Pong-grace timeout. If no pong arrives within this window after a ping,
/// the connection is considered dead and closed with code 1011 (Internal).
/// Two missed intervals matches typical browser-side WS deadband budgets.
const PONG_TIMEOUT: Duration = Duration::from_secs(60);

/// WS close codes used by this crate. Numbers cross-checked against
/// `docs/ssot/wire-protocol.md` §3 (1003 unsupported-data, 1008 policy,
/// 1011 internal) and ADR-0003 D13/D21 c7 for the 4001/4002 custom codes
/// (those are not raised here directly — `auth::rotate_token` triggers 4001
/// via a broadcast channel that will be wired up in Sprint 3).
mod close_codes {
    pub const NORMAL: u16 = 1000;
    pub const UNSUPPORTED_DATA: u16 = 1003;
    pub const POLICY_VIOLATION: u16 = 1008;
    pub const INTERNAL: u16 = 1011;
}

// ─────────────────────────────────────────────────────────────────────────────
//  Frame type IDs — `docs/ssot/wire-protocol.md` §2 (tmux-domain 0x01–0x07,
//  web-domain 0x80–0x84). 0x08–0x0F and 0x85–0x8F are SSoT-reserved and
//  intentionally NOT modelled here — `from_u8` rejects them as unknown to
//  prevent client code from accidentally relying on a reserved slot.
// ─────────────────────────────────────────────────────────────────────────────

/// Envelope frame type. 1 byte on the wire. The discriminant is the canonical
/// SSoT value — never re-number without updating the SSoT table first.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameType {
    // tmux-domain (SSoT §2.1)
    /// `0x01 CTRL` — control-mode argv command / response (paneId = 0).
    Ctrl = 0x01,
    /// `0x02 PANE_OUT` — `%output` raw bytes (server → client).
    PaneOutput = 0x02,
    /// `0x03 PANE_IN` — keyboard input bytes (client → server).
    PaneInput = 0x03,
    /// `0x04 PANE_RESIZE` — single-pane-window size change.
    PaneResize = 0x04,
    /// `0x05 PANE_PAUSE` — Panel Streaming State → Suspended.
    PanePause = 0x05,
    /// `0x06 PANE_RESUME` — Panel Streaming State → Streaming.
    PaneResume = 0x06,
    /// `0x07 NOTIFY_MIRROR` — non-`%output` tmux notification mirror.
    NotifyMirror = 0x07,
    // web-domain (SSoT §2.2)
    /// `0x80 LAYOUT_CHANGED` — broadcast on HTTP layout PUT success.
    /// Server → client only; client→server is a protocol violation.
    LayoutChanged = 0x80,
    /// `0x81 M_CHANGED` — Manipulation Selection broadcast.
    ManipulationSelection = 0x81,
    /// `0x82 I_CHANGED` — Input Target broadcast.
    InputTarget = 0x82,
    /// `0x83 VIEWPORT_CHANGED` — viewport pan/zoom broadcast.
    ViewportChanged = 0x83,
    /// `0x84 FOCUS_MODE_CHANGED` — focus mode toggle broadcast.
    FocusMode = 0x84,
}

impl FrameType {
    /// Best-effort decode of the type byte. Returns `None` for SSoT-reserved
    /// slots and any byte outside the two defined ranges — callers must treat
    /// `None` as a protocol error (RFC 6455 close 1003).
    pub fn from_u8(b: u8) -> Option<Self> {
        Some(match b {
            0x01 => Self::Ctrl,
            0x02 => Self::PaneOutput,
            0x03 => Self::PaneInput,
            0x04 => Self::PaneResize,
            0x05 => Self::PanePause,
            0x06 => Self::PaneResume,
            0x07 => Self::NotifyMirror,
            0x80 => Self::LayoutChanged,
            0x81 => Self::ManipulationSelection,
            0x82 => Self::InputTarget,
            0x83 => Self::ViewportChanged,
            0x84 => Self::FocusMode,
            _ => return None,
        })
    }

    /// On-wire discriminant.
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    /// `true` if the slot is web-domain (0x80–0x84). Used to gate broadcast
    /// dispatch — keeps the tmux/web split machine-enforced (invariant #1).
    pub fn is_web_domain(self) -> bool {
        (self.as_u8() & 0x80) != 0
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Codec
// ─────────────────────────────────────────────────────────────────────────────

/// A single wire envelope. The `payload` is opaque to the codec — the on-wire
/// `paneId` varint lives *inside* it for the tmux-domain frames; per-type
/// payload parsing happens in the routing layer (Sprint 3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Envelope {
    /// Type discriminant.
    pub kind: FrameType,
    /// Opaque payload bytes (may be empty).
    pub payload: Bytes,
}

impl Envelope {
    /// Build a new envelope. No length validation here — `encode` performs the
    /// MAX_PAYLOAD check at serialisation time so this constructor stays
    /// allocation-free in hot paths that already know the payload fits.
    pub fn new(kind: FrameType, payload: Bytes) -> Self {
        Self { kind, payload }
    }

    /// Encode to wire bytes. Layout: `[type(1)][len(4 LE)][payload(len)]`.
    /// Returns `Err(PayloadTooLarge)` if `payload.len() > MAX_PAYLOAD` so the
    /// caller cannot accidentally publish a frame the decoder will reject.
    pub fn encode(&self) -> Result<Bytes, CodecError> {
        let len = self.payload.len();
        if len > MAX_PAYLOAD {
            // `as u32` is safe because the guard above already constrains
            // `len` to fit in MAX_PAYLOAD (well below u32::MAX).
            return Err(CodecError::PayloadTooLarge(len as u32));
        }
        let mut buf = BytesMut::with_capacity(HEADER_LEN + len);
        buf.put_u8(self.kind.as_u8());
        // u32 LE is mandated by the SSoT codec sketch (§1.2 + §3 pseudocode
        // uses LE for the int32/float32 payload fields; we keep the length
        // prefix in the same byte order to avoid two endianness conventions
        // coexisting in one decoder).
        buf.put_u32_le(len as u32);
        buf.extend_from_slice(&self.payload);
        Ok(buf.freeze())
    }

    /// Decode one envelope from `input`. Returns the envelope and the number
    /// of bytes consumed, so a buffered reader can advance past it. Never
    /// panics — every malformed shape becomes a typed `CodecError`.
    pub fn decode(input: &[u8]) -> Result<(Self, usize), CodecError> {
        if input.len() < HEADER_LEN {
            return Err(CodecError::Truncated);
        }
        let type_byte = input[0];
        let kind = FrameType::from_u8(type_byte).ok_or(CodecError::UnknownType(type_byte))?;
        // The header guarantees 4 bytes are available; `try_into` returns
        // `Result` so we keep the no-panic deny intact.
        let len_bytes: [u8; 4] = input[1..5].try_into().map_err(|_| CodecError::Truncated)?;
        let len = u32::from_le_bytes(len_bytes);
        if (len as usize) > MAX_PAYLOAD {
            return Err(CodecError::PayloadTooLarge(len));
        }
        let total = HEADER_LEN
            .checked_add(len as usize)
            .ok_or(CodecError::PayloadTooLarge(len))?;
        if input.len() < total {
            return Err(CodecError::Truncated);
        }
        // `Bytes::copy_from_slice` keeps the decoder owning its payload —
        // callers cannot accidentally hold a slice into the framing buffer
        // past the next decode call.
        let payload = Bytes::copy_from_slice(&input[HEADER_LEN..total]);
        Ok((Self { kind, payload }, total))
    }
}

/// Codec failures surface as a closed enum so the WS handler can map each
/// variant to a specific close code without string-matching.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum CodecError {
    /// Header or payload was shorter than the declared length.
    #[error("truncated frame")]
    Truncated,
    /// Type byte is outside SSoT §2 and §2.2 ranges or hits a reserved slot.
    #[error("unknown frame type 0x{0:02x}")]
    UnknownType(u8),
    /// Declared payload length exceeds [`MAX_PAYLOAD`] — defensive cap, see
    /// the constant's doc-comment for rationale.
    #[error("payload too large: {0} bytes > {MAX_PAYLOAD} max")]
    PayloadTooLarge(u32),
}

// ─────────────────────────────────────────────────────────────────────────────
//  Subprotocol parser — RFC 6455 §11.3.4 comma-separated tokens.
//  ADR-0003 D5 + `docs/ssot/security-defaults.md` §1.3.
// ─────────────────────────────────────────────────────────────────────────────

/// Result of parsing the `Sec-WebSocket-Protocol` header value. Both fields
/// can be independently absent — the handler decides whether a missing piece
/// is fatal (it always is for `gtmux.v1` and bearer-style auth in practice,
/// but the parser stays mechanical and reports what it saw).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedSubprotocol {
    /// `gtmux.v1` advertised by the client.
    pub gtmux_v1: bool,
    /// The base64url-encoded token following the `bearer.` prefix, if present.
    /// The token text is preserved verbatim — case-sensitive base64url, no
    /// whitespace tolerance inside the token itself.
    pub bearer_token: Option<String>,
}

/// Parse the `Sec-WebSocket-Protocol` request header.
///
/// Tolerance policy (chosen so the parser handles real-world client behaviour
/// without becoming a security footgun):
/// - **Order-independent**: `"gtmux.v1, bearer.<t>"` and the reverse both work.
/// - **Whitespace-tolerant**: per RFC 7230 §3.2.6 a list element can be
///   surrounded by optional whitespace. We trim per-element only.
/// - **Case-sensitive on tokens**: `Gtmux.V1` does NOT match. The SSoT writes
///   the literal `gtmux.v1` and the bearer.* prefix in lowercase, so anything
///   else is a misconfigured client.
/// - **Empty tokens rejected**: `"bearer."` (zero-length token) returns
///   `bearer_token = None`. A real token is 43 base64url chars; downstream
///   `verify_token` will reject decode-failures, but we also refuse to even
///   surface an empty string here.
///
/// Returns `None` only when the header value is itself empty or contains no
/// non-whitespace tokens at all — that lets the handler distinguish
/// `"header absent / header empty"` from `"header present but unhelpful"`.
pub fn parse_subprotocol(header_value: &str) -> Option<ParsedSubprotocol> {
    let mut gtmux_v1 = false;
    let mut bearer_token: Option<String> = None;
    let mut any = false;
    for raw in header_value.split(',') {
        let tok = raw.trim();
        if tok.is_empty() {
            continue;
        }
        any = true;
        if tok == "gtmux.v1" {
            gtmux_v1 = true;
        } else if let Some(t) = tok.strip_prefix("bearer.") {
            if !t.is_empty() {
                // Take the *first* non-empty bearer.* we see — repeated
                // bearer.* tokens in one header are not a valid client shape
                // and we refuse to silently coalesce them.
                if bearer_token.is_none() {
                    bearer_token = Some(t.to_string());
                }
            }
        }
        // Unknown sub-tokens are tolerated (forward-compat) but produce
        // neither `gtmux_v1` nor `bearer_token` evidence.
    }
    if !any {
        return None;
    }
    Some(ParsedSubprotocol {
        gtmux_v1,
        bearer_token,
    })
}

// ─────────────────────────────────────────────────────────────────────────────
//  Router + handshake
// ─────────────────────────────────────────────────────────────────────────────

/// Shared state passed to the upgrade handler. `Arc` so axum's clone-per-
/// request stays cheap.
#[derive(Clone)]
struct WsState {
    token: Arc<TokenString>,
    /// Subprotocol value we echo on success — always `"gtmux.v1"`. Stored as
    /// a pre-built `HeaderValue` so the hot path avoids a re-validation alloc.
    echo_protocol: HeaderValue,
}

/// Build the WS sub-router. Mount onto the top-level `axum::Router` and apply
/// Origin/Host middleware externally — those policies are shared with the
/// HTTP API surface (ADR-0002 D6 + `docs/ssot/security-defaults.md` §1.2),
/// so each crate must not re-implement them.
///
/// `token` is the server's stored token; the handler verifies the client's
/// `bearer.<...>` against it in constant time via `gtmux_auth::verify_token`.
pub fn router(_config: &Config, token: &TokenString) -> Router {
    // Pre-build the echo header value so handle_upgrade doesn't have to
    // re-validate "gtmux.v1" on every connection.
    let echo_protocol = HeaderValue::from_static("gtmux.v1");
    let state = WsState {
        token: Arc::new(token.clone()),
        echo_protocol,
    };
    Router::new()
        .route("/ws", get(ws_handler))
        .with_state(state)
}

/// Upgrade entry point. Validates the subprotocol header before letting axum
/// flip the TCP stream into WS mode — failed auth never observes the
/// `on_upgrade` callback (so no half-attached connection survives).
async fn ws_handler(
    State(state): State<WsState>,
    ws: WebSocketUpgrade,
    headers: HeaderMap,
) -> Response {
    let Some(raw) = headers
        .get("sec-websocket-protocol")
        .and_then(|v| v.to_str().ok())
    else {
        // RFC 6455 §4.1 lets the server refuse the upgrade entirely; we use
        // 426 to make the failure mode legible in browser devtools (the
        // `Upgrade` header echo is informational — axum sets the framing).
        return (StatusCode::UPGRADE_REQUIRED, "subprotocol header required").into_response();
    };

    let Some(parsed) = parse_subprotocol(raw) else {
        return (StatusCode::UPGRADE_REQUIRED, "subprotocol header empty").into_response();
    };
    if !parsed.gtmux_v1 {
        return (StatusCode::UPGRADE_REQUIRED, "gtmux.v1 required").into_response();
    }
    let Some(token) = parsed.bearer_token else {
        return (StatusCode::UNAUTHORIZED, "bearer token required").into_response();
    };

    if !gtmux_auth::verify_token(&token, state.token.as_ref()) {
        // No logging of the presented token — the logging redact policy
        // (`docs/ssot/security-defaults.md` §1.10 `logging.redact_fields`)
        // names "token" / "authorization" as redacted, and we honour it
        // here by simply not putting the value on a log line at all.
        warn!("ws upgrade rejected: token mismatch");
        return (StatusCode::UNAUTHORIZED, "invalid token").into_response();
    }

    let echo = state.echo_protocol.clone();
    let mut response = ws.protocols(["gtmux.v1"]).on_upgrade(handle_socket);
    // axum's `protocols(...)` already echoes the matched value; we also set
    // it explicitly so the response header is present even if axum's matching
    // logic changes shape across minor versions. The bearer.* sub-token is
    // intentionally NOT echoed — SSoT §6 step 5.
    response
        .headers_mut()
        .insert("sec-websocket-protocol", echo);
    response
}

/// Per-connection loop. Echoes web-domain envelopes, rejects client-origin
/// 0x80 frames (server-only), and maintains a 30 s ping / 60 s pong-grace
/// heartbeat. The data pump (per-pane output, ring buffer replay) lands in
/// Sprint 3 once `mux-router` exposes its output stream.
async fn handle_socket(socket: WebSocket) {
    let (mut sink, mut stream) = socket.split();

    // Send the initial LAYOUT_CHANGED envelope so the client knows the
    // server is alive and can decide whether to re-fetch `/api/layout`.
    // Payload is an all-zeros 16-byte ETag sentinel — the real ETag binding
    // arrives once http-api wires the broadcast channel (Sprint 3 LIFE→WS).
    let hello = Envelope::new(FrameType::LayoutChanged, Bytes::from_static(&[0u8; 16]));
    if let Ok(buf) = hello.encode() {
        if sink
            .send(Message::Binary(buf.to_vec().into()))
            .await
            .is_err()
        {
            debug!("ws hello send failed; peer hung up early");
            return;
        }
    }

    let mut ping_timer = tokio::time::interval(PING_INTERVAL);
    ping_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    // Skip the immediate first tick — the hello frame above is the
    // server's liveness signal for the first 30 s.
    ping_timer.tick().await;
    let mut last_pong = Instant::now();

    loop {
        tokio::select! {
            biased; // Drain inbound first so pongs reset `last_pong` before
                    // the next ping decision.
            maybe_msg = stream.next() => {
                let Some(msg) = maybe_msg else { break };
                match msg {
                    Ok(Message::Binary(buf)) => {
                        match Envelope::decode(buf.as_ref()) {
                            Ok((env, _)) => {
                                if let Some(close) = handle_envelope(&env) {
                                    let _ = sink.send(close_frame(close.code, close.reason)).await;
                                    return;
                                }
                                // Echo back so an integration test can prove
                                // the round-trip without invoking the data
                                // pump. Real broadcast lands in Sprint 3.
                                if let Ok(out) = env.encode() {
                                    if sink.send(Message::Binary(out.to_vec().into())).await.is_err() {
                                        break;
                                    }
                                }
                            }
                            Err(e) => {
                                debug!("ws decode error: {e}");
                                let _ = sink.send(close_frame(
                                    close_codes::UNSUPPORTED_DATA,
                                    "malformed envelope",
                                )).await;
                                return;
                            }
                        }
                    }
                    Ok(Message::Text(_)) => {
                        // SSoT §1.1: text frames are not part of the protocol.
                        let _ = sink.send(close_frame(
                            close_codes::UNSUPPORTED_DATA,
                            "text frames not supported",
                        )).await;
                        return;
                    }
                    Ok(Message::Pong(_)) => { last_pong = Instant::now(); }
                    Ok(Message::Ping(p)) => {
                        // RFC 6455 says we MUST reply with the same payload;
                        // axum's tungstenite layer does this automatically
                        // in most cases, but doing it explicitly costs
                        // nothing and is defensive against version drift.
                        let _ = sink.send(Message::Pong(p)).await;
                    }
                    Ok(Message::Close(_)) => {
                        let _ = sink.send(close_frame(
                            close_codes::NORMAL,
                            "peer closed",
                        )).await;
                        return;
                    }
                    Err(e) => {
                        debug!("ws stream error: {e}");
                        return;
                    }
                }
            }
            _ = ping_timer.tick() => {
                if last_pong.elapsed() > PONG_TIMEOUT {
                    info!("ws timeout: no pong for {:?}", last_pong.elapsed());
                    let _ = sink.send(close_frame(
                        close_codes::INTERNAL,
                        "heartbeat timeout",
                    )).await;
                    return;
                }
                if sink.send(Message::Ping(Bytes::new())).await.is_err() {
                    break;
                }
            }
        }
    }
}

/// Verdict for an incoming envelope. `Some(close)` aborts the connection;
/// `None` lets the handler keep echoing.
struct CloseDirective {
    code: u16,
    reason: &'static str,
}

/// Validate an incoming envelope's *origin direction*. The codec already
/// rejected unknown types; this layer enforces the SSoT §2 directionality
/// rules. Currently the only client-origin invariant is "0x80 is server-only".
fn handle_envelope(env: &Envelope) -> Option<CloseDirective> {
    match env.kind {
        FrameType::LayoutChanged => Some(CloseDirective {
            code: close_codes::POLICY_VIOLATION,
            reason: "0x80 LAYOUT_CHANGED is server-only",
        }),
        FrameType::ManipulationSelection
        | FrameType::InputTarget
        | FrameType::ViewportChanged
        | FrameType::FocusMode => {
            debug!(
                kind = ?env.kind,
                bytes = env.payload.len(),
                "ws web-domain envelope received (skeleton echo)",
            );
            None
        }
        // tmux-domain frames: real handling needs `mux-router` wiring
        // (Sprint 3). Log + echo for now so integration tests can probe.
        _ => {
            debug!(
                kind = ?env.kind,
                bytes = env.payload.len(),
                "ws tmux-domain envelope received (skeleton echo)",
            );
            None
        }
    }
}

fn close_frame(code: u16, reason: &'static str) -> Message {
    Message::Close(Some(axum::extract::ws::CloseFrame {
        code,
        reason: reason.into(),
    }))
}

// ─────────────────────────────────────────────────────────────────────────────
//  Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "tests assert via panic; relaxing the crate-wide deny only inside this module"
)]
mod tests {
    use super::*;

    // ── Subprotocol parser ────────────────────────────────────────────────

    #[test]
    fn parse_subprotocol_both() {
        let p = parse_subprotocol("gtmux.v1, bearer.xyz").unwrap();
        assert!(p.gtmux_v1);
        assert_eq!(p.bearer_token.as_deref(), Some("xyz"));
    }

    #[test]
    fn parse_subprotocol_reverse_order() {
        let p = parse_subprotocol("bearer.xyz, gtmux.v1").unwrap();
        assert!(p.gtmux_v1);
        assert_eq!(p.bearer_token.as_deref(), Some("xyz"));
    }

    #[test]
    fn parse_subprotocol_whitespace_tolerant() {
        let a = parse_subprotocol("gtmux.v1,bearer.xyz").unwrap();
        let b = parse_subprotocol(" gtmux.v1 , bearer.xyz ").unwrap();
        let c = parse_subprotocol("\tgtmux.v1\t,\tbearer.xyz\t").unwrap();
        for p in [a, b, c] {
            assert!(p.gtmux_v1);
            assert_eq!(p.bearer_token.as_deref(), Some("xyz"));
        }
    }

    #[test]
    fn parse_subprotocol_missing_v1() {
        let p = parse_subprotocol("bearer.xyz").unwrap();
        assert!(!p.gtmux_v1);
        assert_eq!(p.bearer_token.as_deref(), Some("xyz"));
    }

    #[test]
    fn parse_subprotocol_missing_bearer() {
        let p = parse_subprotocol("gtmux.v1").unwrap();
        assert!(p.gtmux_v1);
        assert!(p.bearer_token.is_none());
    }

    #[test]
    fn parse_subprotocol_malformed() {
        assert!(parse_subprotocol("").is_none());
        // Just whitespace and commas: no tokens at all.
        assert!(parse_subprotocol(",").is_none());
        assert!(parse_subprotocol("  ,  ").is_none());
        // bearer. with empty token → bearer_token absent, but the header is
        // not empty so we still return Some with gtmux_v1=false.
        let p = parse_subprotocol("bearer.").unwrap();
        assert!(!p.gtmux_v1);
        assert!(p.bearer_token.is_none());
        // Case-sensitivity: capitalised variants are NOT accepted.
        let p = parse_subprotocol("Gtmux.V1, BEARER.xyz").unwrap();
        assert!(!p.gtmux_v1);
        assert!(p.bearer_token.is_none());
    }

    #[test]
    fn parse_subprotocol_ignores_unknown_tokens() {
        // Forward-compat: an unknown sub-token shouldn't dynamite the parse.
        let p = parse_subprotocol("gtmux.v1, x-future-extension, bearer.t").unwrap();
        assert!(p.gtmux_v1);
        assert_eq!(p.bearer_token.as_deref(), Some("t"));
    }

    // ── Codec ─────────────────────────────────────────────────────────────

    #[test]
    fn envelope_encode_decode_roundtrip() {
        let payload = Bytes::from(vec![0x41u8; 100]);
        let env = Envelope::new(FrameType::PaneOutput, payload.clone());
        let buf = env.encode().unwrap();
        assert_eq!(buf.len(), HEADER_LEN + 100);
        let (decoded, consumed) = Envelope::decode(&buf).unwrap();
        assert_eq!(consumed, HEADER_LEN + 100);
        assert_eq!(decoded.kind, FrameType::PaneOutput);
        assert_eq!(decoded.payload, payload);
    }

    #[test]
    fn envelope_decode_truncated_returns_err() {
        // Just the header byte, no length.
        assert_eq!(Envelope::decode(&[0x02]), Err(CodecError::Truncated));
        // Header but no payload, declared length 10.
        let mut buf = vec![0x02u8];
        buf.extend_from_slice(&10u32.to_le_bytes());
        assert_eq!(Envelope::decode(&buf), Err(CodecError::Truncated));
        // Header + partial payload.
        buf.extend_from_slice(b"abc");
        assert_eq!(Envelope::decode(&buf), Err(CodecError::Truncated));
    }

    #[test]
    fn envelope_decode_unknown_type() {
        let mut buf = vec![0x42u8];
        buf.extend_from_slice(&0u32.to_le_bytes());
        assert_eq!(Envelope::decode(&buf), Err(CodecError::UnknownType(0x42)));
        // Reserved slot 0x08 also rejected.
        let mut buf = vec![0x08u8];
        buf.extend_from_slice(&0u32.to_le_bytes());
        assert_eq!(Envelope::decode(&buf), Err(CodecError::UnknownType(0x08)));
        // Reserved web-domain slot 0x85.
        let mut buf = vec![0x85u8];
        buf.extend_from_slice(&0u32.to_le_bytes());
        assert_eq!(Envelope::decode(&buf), Err(CodecError::UnknownType(0x85)));
    }

    #[test]
    fn envelope_decode_oversized() {
        let oversize = (MAX_PAYLOAD as u32) + 1;
        let mut buf = vec![0x02u8];
        buf.extend_from_slice(&oversize.to_le_bytes());
        // Don't actually allocate the payload — the decoder must reject on
        // the length prefix alone, before reading the payload bytes.
        assert_eq!(
            Envelope::decode(&buf),
            Err(CodecError::PayloadTooLarge(oversize))
        );
    }

    #[test]
    fn envelope_payload_endianness_le() {
        // Pick a length whose LE and BE byte patterns differ visibly. The
        // value sits comfortably inside MAX_PAYLOAD (4 MiB) yet has all four
        // bytes distinct, so a BE/LE confusion would be obvious in the
        // assertions below.
        let len_value = 0x000400FFu32; // 262_399 bytes
        let payload = Bytes::from(vec![0u8; len_value as usize]);
        let env = Envelope::new(FrameType::PaneOutput, payload);
        let buf = env.encode().unwrap();
        // Bytes 1..5 are the length, little-endian.
        assert_eq!(&buf[1..5], &len_value.to_le_bytes()[..]);
        // Sanity: the same bytes in BE would be a *different* prefix.
        assert_ne!(&buf[1..5], &len_value.to_be_bytes()[..]);
    }

    #[test]
    fn envelope_encode_rejects_oversize() {
        // Build a payload exactly at the cap — should encode fine. Then push
        // it one byte over and confirm we get PayloadTooLarge.
        let at_cap = Bytes::from(vec![0u8; MAX_PAYLOAD]);
        let env = Envelope::new(FrameType::PaneOutput, at_cap);
        assert!(env.encode().is_ok());
        let over_cap = Bytes::from(vec![0u8; MAX_PAYLOAD + 1]);
        let env = Envelope::new(FrameType::PaneOutput, over_cap);
        assert!(matches!(env.encode(), Err(CodecError::PayloadTooLarge(_))));
    }

    #[test]
    fn frame_type_from_u8_covers_all() {
        for &b in &[
            0x01u8, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x80, 0x81, 0x82, 0x83, 0x84,
        ] {
            let ft = FrameType::from_u8(b).expect("known frame");
            assert_eq!(ft.as_u8(), b);
        }
        // Reserved slots and undefined bytes must not decode.
        for &b in &[0x00u8, 0x08, 0x0F, 0x10, 0x7F, 0x85, 0x8F, 0xFE, 0xFF] {
            assert!(FrameType::from_u8(b).is_none(), "byte 0x{b:02x} leaked");
        }
    }

    #[test]
    fn frame_type_web_domain_flag() {
        assert!(!FrameType::PaneOutput.is_web_domain());
        assert!(FrameType::LayoutChanged.is_web_domain());
        assert!(FrameType::ManipulationSelection.is_web_domain());
    }

    // ── In-process WS handshake ──────────────────────────────────────────
    //
    // The next four tests spin up the real `router()` on an ephemeral port
    // and exercise the handshake with tokio-tungstenite. We don't bring up
    // the Origin/Host middleware here because those policies are a shared
    // tower-http layer applied at the top-level Router — this crate's
    // contract is to refuse on subprotocol/bearer terms alone.

    use std::net::{Ipv4Addr, SocketAddr};
    use tokio::net::TcpListener;
    use tokio_tungstenite::tungstenite::{
        client::IntoClientRequest, http::header::SEC_WEBSOCKET_PROTOCOL, protocol::Message as TM,
    };

    fn test_config() -> Config {
        // Build a minimal Local-mode config. We can't call any constructor
        // helper because Config has no builder — round-trip a TOML literal
        // through the loader instead.
        let toml = r#"schema_version = 1
[server]
session = "tests"
port = 9001
bind = "127.0.0.1"
"#;
        // figment::Jail isn't needed because we're not asserting on env;
        // load() reads `GTMUX_*` env but the absence of those vars is fine.
        let path = std::env::temp_dir().join(format!("gtmux-ws-test-{}.toml", std::process::id()));
        std::fs::write(&path, toml).unwrap();
        let cfg = gtmux_config::load(Some(&path), "tests").unwrap();
        let _ = std::fs::remove_file(&path);
        cfg
    }

    async fn spawn_test_server(token: TokenString) -> SocketAddr {
        let cfg = test_config();
        let app = router(&cfg, &token);
        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
            .await
            .unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        addr
    }

    #[tokio::test]
    async fn ws_upgrade_requires_protocol_header() {
        let token = gtmux_auth::issue_token().unwrap();
        let addr = spawn_test_server(token).await;
        let url = format!("ws://{addr}/ws");
        let req = url.into_client_request().unwrap();
        let res = tokio_tungstenite::connect_async(req).await;
        // tokio-tungstenite surfaces a non-101 response as an error; we just
        // need to see that the handshake didn't succeed.
        assert!(res.is_err(), "handshake without subprotocol must fail");
    }

    #[tokio::test]
    async fn ws_upgrade_wrong_token() {
        let token = gtmux_auth::issue_token().unwrap();
        let addr = spawn_test_server(token).await;
        let url = format!("ws://{addr}/ws");
        let mut req = url.into_client_request().unwrap();
        req.headers_mut().insert(
            SEC_WEBSOCKET_PROTOCOL,
            // A syntactically valid but unknown token. `verify_token`
            // decodes it (43 chars of `A` = 32 zero bytes) and finds it
            // doesn't match the issued token.
            "gtmux.v1, bearer.AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
                .parse()
                .unwrap(),
        );
        let res = tokio_tungstenite::connect_async(req).await;
        assert!(res.is_err(), "handshake with wrong token must fail");
    }

    #[tokio::test]
    async fn ws_upgrade_success_echoes_only_gtmux_v1() {
        let token = gtmux_auth::issue_token().unwrap();
        let addr = spawn_test_server(token.clone()).await;
        let url = format!("ws://{addr}/ws");
        let mut req = url.into_client_request().unwrap();
        let proto = format!("gtmux.v1, bearer.{}", token.0);
        req.headers_mut()
            .insert(SEC_WEBSOCKET_PROTOCOL, proto.parse().unwrap());
        let (ws, response) = tokio_tungstenite::connect_async(req)
            .await
            .expect("upgrade should succeed with valid token");
        // The response's Sec-WebSocket-Protocol header must be exactly
        // "gtmux.v1" — bearer.* MUST NOT echo (SSoT §6 step 5).
        let echoed = response
            .headers()
            .get(SEC_WEBSOCKET_PROTOCOL)
            .expect("protocol echoed")
            .to_str()
            .unwrap();
        assert_eq!(echoed, "gtmux.v1");
        drop(ws);
    }

    #[tokio::test]
    async fn ws_close_code_for_invalid_frame_from_client() {
        let token = gtmux_auth::issue_token().unwrap();
        let addr = spawn_test_server(token.clone()).await;
        let url = format!("ws://{addr}/ws");
        let mut req = url.into_client_request().unwrap();
        let proto = format!("gtmux.v1, bearer.{}", token.0);
        req.headers_mut()
            .insert(SEC_WEBSOCKET_PROTOCOL, proto.parse().unwrap());
        let (mut ws, _resp) = tokio_tungstenite::connect_async(req).await.unwrap();
        // Drain the initial server-side LAYOUT_CHANGED hello so the next
        // message we read is the close response.
        let _hello = ws.next().await;
        // Client sends 0x80 LAYOUT_CHANGED — server-only, must close 1008.
        let bad = Envelope::new(FrameType::LayoutChanged, Bytes::from_static(&[0u8; 16]))
            .encode()
            .unwrap();
        ws.send(TM::Binary(bad.to_vec().into())).await.unwrap();
        let mut got_policy_close = false;
        while let Some(msg) = ws.next().await {
            match msg {
                Ok(TM::Close(Some(cf))) => {
                    assert_eq!(u16::from(cf.code), close_codes::POLICY_VIOLATION);
                    got_policy_close = true;
                    break;
                }
                Ok(TM::Close(None)) => break,
                Ok(_) => continue,
                Err(_) => break,
            }
        }
        assert!(got_policy_close, "expected explicit 1008 close frame");
    }
}
