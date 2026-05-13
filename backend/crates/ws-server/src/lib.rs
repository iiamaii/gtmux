//! gtmux-ws-server — axum `/ws` upgrade + envelope codec + Hub broadcaster.
//!
//! Surface (Sprint 4-B WIRE-2/3):
//! - [`router`] mounts `GET /ws` with Origin/Host gating and subprotocol-based
//!   bearer auth (ADR-0002 D5 + ADR-0003 D5, `docs/ssot/security-defaults.md` §6).
//!   New signature accepts a [`Hub`] and a [`tokio::sync::mpsc::Sender`] of
//!   [`cmd_router::TmuxRequest`] so the handler can fan out tmux Events to
//!   the WS sink and emit user-origin commands back to the daemon.
//! - [`Envelope`] is the wire object that flows through the binary frame.
//!   See `docs/ssot/wire-protocol.md` §1.2 for the byte layout (the on-wire
//!   `paneId` varint lives *inside* the payload bytes — this crate carries it
//!   transparently; envelope encode/decode is the framing-level concern only).
//! - [`Hub`] is the fan-out point: mux-router [`gtmux_mux_router::Event`]s
//!   come in, web subscribers see the matching envelope sequence out.
//! - [`parse_subprotocol`] implements the RFC 6455 §11.3.4 comma-separated
//!   list semantics ADR-0003 D5 mandates: `"gtmux.v1, bearer.<token>"`.
//!
//! Out of scope for this sprint:
//! - The real tmux daemon read loop wiring (lifecycle::run_event_loop) and
//!   write loop wiring (lifecycle::run_command_loop) are owned by the
//!   `gtmux_lifecycle` crate. This crate only consumes the Hub and the
//!   `mpsc::Sender<TmuxRequest>` they hand it.
//! - HTTP cookie bootstrap exchange (P0-HTTP-2, owned by `gtmux-http-api`).

#![forbid(unsafe_code)]
#![deny(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashMap;
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
use gtmux_mux_router::Event;
use thiserror::Error;
use tokio::sync::mpsc;
use tokio::time::Instant;
use tracing::{debug, info, warn};

pub mod cmd_router;
mod hub;
mod payload;
mod ring;
mod varint;

pub use cmd_router::{
    build_ctrl_request, build_pane_in_request, build_pane_pause_request, build_pane_resize_request,
    build_pane_resume_request, is_allowed_ctrl_cmd, TmuxRequest, ALLOWLISTED_CTRL_CMDS,
};
pub use hub::{Hub, HUB_BROADCAST_CAPACITY};
pub use ring::{RingBuffer, RING_BUFFER_CAPACITY};

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

/// Panel Streaming State pause/resume debounce window (ADR-0001 D8 +
/// `docs/reports/0010-grill-amendments.md` D16). Duplicate 0x05 / 0x06
/// frames arriving within this window for the same pane id collapse to a
/// single tmux command — protects the daemon from a rapid-toggle storm.
const PAUSE_DEBOUNCE: Duration = Duration::from_millis(300);

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
    /// Daemon exited (received `%exit`). Custom code in the 4xxx user range
    /// per RFC 6455 — reusable by the FE for "daemon dead" UX.
    pub const DAEMON_EXITED: u16 = 1011;
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
/// payload parsing happens in the routing layer (see [`payload`] module).
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
//  Event → Envelope mapping
// ─────────────────────────────────────────────────────────────────────────────

