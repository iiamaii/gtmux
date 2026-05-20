//! Inner-payload codecs for the 12 envelope frame types.
//!
//! The outer codec ([`crate::Envelope::encode`] / [`crate::Envelope::decode`]) frames
//! one envelope as `[1B type][4B LE u32 length][inner bytes]`. *This module*
//! handles the contents of `inner bytes`, which follow the per-type schema
//! defined in `docs/ssot/wire-protocol.md` §2 and the `kind` enum in §2.3.
//!
//! Every encoder here writes `varint paneId + tail`, matching the frontend
//! `decode.ts` byte-for-byte. The `paneId = 0` sentinel is used for messages
//! that do not target a specific pane (CTRL, LAYOUT_CHANGED, VIEWPORT, etc.).

use serde_json::json;

use crate::varint;

// ─── Outbound (server → client) — Event → inner payload ───────────────────

/// `0x02 PANE_OUT` inner = `varint paneId + raw bytes`.
pub fn encode_pane_out(pane_id: u32, bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(5 + bytes.len());
    varint::encode_into(u64::from(pane_id), &mut out);
    out.extend_from_slice(bytes);
    out
}

/// `0x07 NOTIFY_MIRROR` inner = `varint paneId + UTF-8 JSON`.
pub fn encode_notify_mirror(pane_id: u32, json_body: &str) -> Vec<u8> {
    let bytes = json_body.as_bytes();
    let mut out = Vec::with_capacity(5 + bytes.len());
    varint::encode_into(u64::from(pane_id), &mut out);
    out.extend_from_slice(bytes);
    out
}

/// `0x80 LAYOUT_CHANGED` inner = `varint 0 + 16-byte raw etag`.
pub fn encode_layout_changed(etag: &[u8; 16]) -> Vec<u8> {
    let mut out = Vec::with_capacity(1 + 16);
    varint::encode_into(0, &mut out);
    out.extend_from_slice(etag);
    out
}

/// `0x85 TERMINAL_DIED` (Stage 5-B) inner =
/// `varint 0 + UTF-8 JSON {"terminal_id":"<uuid>","reason":"exit"|"killed"}`.
///
/// The leading varint paneId is fixed at zero because this frame is
/// terminal-scoped (UUID), not pane-scoped — the SSoT-wide convention
/// for web-domain frames that have no PaneId.
pub fn encode_terminal_died(uuid: &str, reason: &str) -> Vec<u8> {
    let body = json!({
        "terminal_id": uuid,
        "reason": reason,
    })
    .to_string();
    let bytes = body.as_bytes();
    let mut out = Vec::with_capacity(1 + bytes.len());
    varint::encode_into(0, &mut out);
    out.extend_from_slice(bytes);
    out
}