/// Translate a single mux-router [`Event`] into one or more envelopes for
/// broadcast. Returns `None` for events that have no SSoT-defined frame
/// (e.g. `Event::Continue`, `Event::Unknown`) — forward-compat per SSoT §6.
///
/// `Event::Exit` is handled specially by the WS handler (closes the
/// connection with 1011 `daemon-exited`); this function returns `None` so
/// the broadcast loop does not emit a stale envelope on the way out.
pub fn event_to_envelope(event: &Event) -> Option<Envelope> {
    match event {
        Event::Output { pane_id, bytes } => Some(Envelope::new(
            FrameType::PaneOutput,
            Bytes::from(payload::encode_pane_out(*pane_id, bytes)),
        )),
        Event::ExtendedOutput { pane_id, bytes, .. } => Some(Envelope::new(
            FrameType::PaneOutput,
            Bytes::from(payload::encode_pane_out(*pane_id, bytes)),
        )),
        Event::Pause { pane_id } => {
            let body = r#"{"kind":"slow-pane"}"#;
            Some(Envelope::new(
                FrameType::NotifyMirror,
                Bytes::from(payload::encode_notify_mirror(*pane_id, body)),
            ))
        }
        Event::PaneDead { pane_id } => {
            let body = r#"{"kind":"pane-died"}"#;
            Some(Envelope::new(
                FrameType::NotifyMirror,
                Bytes::from(payload::encode_notify_mirror(*pane_id, body)),
            ))
        }
        Event::WindowAdd { window_id } => {
            let body = format!(r#"{{"kind":"window-add","window_id":"@{window_id}","name":""}}"#);
            Some(Envelope::new(
                FrameType::NotifyMirror,
                Bytes::from(payload::encode_notify_mirror(0, &body)),
            ))
        }
        Event::WindowClose { window_id } => {
            let body = format!(r#"{{"kind":"window-close","window_id":"@{window_id}"}}"#);
            Some(Envelope::new(
                FrameType::NotifyMirror,
                Bytes::from(payload::encode_notify_mirror(0, &body)),
            ))
        }
        Event::WindowRenamed { window_id, name } => {
            let escaped = json_escape(name);
            let body = format!(
                r#"{{"kind":"window-renamed","window_id":"@{window_id}","name":"{escaped}"}}"#,
            );
            Some(Envelope::new(
                FrameType::NotifyMirror,
                Bytes::from(payload::encode_notify_mirror(0, &body)),
            ))
        }
        Event::SessionChanged { session_id, name } => {
            let escaped = json_escape(name);
            let body = format!(
                r#"{{"kind":"session-changed","session_id":"${session_id}","name":"{escaped}"}}"#,
            );
            Some(Envelope::new(
                FrameType::NotifyMirror,
                Bytes::from(payload::encode_notify_mirror(0, &body)),
            ))
        }
        Event::LayoutChange { window_id, layout } => {
            let escaped = json_escape(layout);
            let body = format!(
                r#"{{"kind":"layout-change","window_id":"@{window_id}","layout":"{escaped}"}}"#,
            );
            Some(Envelope::new(
                FrameType::NotifyMirror,
                Bytes::from(payload::encode_notify_mirror(0, &body)),
            ))
        }
        Event::SessionsChanged => {
            let body = r#"{"kind":"sessions-changed"}"#.to_string();
            Some(Envelope::new(
                FrameType::NotifyMirror,
                Bytes::from(payload::encode_notify_mirror(0, &body)),
            ))
        }
        // `Continue` has no SSoT-defined frame yet (Event::Pause's
        // `slow-pane` mirror does not have a paired "resumed" kind in
        // §2.3) — drop forward-compat. See SSoT §6.
        Event::Continue { .. } => None,
        // `Exit` is consumed by the WS handler outside the broadcast loop
        // (it triggers a close frame, not an envelope).
        Event::Exit { .. } => None,
        Event::Unknown => None,
    }
}

/// Minimal JSON-string escaper — covers backslash, double-quote, and the
/// C0 control bytes that JSON forbids verbatim. Used for the small bag of
/// `name`/`layout` strings we embed into NOTIFY_MIRROR bodies; full JSON
/// objects go through `serde_json` elsewhere.
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
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
            if !t.is_empty() && bearer_token.is_none() {
                // Take the *first* non-empty bearer.* we see — repeated
                // bearer.* tokens in one header are not a valid client
                // shape and we refuse to silently coalesce them.
                bearer_token = Some(t.to_string());
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
    /// Hub for event subscription and ring-buffer catch-up replay.
    hub: Hub,
    /// Sender side of the lifecycle command loop. Cloned per-connection.
    cmd_tx: mpsc::Sender<TmuxRequest>,
}

/// Build the WS sub-router. Mount onto the top-level `axum::Router` and apply
/// Origin/Host middleware externally — those policies are shared with the
/// HTTP API surface (ADR-0002 D6 + `docs/ssot/security-defaults.md` §1.2),
/// so each crate must not re-implement them.
///
/// `token` is the server's stored token; the handler verifies the client's
/// `bearer.<...>` against it in constant time via `gtmux_auth::verify_token`.
/// `hub` is the fan-out hub the WS handler subscribes to; `cmd_tx` is the
/// outbound channel into the lifecycle command writer (single-writer
/// constraint preserved by mpsc back-pressure).
pub fn router(
    _config: &Config,
    token: &TokenString,
    hub: Hub,
    cmd_tx: mpsc::Sender<TmuxRequest>,
) -> Router {
    // Pre-build the echo header value so handle_upgrade doesn't have to
    // re-validate "gtmux.v1" on every connection.
    let echo_protocol = HeaderValue::from_static("gtmux.v1");
    let state = WsState {
        token: Arc::new(token.clone()),
        echo_protocol,
        hub,
        cmd_tx,
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
    let hub = state.hub.clone();
    let cmd_tx = state.cmd_tx.clone();
    let mut response = ws
        .protocols(["gtmux.v1"])
        .on_upgrade(move |socket| async move {
            handle_socket(socket, hub, cmd_tx).await;
        });
    // axum's `protocols(...)` already echoes the matched value; we also set
    // it explicitly so the response header is present even if axum's matching
    // logic changes shape across minor versions. The bearer.* sub-token is
    // intentionally NOT echoed — SSoT §6 step 5.
    response
        .headers_mut()
        .insert("sec-websocket-protocol", echo);
    response
}

/// Per-connection loop. Performs catch-up replay on attach (every pane's
/// ring buffer is flushed to this client as a sequence of 0x02 PANE_OUT
/// envelopes), then enters the live fan-out: hub events are translated to
/// envelopes and pushed to the sink; inbound envelopes are routed to the
/// tmux command channel.
async fn handle_socket(socket: WebSocket, hub: Hub, cmd_tx: mpsc::Sender<TmuxRequest>) {
    let (mut sink, mut stream) = socket.split();

    // Subscribe BEFORE the catch-up replay so events that arrive while we
    // are flushing snapshots are not lost.
    let mut rx = hub.subscribe();

    // Send the initial LAYOUT_CHANGED envelope so the client knows the
    // server is alive and can decide whether to re-fetch `/api/layout`.
    // Payload is a varint-0 + all-zeros 16-byte ETag sentinel.
    let hello_etag = [0u8; 16];
    let hello = Envelope::new(
        FrameType::LayoutChanged,
        Bytes::from(payload::encode_layout_changed(&hello_etag)),
    );
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

    // Catch-up replay: every pane's ring buffer becomes a 0x02 PANE_OUT
    // envelope. We do this *after* the LAYOUT_CHANGED hello so the client's
    // dispatcher is already in steady-state mode.
    let snapshots = hub.snapshot_all().await;
    for (pane_id, bytes) in snapshots {
        let env = Envelope::new(
            FrameType::PaneOutput,
            Bytes::from(payload::encode_pane_out(pane_id, &bytes)),
        );
        if let Ok(buf) = env.encode() {
            if sink
                .send(Message::Binary(buf.to_vec().into()))
                .await
                .is_err()
            {
                debug!("ws catch-up send failed; peer hung up during replay");
                return;
            }
        }
    }

    let mut ping_timer = tokio::time::interval(PING_INTERVAL);
    ping_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    // Skip the immediate first tick — the hello frame above is the
    // server's liveness signal for the first 30 s.
    ping_timer.tick().await;
    let mut last_pong = Instant::now();

    // Pause/resume debounce state — per pane id, last time we forwarded a
    // pause/resume command. The 300 ms window collapses duplicate frames
    // from a noisy UI (rapid hide/show toggle) into a single tmux command.
    let mut last_pause_event: HashMap<u32, Instant> = HashMap::new();

    loop {
        tokio::select! {
            biased; // Drain inbound first so pongs reset `last_pong` before
                    // the next ping decision.
            maybe_msg = stream.next() => {
                let Some(msg) = maybe_msg else { break };
                match msg {
                    Ok(Message::Binary(buf)) => {
                        let close_now = handle_client_envelope(
                            buf.as_ref(),
                            &cmd_tx,
                            &mut last_pause_event,
                            &mut sink,
                        ).await;
                        if close_now {
                            return;
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
            event = rx.recv() => {
                match event {
                    Ok(Event::Exit { reason }) => {
                        info!(?reason, "ws closing: tmux daemon exited");
                        let _ = sink.send(close_frame(
                            close_codes::DAEMON_EXITED,
                            "daemon-exited",
                        )).await;
                        return;
                    }
                    Ok(ev) => {
                        if let Some(env) = event_to_envelope(&ev) {
                            if let Ok(buf) = env.encode() {
                                if sink.send(Message::Binary(buf.to_vec().into())).await.is_err() {
                                    break;
                                }
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        // Slow subscriber: log + continue. Future work
                        // (S4-D) will send a NOTIFY_MIRROR `client-lagged`
                        // frame so the FE can decide to re-subscribe.
                        warn!(skipped = n, "ws subscriber lagged; events dropped");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        info!("hub channel closed; ending connection");
                        let _ = sink.send(close_frame(
                            close_codes::INTERNAL,
                            "hub closed",
                        )).await;
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

/// Handle one client-origin binary frame. Returns `true` when the caller
/// must close the connection (a policy violation already sent its close
/// frame on `sink`).
async fn handle_client_envelope(
    buf: &[u8],
    cmd_tx: &mpsc::Sender<TmuxRequest>,
    last_pause_event: &mut HashMap<u32, Instant>,
    sink: &mut (impl SinkExt<Message, Error = axum::Error> + Unpin),
) -> bool {
    let (env, _) = match Envelope::decode(buf) {
        Ok(t) => t,
        Err(e) => {
            debug!("ws decode error: {e}");
            let _ = sink
                .send(close_frame(
                    close_codes::UNSUPPORTED_DATA,
                    "malformed envelope",
                ))
                .await;
            return true;
        }
    };

    match env.kind {
        FrameType::PaneInput => {
            let Some(p) = payload::decode_pane_in(&env.payload) else {
                let _ = sink
                    .send(close_frame(
                        close_codes::UNSUPPORTED_DATA,
                        "malformed PANE_IN",
                    ))
                    .await;
                return true;
            };
            if !p.bytes.is_empty() {
                let req = build_pane_in_request(p.pane_id, p.bytes);
                let _ = cmd_tx.send(req).await;
            }
            false
        }
        FrameType::PaneResize => {
            let Some(r) = payload::decode_pane_resize(&env.payload) else {
                let _ = sink
                    .send(close_frame(
                        close_codes::UNSUPPORTED_DATA,
                        "malformed PANE_RESIZE",
                    ))
                    .await;
                return true;
            };
            let req = build_pane_resize_request(r.pane_id, r.cols, r.rows);
            let _ = cmd_tx.send(req).await;
            false
        }
        FrameType::PanePause => {
            let Some(pane_id) = payload::decode_pane_bare_id(&env.payload) else {
                let _ = sink
                    .send(close_frame(
                        close_codes::UNSUPPORTED_DATA,
                        "malformed PANE_PAUSE",
                    ))
                    .await;
                return true;
            };
            if !debounce_pause(last_pause_event, pane_id) {
                let req = build_pane_pause_request(pane_id);
                let _ = cmd_tx.send(req).await;
            }
            false
        }
        FrameType::PaneResume => {
            let Some(pane_id) = payload::decode_pane_bare_id(&env.payload) else {
                let _ = sink
                    .send(close_frame(
                        close_codes::UNSUPPORTED_DATA,
                        "malformed PANE_RESUME",
                    ))
                    .await;
                return true;
            };
            if !debounce_pause(last_pause_event, pane_id) {
                let req = build_pane_resume_request(pane_id);
                let _ = cmd_tx.send(req).await;
            }
            false
        }
        FrameType::Ctrl => {
            let Some(ctrl) = payload::decode_ctrl_request(&env.payload) else {
                let body = payload::encode_ctrl_error(None, "ERR_BAD_REQUEST", "invalid CTRL JSON");
                let err = Envelope::new(FrameType::Ctrl, Bytes::from(body));
                if let Ok(buf) = err.encode() {
                    let _ = sink.send(Message::Binary(buf.to_vec().into())).await;
                }
                return false;
            };
            if !is_allowed_ctrl_cmd(&ctrl.cmd) {
                let body = payload::encode_ctrl_error(
                    ctrl.id.as_deref(),
                    "ERR_NOT_ALLOWED",
                    "command not in allowlist",
                );
                let err = Envelope::new(FrameType::Ctrl, Bytes::from(body));
                if let Ok(buf) = err.encode() {
                    let _ = sink.send(Message::Binary(buf.to_vec().into())).await;
                }
                return false;
            }
            if let Some(req) = build_ctrl_request(ctrl.id, &ctrl.cmd, ctrl.args) {
                let _ = cmd_tx.send(req).await;
            }
            false
        }
        FrameType::LayoutChanged => {
            // Server-only. Receiving from a client is a policy violation
            // (SSoT §2.2).
            let _ = sink
                .send(close_frame(
                    close_codes::POLICY_VIOLATION,
                    "0x80 LAYOUT_CHANGED is server-only",
                ))
                .await;
            true
        }
        FrameType::ManipulationSelection
        | FrameType::InputTarget
        | FrameType::ViewportChanged
        | FrameType::FocusMode => {
            // Web-domain MT-3 broadcast: out of scope for this sprint —
            // the FE peers will get a dedicated relay in S4-C. For now we
            // accept and ignore so existing FE prototypes do not error.
            debug!(
                kind = ?env.kind,
                bytes = env.payload.len(),
                "ws web-domain envelope received (S4-C placeholder)",
            );
            false
        }
        FrameType::PaneOutput | FrameType::NotifyMirror => {
            // Server-origin only. A client publishing these is a protocol
            // violation.
            let _ = sink
                .send(close_frame(
                    close_codes::POLICY_VIOLATION,
                    "frame is server-only",
                ))
                .await;
            true
        }
    }
}

/// Return `true` if this pause/resume event should be debounced (i.e. a
/// previous one fired within [`PAUSE_DEBOUNCE`] for this pane).
fn debounce_pause(state: &mut HashMap<u32, Instant>, pane_id: u32) -> bool {
    let now = Instant::now();
    match state.get_mut(&pane_id) {
        Some(prev) if now.duration_since(*prev) < PAUSE_DEBOUNCE => true,
        Some(prev) => {
            *prev = now;
            false
        }
        None => {
            state.insert(pane_id, now);
            false
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
        assert!(parse_subprotocol(",").is_none());
        assert!(parse_subprotocol("  ,  ").is_none());
        let p = parse_subprotocol("bearer.").unwrap();
        assert!(!p.gtmux_v1);
        assert!(p.bearer_token.is_none());
        let p = parse_subprotocol("Gtmux.V1, BEARER.xyz").unwrap();
        assert!(!p.gtmux_v1);
        assert!(p.bearer_token.is_none());
    }

    #[test]
    fn parse_subprotocol_ignores_unknown_tokens() {
        let p = parse_subprotocol("gtmux.v1, x-future-extension, bearer.t").unwrap();
        assert!(p.gtmux_v1);
        assert_eq!(p.bearer_token.as_deref(), Some("t"));
    }

    // ── Codec ─────────────────────────────────────────────────────────────

    #[test]
    fn envelope_encode_decode_roundtrip() {
        let inner = payload::encode_pane_out(37, &[0x41u8; 100]);
        let env = Envelope::new(FrameType::PaneOutput, Bytes::from(inner.clone()));
        let buf = env.encode().unwrap();
        assert_eq!(buf.len(), HEADER_LEN + inner.len());
        let (decoded, consumed) = Envelope::decode(&buf).unwrap();
        assert_eq!(consumed, HEADER_LEN + inner.len());
        assert_eq!(decoded.kind, FrameType::PaneOutput);
        assert_eq!(decoded.payload.as_ref(), inner.as_slice());
    }

    #[test]
    fn envelope_decode_truncated_returns_err() {
        assert_eq!(Envelope::decode(&[0x02]), Err(CodecError::Truncated));
        let mut buf = vec![0x02u8];
        buf.extend_from_slice(&10u32.to_le_bytes());
        assert_eq!(Envelope::decode(&buf), Err(CodecError::Truncated));
        buf.extend_from_slice(b"abc");
        assert_eq!(Envelope::decode(&buf), Err(CodecError::Truncated));
    }

    #[test]
    fn envelope_decode_unknown_type() {
        let mut buf = vec![0x42u8];
        buf.extend_from_slice(&0u32.to_le_bytes());
        assert_eq!(Envelope::decode(&buf), Err(CodecError::UnknownType(0x42)));
        let mut buf = vec![0x08u8];
        buf.extend_from_slice(&0u32.to_le_bytes());
        assert_eq!(Envelope::decode(&buf), Err(CodecError::UnknownType(0x08)));
        let mut buf = vec![0x85u8];
        buf.extend_from_slice(&0u32.to_le_bytes());
        assert_eq!(Envelope::decode(&buf), Err(CodecError::UnknownType(0x85)));
    }

    #[test]
    fn envelope_decode_oversized() {
        let oversize = (MAX_PAYLOAD as u32) + 1;
        let mut buf = vec![0x02u8];
        buf.extend_from_slice(&oversize.to_le_bytes());
        assert_eq!(
            Envelope::decode(&buf),
            Err(CodecError::PayloadTooLarge(oversize))
        );
    }

    #[test]
    fn envelope_payload_endianness_le() {
        let len_value = 0x000400FFu32;
        let payload = Bytes::from(vec![0u8; len_value as usize]);
        let env = Envelope::new(FrameType::PaneOutput, payload);
        let buf = env.encode().unwrap();
        assert_eq!(&buf[1..5], &len_value.to_le_bytes()[..]);
        assert_ne!(&buf[1..5], &len_value.to_be_bytes()[..]);
    }

    #[test]
    fn envelope_encode_rejects_oversize() {
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

    // ── Event → Envelope mapping ─────────────────────────────────────────

    #[test]
    fn event_to_envelope_output() {
        let ev = Event::Output {
            pane_id: 37,
            bytes: b"hello".to_vec(),
        };
        let env = event_to_envelope(&ev).unwrap();
        assert_eq!(env.kind, FrameType::PaneOutput);
        // varint(37) = 0x25 + "hello"
        assert_eq!(env.payload.as_ref(), &[0x25, b'h', b'e', b'l', b'l', b'o']);
    }

    #[test]
    fn event_to_envelope_extended_output_same_shape() {
        let a = event_to_envelope(&Event::Output {
            pane_id: 37,
            bytes: b"abc".to_vec(),
        })
        .unwrap();
        let b = event_to_envelope(&Event::ExtendedOutput {
            pane_id: 37,
            age_ms: 100,
            bytes: b"abc".to_vec(),
        })
        .unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn event_to_envelope_pause_emits_slow_pane() {
        let env = event_to_envelope(&Event::Pause { pane_id: 5 }).unwrap();
        assert_eq!(env.kind, FrameType::NotifyMirror);
        // After varint(5), the body is the JSON.
        let body = &env.payload[1..];
        let json: serde_json::Value = serde_json::from_slice(body).unwrap();
        assert_eq!(json["kind"], "slow-pane");
    }

    #[test]
    fn event_to_envelope_layout_change_includes_layout_string() {
        let env = event_to_envelope(&Event::LayoutChange {
            window_id: 1,
            layout: "b25e,80x24,0,0,1".into(),
        })
        .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&env.payload[1..]).unwrap();
        assert_eq!(json["kind"], "layout-change");
        assert_eq!(json["layout"], "b25e,80x24,0,0,1");
        assert_eq!(json["window_id"], "@1");
    }

    #[test]
    fn event_to_envelope_continue_returns_none() {
        // Per the SSoT decision: `slow-pane-resumed` is not in §2.3, so we
        // drop `Continue` events (forward-compat).
        assert!(event_to_envelope(&Event::Continue { pane_id: 5 }).is_none());
    }

    #[test]
    fn event_to_envelope_unknown_returns_none() {
        assert!(event_to_envelope(&Event::Unknown).is_none());
    }

    #[test]
    fn event_to_envelope_exit_returns_none() {
        // Exit triggers a close frame, not an envelope.
        assert!(event_to_envelope(&Event::Exit { reason: None }).is_none());
    }

    #[test]
    fn event_to_envelope_round_trip_all_12_frames() {
        // Build one envelope per frame slot (server-origin set) and decode
        // back through the outer codec — guards against payload formats
        // drifting from the SSoT byte representation.
        let cases: Vec<Envelope> = vec![
            event_to_envelope(&Event::Output {
                pane_id: 1,
                bytes: b"x".to_vec(),
            })
            .unwrap(),
            event_to_envelope(&Event::Pause { pane_id: 1 }).unwrap(),
            event_to_envelope(&Event::PaneDead { pane_id: 1 }).unwrap(),
            event_to_envelope(&Event::WindowAdd { window_id: 1 }).unwrap(),
            event_to_envelope(&Event::WindowClose { window_id: 1 }).unwrap(),
            event_to_envelope(&Event::WindowRenamed {
                window_id: 1,
                name: "n".into(),
            })
            .unwrap(),
            event_to_envelope(&Event::SessionChanged {
                session_id: 0,
                name: "s".into(),
            })
            .unwrap(),
            event_to_envelope(&Event::LayoutChange {
                window_id: 1,
                layout: "l".into(),
            })
            .unwrap(),
            event_to_envelope(&Event::SessionsChanged).unwrap(),
        ];
        for env in cases {
            let buf = env.encode().unwrap();
            let (decoded, _) = Envelope::decode(&buf).unwrap();
            assert_eq!(decoded, env);
        }
        // Client-origin / web-domain frames: build by hand and confirm the
        // outer codec round-trips them too.
        let etag = [0x11u8; 16];
        let layout = Envelope::new(
            FrameType::LayoutChanged,
            Bytes::from(payload::encode_layout_changed(&etag)),
        );
        let buf = layout.encode().unwrap();
        let (decoded, _) = Envelope::decode(&buf).unwrap();
        assert_eq!(decoded, layout);
    }

    // ── Hub broadcast ────────────────────────────────────────────────────

    #[tokio::test]
    async fn hub_broadcast_to_multiple_subscribers() {
        let hub = Hub::new();
        let mut a = hub.subscribe();
        let mut b = hub.subscribe();
        hub.publish(Event::Pause { pane_id: 9 }).await;
        let ev_a = a.recv().await.unwrap();
        let ev_b = b.recv().await.unwrap();
        assert_eq!(ev_a, Event::Pause { pane_id: 9 });
        assert_eq!(ev_b, Event::Pause { pane_id: 9 });
    }

    #[tokio::test]
    async fn ring_buffer_oldest_drop_via_hub() {
        let hub = Hub::new();
        hub.publish(Event::Output {
            pane_id: 1,
            bytes: vec![b'A'; RING_BUFFER_CAPACITY],
        })
        .await;
        hub.publish(Event::Output {
            pane_id: 1,
            bytes: vec![b'B'; 100],
        })
        .await;
        let snap = hub.snapshot(1).await.unwrap();
        assert_eq!(snap.len(), RING_BUFFER_CAPACITY);
        assert!(snap.ends_with(&[b'B'; 100]));
    }

    // ── In-process WS handshake ──────────────────────────────────────────

    use std::net::{Ipv4Addr, SocketAddr};
    use tokio::net::TcpListener;
    use tokio_tungstenite::tungstenite::{
        client::IntoClientRequest, http::header::SEC_WEBSOCKET_PROTOCOL, protocol::Message as TM,
    };

    /// Per-call unique counter so concurrent tokio tests don't race on the
    /// shared `std::process::id()`-derived path.
    static CONFIG_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    fn test_config() -> Config {
        let toml = r#"schema_version = 1
[server]
session = "tests"
port = 9001
bind = "127.0.0.1"
"#;
        let n = CONFIG_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("gtmux-ws-test-{}-{}.toml", std::process::id(), n,));
        std::fs::write(&path, toml).unwrap();
        let cfg = gtmux_config::load(Some(&path), "tests").unwrap();
        let _ = std::fs::remove_file(&path);
        cfg
    }

    async fn spawn_test_server(
        token: TokenString,
    ) -> (SocketAddr, Hub, mpsc::Receiver<TmuxRequest>) {
        let cfg = test_config();
        let hub = Hub::new();
        let (cmd_tx, cmd_rx) = mpsc::channel::<TmuxRequest>(32);
        let app = router(&cfg, &token, hub.clone(), cmd_tx);
        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
            .await
            .unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (addr, hub, cmd_rx)
    }

    async fn connect_authed(
        addr: SocketAddr,
        token: &TokenString,
    ) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>
    {
        let url = format!("ws://{addr}/ws");
        let mut req = url.into_client_request().unwrap();
        let proto = format!("gtmux.v1, bearer.{}", token.0);
        req.headers_mut()
            .insert(SEC_WEBSOCKET_PROTOCOL, proto.parse().unwrap());
        let (ws, _resp) = tokio_tungstenite::connect_async(req).await.unwrap();
        ws
    }

    #[tokio::test]
    async fn ws_upgrade_requires_protocol_header() {
        let token = gtmux_auth::issue_token().unwrap();
        let (addr, _hub, _rx) = spawn_test_server(token).await;
        let url = format!("ws://{addr}/ws");
        let req = url.into_client_request().unwrap();
        let res = tokio_tungstenite::connect_async(req).await;
        assert!(res.is_err(), "handshake without subprotocol must fail");
    }

    #[tokio::test]
    async fn ws_upgrade_wrong_token() {
        let token = gtmux_auth::issue_token().unwrap();
        let (addr, _hub, _rx) = spawn_test_server(token).await;
        let url = format!("ws://{addr}/ws");
        let mut req = url.into_client_request().unwrap();
        req.headers_mut().insert(
            SEC_WEBSOCKET_PROTOCOL,
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
        let (addr, _hub, _rx) = spawn_test_server(token.clone()).await;
        let url = format!("ws://{addr}/ws");
        let mut req = url.into_client_request().unwrap();
        let proto = format!("gtmux.v1, bearer.{}", token.0);
        req.headers_mut()
            .insert(SEC_WEBSOCKET_PROTOCOL, proto.parse().unwrap());
        let (ws, response) = tokio_tungstenite::connect_async(req)
            .await
            .expect("upgrade should succeed with valid token");
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
    async fn client_origin_layout_changed_closes_1008() {
        let token = gtmux_auth::issue_token().unwrap();
        let (addr, _hub, _rx) = spawn_test_server(token.clone()).await;
        let mut ws = connect_authed(addr, &token).await;
        // Drain the initial hello.
        let _hello = ws.next().await;
        let bad = Envelope::new(
            FrameType::LayoutChanged,
            Bytes::from(payload::encode_layout_changed(&[0u8; 16])),
        )
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

    #[tokio::test]
    async fn catch_up_replay_on_new_attach() {
        let token = gtmux_auth::issue_token().unwrap();
        let (addr, hub, _rx) = spawn_test_server(token.clone()).await;
        // Seed two panes with output before any client connects.
        hub.publish(Event::Output {
            pane_id: 1,
            bytes: b"first".to_vec(),
        })
        .await;
        hub.publish(Event::Output {
            pane_id: 2,
            bytes: b"second".to_vec(),
        })
        .await;
        let mut ws = connect_authed(addr, &token).await;
        // Read the LAYOUT_CHANGED hello frame.
        let hello = expect_binary(&mut ws).await;
        let (env, _) = Envelope::decode(&hello).unwrap();
        assert_eq!(env.kind, FrameType::LayoutChanged);
        // Now two PANE_OUT frames in pane-id order (1 then 2).
        let f1 = expect_binary(&mut ws).await;
        let (e1, _) = Envelope::decode(&f1).unwrap();
        assert_eq!(e1.kind, FrameType::PaneOutput);
        assert_eq!(e1.payload.as_ref(), &[0x01, b'f', b'i', b'r', b's', b't']);
        let f2 = expect_binary(&mut ws).await;
        let (e2, _) = Envelope::decode(&f2).unwrap();
        assert_eq!(e2.kind, FrameType::PaneOutput);
        assert_eq!(
            e2.payload.as_ref(),
            &[0x02, b's', b'e', b'c', b'o', b'n', b'd']
        );
    }

    #[tokio::test]
    async fn client_pane_in_routed_to_command() {
        let token = gtmux_auth::issue_token().unwrap();
        let (addr, _hub, mut rx) = spawn_test_server(token.clone()).await;
        let mut ws = connect_authed(addr, &token).await;
        // Drain hello.
        let _hello = expect_binary(&mut ws).await;
        // Send 0x03 PANE_IN: varint paneId=7 + bytes "ls\n".
        let inner = payload::encode_pane_out(7, b"ls\n");
        let frame = Envelope::new(FrameType::PaneInput, Bytes::from(inner))
            .encode()
            .unwrap();
        ws.send(TM::Binary(frame.to_vec().into())).await.unwrap();
        // Expect a TmuxRequest on the cmd channel.
        let got = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
            .await
            .expect("command should arrive")
            .expect("channel open");
        assert!(
            matches!(got.command, gtmux_mux_router::Command::SendKeys),
            "expected SendKeys, got {:?}",
            got.command,
        );
        assert!(got.args.iter().any(|a| a == "%7"));
        assert!(got.args.iter().any(|a| a == "ls\n"));
    }

    #[tokio::test]
    async fn disallowed_ctrl_cmd_rejected() {
        let token = gtmux_auth::issue_token().unwrap();
        let (addr, _hub, mut rx) = spawn_test_server(token.clone()).await;
        let mut ws = connect_authed(addr, &token).await;
        let _hello = expect_binary(&mut ws).await;
        let body = br#"{"id":"r1","cmd":"split-window","args":[]}"#;
        let mut inner = vec![0u8];
        inner.extend_from_slice(body);
        let frame = Envelope::new(FrameType::Ctrl, Bytes::from(inner))
            .encode()
            .unwrap();
        ws.send(TM::Binary(frame.to_vec().into())).await.unwrap();
        // Expect a CTRL error response (ok:false).
        let resp = expect_binary(&mut ws).await;
        let (env, _) = Envelope::decode(&resp).unwrap();
        assert_eq!(env.kind, FrameType::Ctrl);
        // varint(0) + JSON body
        assert_eq!(env.payload[0], 0x00);
        let json: serde_json::Value = serde_json::from_slice(&env.payload[1..]).unwrap();
        assert_eq!(json["ok"], false);
        assert_eq!(json["code"], "ERR_NOT_ALLOWED");
        // No tmux command should have arrived.
        let race = tokio::time::timeout(std::time::Duration::from_millis(200), rx.recv()).await;
        assert!(
            race.is_err(),
            "disallowed cmd must not produce a TmuxRequest"
        );
    }

    #[tokio::test]
    async fn allowed_ctrl_cmd_routed_to_command() {
        let token = gtmux_auth::issue_token().unwrap();
        let (addr, _hub, mut rx) = spawn_test_server(token.clone()).await;
        let mut ws = connect_authed(addr, &token).await;
        let _hello = expect_binary(&mut ws).await;
        let body = br#"{"id":"r2","cmd":"new-window","args":["-t","s"]}"#;
        let mut inner = vec![0u8];
        inner.extend_from_slice(body);
        let frame = Envelope::new(FrameType::Ctrl, Bytes::from(inner))
            .encode()
            .unwrap();
        ws.send(TM::Binary(frame.to_vec().into())).await.unwrap();
        let got = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
            .await
            .expect("command should arrive")
            .expect("channel open");
        assert!(matches!(got.command, gtmux_mux_router::Command::NewWindow));
        assert_eq!(got.args, vec!["-t".to_string(), "s".to_string()]);
        assert_eq!(got.id.as_deref(), Some("r2"));
    }

    // Helper — read one Binary frame from the WS sink, panic on anything else.
    async fn expect_binary(
        ws: &mut tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    ) -> Vec<u8> {
        loop {
            match ws.next().await {
                Some(Ok(TM::Binary(b))) => return b.to_vec(),
                Some(Ok(TM::Ping(p))) => {
                    let _ = ws.send(TM::Pong(p)).await;
                }
                Some(Ok(TM::Pong(_))) => continue,
                Some(Ok(other)) => panic!("expected Binary, got {other:?}"),
                Some(Err(e)) => panic!("ws err: {e}"),
                None => panic!("ws closed unexpectedly"),
            }
        }
    }
}