/// `0x86 MOUNT_CASCADE` (Stage 5-D path P2) inner =
/// `varint 0 + UTF-8 JSON
/// {"trigger_session":"<name>","terminal_id":"<uuid>","x":<num>,"y":<num>,"w":<num>,"h":<num>}`.
///
/// Emitted by `POST /api/sessions/:name/terminals` to the *trigger
/// session* only (other sessions get `0x87 TERMINAL_LIST_UPDATE`). FE
/// `decodeMountCascade` (decode.ts) validates `w > 0`, `h > 0` and that
/// `trigger_session` matches the connection's currently-attached session
/// before appending the item — closes the **session-switch race**
/// (0072 BE follow-up §1, paired with FE `dispatcher.handleMountCascade`)
/// where BE's per-frame `session_for_owner` filter could send the frame
/// while the owner is *mid-switch*; the in-flight frame arriving after
/// the FE state flip would otherwise land the new terminal on the wrong
/// session.
///
/// Coordinates are server-determined defaults (cascade offset from the
/// session's current max x/y, fallback to `(80, 80, 720, 420)` for empty
/// layout — see http-api `sessions::next_mount_cascade_coords`).
pub fn encode_mount_cascade(
    trigger_session: &str,
    terminal_id: &str,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> Vec<u8> {
    let body = json!({
        "trigger_session": trigger_session,
        "terminal_id": terminal_id,
        "x": x,
        "y": y,
        "w": w,
        "h": h,
    })
    .to_string();
    let bytes = body.as_bytes();
    let mut out = Vec::with_capacity(1 + bytes.len());
    varint::encode_into(0, &mut out);
    out.extend_from_slice(bytes);
    out
}

/// `0x87 TERMINAL_LIST_UPDATE` (Stage 5-D path P1) inner =
/// `varint 0 + UTF-8 JSON {"added":["<uuid>",…],"removed":["<uuid>",…]}`.
///
/// `added`/`removed` are always emitted (possibly empty) so the FE decoder
/// (`decode.ts::decodeTerminalListUpdate`) can validate the shape with one
/// `parseStringArray` pass per field. Empty `removed` is the normal P1
/// case — attach_confirm emits spawn deltas only.
pub fn encode_terminal_list_update(added: &[String], removed: &[String]) -> Vec<u8> {
    let body = json!({
        "added": added,
        "removed": removed,
    })
    .to_string();
    let bytes = body.as_bytes();
    let mut out = Vec::with_capacity(1 + bytes.len());
    varint::encode_into(0, &mut out);
    out.extend_from_slice(bytes);
    out
}

/// Test-only helper — produces a minimal 0x83 VIEWPORT_CHANGED inner
/// payload (varint 0 + 12 zero bytes = x=0, y=0, zoom=0.0). Stage 5-C
/// tests use this as a recognizable "marker" body so they can assert the
/// manipulation routing carries the original bytes through unchanged
/// before the session_id trailer.
#[cfg(test)]
pub fn encode_viewport_marker_only() -> Vec<u8> {
    let mut out = Vec::with_capacity(1 + 12);
    varint::encode_into(0, &mut out);
    out.extend_from_slice(&[0u8; 12]);
    out
}

/// `0x88 TERMINAL_SPAWNED` (FE Issue C unblock) inner =
/// `varint 0 + UTF-8 JSON {"terminal_id":"<uuid>","pane_id":<u64>}`.
///
/// Emitted by `AppState::spawn_terminal_with_uuid` once the bridge map
/// register succeeds — i.e. the http-api layer holds an authoritative
/// UUID ↔ PaneId binding. Server-wide broadcast. FE consumes to update
/// its local map so an `XtermHost` instance can immediately switch to
/// "terminal" mode and start a per-pane subscription using the canonical
/// PaneId.
///
/// `pane_id` is encoded as a JSON number; `u64::MAX` fits within JavaScript
/// `Number.MAX_SAFE_INTEGER` (2⁵³ − 1) under any realistic deployment —
/// PaneId in this project counts from 1 and increments per spawn. The FE
/// decoder is responsible for the upper-bound check (`Number.isSafeInteger`).
pub fn encode_terminal_spawned(terminal_id: &str, pane_id: u64) -> Vec<u8> {
    let body = json!({
        "terminal_id": terminal_id,
        "pane_id": pane_id,
    })
    .to_string();
    let bytes = body.as_bytes();
    let mut out = Vec::with_capacity(1 + bytes.len());
    varint::encode_into(0, &mut out);
    out.extend_from_slice(bytes);
    out
}

/// `0x89 SERVER_SHUTDOWN` (Slice D-5, ADR-0014 D12) inner =
/// `varint 0 + UTF-8 JSON {"reason":"<kind>","expected_exit_code":<int>}`.
///
/// Emitted ~50 ms after `POST /api/shutdown` returns 202, just before
/// the WS connections are closed (CloseFrame 1000). FE consumers
/// switch their reconnect banner to the *intentional shutdown* branch
/// — no retry loop, a toast surfaces the reason.
pub fn encode_server_shutdown(reason: &str, expected_exit_code: i32) -> Vec<u8> {
    let body = json!({
        "reason": reason,
        "expected_exit_code": expected_exit_code,
    })
    .to_string();
    let bytes = body.as_bytes();
    let mut out = Vec::with_capacity(1 + bytes.len());
    varint::encode_into(0, &mut out);
    out.extend_from_slice(bytes);
    out
}

// ─── Inbound (client → server) — inner payload parsers ────────────────────

/// Parsed `0x03 PANE_IN` payload — varint paneId + raw input bytes.
#[derive(Debug, PartialEq, Eq)]
pub struct PaneInPayload<'a> {
    pub pane_id: u32,
    pub bytes: &'a [u8],
}

/// Decode a `0x03 PANE_IN` inner payload. Returns `None` if the varint is
/// malformed; the remaining bytes (which may be empty for a zero-byte burst)
/// become the input payload.
pub fn decode_pane_in(inner: &[u8]) -> Option<PaneInPayload<'_>> {
    let (pane_id, n) = varint::decode(inner)?;
    Some(PaneInPayload {
        pane_id: u32::try_from(pane_id).ok()?,
        bytes: &inner[n..],
    })
}

/// Parsed `0x04 PANE_RESIZE` payload — varint paneId + varint cols + varint rows.
#[derive(Debug, PartialEq, Eq)]
pub struct PaneResizePayload {
    pub pane_id: u32,
    pub cols: u32,
    pub rows: u32,
}

/// Decode a `0x04 PANE_RESIZE` inner payload.
pub fn decode_pane_resize(inner: &[u8]) -> Option<PaneResizePayload> {
    let (pane_id, n1) = varint::decode(inner)?;
    let (cols, n2) = varint::decode(&inner[n1..])?;
    let (rows, n3) = varint::decode(&inner[n1 + n2..])?;
    // Strict: any trailing bytes are a protocol violation.
    if n1 + n2 + n3 != inner.len() {
        return None;
    }
    Some(PaneResizePayload {
        pane_id: u32::try_from(pane_id).ok()?,
        cols: u32::try_from(cols).ok()?,
        rows: u32::try_from(rows).ok()?,
    })
}

/// Decode a `0x05/0x06 PANE_PAUSE/RESUME` inner payload — just a varint paneId.
/// Trailing bytes are a protocol violation.
pub fn decode_pane_bare_id(inner: &[u8]) -> Option<u32> {
    let (pane_id, n) = varint::decode(inner)?;
    if n != inner.len() {
        return None;
    }
    u32::try_from(pane_id).ok()
}

/// Parsed `0x01 CTRL` payload — varint 0 + UTF-8 JSON `{"id","cmd","args"}`.
#[derive(Debug)]
pub struct CtrlPayload {
    pub id: Option<String>,
    pub cmd: String,
    pub args: Vec<String>,
}

/// Decode a `0x01 CTRL` request inner payload. The leading varint MUST be 0
/// (SSoT §2.1: CTRL is pane-independent). Returns `None` on malformed JSON
/// or schema mismatch — the WS handler then sends an `ERR_BAD_REQUEST`
/// reply and keeps the connection alive (SSoT §3 commentary).
pub fn decode_ctrl_request(inner: &[u8]) -> Option<CtrlPayload> {
    let (head, n) = varint::decode(inner)?;
    if head != 0 {
        return None;
    }
    let json: serde_json::Value = serde_json::from_slice(&inner[n..]).ok()?;
    let obj = json.as_object()?;
    let cmd = obj.get("cmd")?.as_str()?.to_owned();
    let args_value = obj.get("args")?;
    let args = args_value
        .as_array()?
        .iter()
        .map(|v| v.as_str().map(str::to_owned))
        .collect::<Option<Vec<String>>>()?;
    let id = obj
        .get("id")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    Some(CtrlPayload { id, cmd, args })
}

/// Build a `0x01 CTRL` error response inner payload.
pub fn encode_ctrl_error(id: Option<&str>, code: &str, message: &str) -> Vec<u8> {
    let json = json!({
        "id": id.unwrap_or(""),
        "ok": false,
        "code": code,
        "error": message,
    });
    let body = json.to_string();
    let mut out = Vec::with_capacity(1 + body.len());
    varint::encode_into(0, &mut out);
    out.extend_from_slice(body.as_bytes());
    out
}

/// Build a `0x01 CTRL` success response inner payload. Used by
/// `kill-session` (ADR-0013 D10 amend) — the WS handler acks before
/// raising SIGTERM so the client sees the success frame before the
/// connection drops.
pub fn encode_ctrl_success(id: Option<&str>) -> Vec<u8> {
    let json = json!({
        "id": id.unwrap_or(""),
        "ok": true,
    });
    let body = json.to_string();
    let mut out = Vec::with_capacity(1 + body.len());
    varint::encode_into(0, &mut out);
    out.extend_from_slice(body.as_bytes());
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn pane_out_round_trip() {
        let inner = encode_pane_out(37, b"hello");
        // varint(37) = 0x25, then "hello".
        assert_eq!(inner, vec![0x25, b'h', b'e', b'l', b'l', b'o']);
    }

    #[test]
    fn pane_out_pane_id_zero() {
        let inner = encode_pane_out(0, b"");
        assert_eq!(inner, vec![0x00]);
    }

    #[test]
    fn notify_mirror_round_trip() {
        let inner = encode_notify_mirror(0, "{\"kind\":\"slow-pane\"}");
        // Leading 0x00 + JSON body.
        assert_eq!(inner[0], 0x00);
        assert_eq!(&inner[1..], b"{\"kind\":\"slow-pane\"}");
    }

    #[test]
    fn terminal_died_carries_uuid_and_reason() {
        let inner = encode_terminal_died("11111111-2222-4333-8444-555555555555", "exit");
        // Leading pane-id varint = 0.
        assert_eq!(inner[0], 0x00);
        let json: serde_json::Value = serde_json::from_slice(&inner[1..]).unwrap();
        assert_eq!(json["terminal_id"], "11111111-2222-4333-8444-555555555555");
        assert_eq!(json["reason"], "exit");
    }

    #[test]
    fn terminal_died_supports_killed_reason() {
        let inner = encode_terminal_died("uuid", "killed");
        let json: serde_json::Value = serde_json::from_slice(&inner[1..]).unwrap();
        assert_eq!(json["reason"], "killed");
    }

    #[test]
    fn terminal_list_update_round_trip() {
        let added = vec!["u1".to_string(), "u2".to_string()];
        let removed: Vec<String> = vec![];
        let inner = encode_terminal_list_update(&added, &removed);
        assert_eq!(inner[0], 0x00);
        let json: serde_json::Value = serde_json::from_slice(&inner[1..]).unwrap();
        assert_eq!(json["added"], serde_json::json!(["u1", "u2"]));
        assert_eq!(json["removed"], serde_json::json!([]));
    }

    #[test]
    fn mount_cascade_round_trip() {
        let inner = encode_mount_cascade(
            "alpha",
            "11111111-2222-4333-8444-555555555555",
            80.0,
            96.0,
            720.0,
            420.0,
        );
        assert_eq!(inner[0], 0x00);
        let json: serde_json::Value = serde_json::from_slice(&inner[1..]).unwrap();
        assert_eq!(json["trigger_session"], "alpha");
        assert_eq!(json["terminal_id"], "11111111-2222-4333-8444-555555555555");
        assert_eq!(json["x"], 80.0);
        assert_eq!(json["y"], 96.0);
        assert_eq!(json["w"], 720.0);
        assert_eq!(json["h"], 420.0);
    }

    #[test]
    fn terminal_spawned_round_trip() {
        let inner = encode_terminal_spawned("11111111-2222-4333-8444-555555555555", 42);
        assert_eq!(inner[0], 0x00);
        let json: serde_json::Value = serde_json::from_slice(&inner[1..]).unwrap();
        assert_eq!(json["terminal_id"], "11111111-2222-4333-8444-555555555555");
        assert_eq!(json["pane_id"], 42);
    }

    #[test]
    fn terminal_spawned_pane_id_fits_in_js_safe_integer() {
        // FE decodes pane_id as a JSON number; values above 2^53 - 1 lose
        // precision in JS. PaneId starts at 1 and increments, so this is a
        // generous upper-bound check rather than a tight one — just guard
        // against accidental u64::MAX leaks in future encoders.
        let inner = encode_terminal_spawned("uuid", (1u64 << 50) + 7);
        let json: serde_json::Value = serde_json::from_slice(&inner[1..]).unwrap();
        let pane = json["pane_id"].as_u64().unwrap();
        assert!(pane < (1u64 << 53));
    }

    #[test]
    fn terminal_list_update_emits_empty_arrays_explicitly() {
        // FE decoder expects both fields present; serde_json::json! macro
        // already emits the empty-array literal — guard the contract.
        let inner = encode_terminal_list_update(&[], &[]);
        let json: serde_json::Value = serde_json::from_slice(&inner[1..]).unwrap();
        assert!(json["added"].is_array());
        assert!(json["removed"].is_array());
    }

    #[test]
    fn layout_changed_etag_inline() {
        let etag: [u8; 16] = [
            0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF, 0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77,
            0x88, 0x99,
        ];
        let inner = encode_layout_changed(&etag);
        assert_eq!(inner.len(), 17);
        assert_eq!(inner[0], 0x00); // paneId varint = 0
        assert_eq!(&inner[1..], &etag);
    }

    #[test]
    fn pane_in_decode_extracts_bytes() {
        // varint(5) + "abc"
        let bytes = [0x05u8, b'a', b'b', b'c'];
        let p = decode_pane_in(&bytes).unwrap();
        assert_eq!(p.pane_id, 5);
        assert_eq!(p.bytes, b"abc");
    }

    #[test]
    fn pane_in_zero_byte_burst_legal() {
        let bytes = [0x05u8];
        let p = decode_pane_in(&bytes).unwrap();
        assert_eq!(p.pane_id, 5);
        assert_eq!(p.bytes, b"");
    }

    #[test]
    fn pane_resize_decode_strict_on_trailing_bytes() {
        // varint(5) + varint(80) + varint(24) + extra byte.
        let bytes = [0x05u8, 0x50, 0x18, 0x00];
        assert!(decode_pane_resize(&bytes).is_none());
        // Without the extra byte → ok.
        let p = decode_pane_resize(&bytes[..3]).unwrap();
        assert_eq!(
            p,
            PaneResizePayload {
                pane_id: 5,
                cols: 80,
                rows: 24,
            }
        );
    }

    #[test]
    fn pane_bare_id_strict_on_trailing_bytes() {
        assert_eq!(decode_pane_bare_id(&[0x05]), Some(5));
        // Extra trailing byte → reject.
        assert_eq!(decode_pane_bare_id(&[0x05, 0x00]), None);
    }

    #[test]
    fn ctrl_decode_basic() {
        let body = br#"{"id":"abc-123","cmd":"new-window","args":["-t","sess"]}"#;
        let mut inner = vec![0u8];
        inner.extend_from_slice(body);
        let c = decode_ctrl_request(&inner).unwrap();
        assert_eq!(c.id.as_deref(), Some("abc-123"));
        assert_eq!(c.cmd, "new-window");
        assert_eq!(c.args, vec!["-t".to_string(), "sess".to_string()]);
    }

    #[test]
    fn ctrl_rejects_non_zero_pane_varint() {
        // varint(5) + valid JSON — pane id must be 0 for CTRL.
        let mut inner = vec![0x05u8];
        inner.extend_from_slice(br#"{"cmd":"new-window","args":[]}"#);
        assert!(decode_ctrl_request(&inner).is_none());
    }

    #[test]
    fn ctrl_rejects_non_string_arg() {
        // args contains a number → must be string-only.
        let mut inner = vec![0u8];
        inner.extend_from_slice(br#"{"cmd":"new-window","args":[1,2]}"#);
        assert!(decode_ctrl_request(&inner).is_none());
    }

    #[test]
    fn ctrl_error_envelope_round_trip() {
        let inner = encode_ctrl_error(Some("xyz"), "ERR_NOT_ALLOWED", "denied");
        assert_eq!(inner[0], 0x00);
        let json: serde_json::Value = serde_json::from_slice(&inner[1..]).unwrap();
        assert_eq!(json["id"], "xyz");
        assert_eq!(json["ok"], false);
        assert_eq!(json["code"], "ERR_NOT_ALLOWED");
    }
}
