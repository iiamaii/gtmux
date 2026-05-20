// WS binary envelope codec — encode/decode + per-frame payload helpers.
//
// 정본:
// - `docs/ssot/wire-protocol.md` §1.2 (envelope), §2 (32 슬롯), §3 (디코더 의사코드)
// - `docs/adr/0002-transport-websocket.md` D2/D3/D4 (envelope 구조 + payload 명세)
// - `docs/reports/0008-frontend-stack.md` §F4 (frontend dispatcher 골격)
// - `codebase/backend/crates/ws-server/src/lib.rs` (backend 정합 — outer frame =
//   `[1B type][4B LE u32 length][payload]`, paneId varint 은 payload 내부에 거주)
//
// **outer 와 inner 의 구분**:
//
// SSoT §1.2 는 envelope 을 "1B type + varint paneId + payload" 로 *논리적으로* 기술하지만
// backend ws-server 의 codec (lib.rs 의 `Envelope::encode/decode`) 은 *outer wire* 를
// `[1B type][4B LE u32 length][payload bytes]` 로 framing 하고 paneId varint 은 그
// `payload bytes` 안에 둔다 (lib.rs docstring lines 7-9). 두 정의는 호환된다 —
// length prefix 가 *codec* 차원의 framing 이고, paneId varint 는 *router* 차원의
// payload 해석. 본 모듈은 backend 와 byte-equal 호환되도록 outer = type+LE32+bytes 를
// 채택하고, paneId varint 를 포함한 per-frame payload 파싱 helper 를 별도로 제공한다.

// ── 상수 ────────────────────────────────────────────────────────────────────

/** Envelope header: 1B type + 4B LE u32 length. (backend `HEADER_LEN`). */
const HEADER_LEN = 5;

/**
 * Payload 의 hard ceiling. backend `MAX_PAYLOAD` (ws-server/src/lib.rs) 와 동일.
 * SSoT §1.2 의 1 MiB soft cap 위로 4× 방어 헤드룸. 초과 길이 prefix 는 디코더가
 * 페이로드 바이트를 *읽기 전에* null 을 돌려 OOM 표면을 막는다.
 */
export const MAX_PAYLOAD = 4 * 1024 * 1024;

/**
 * Frame type discriminants — SSoT §2 의 32 슬롯 중 정의된 15 개.
 * 예약 슬롯 (0x08–0x0F, 0x88–0x8F) 은 *디코드 단계에서 null* 로 떨어뜨려 forward-compat
 * 을 유지한다 (SSoT §6 호환성 정책).
 *
 * 0x86/0x87 는 BE Stage 5-D 의 trigger-aware auto-mount 용 — FE 측 사전 wire 정의
 * (FE-NEW-6 co-decision). 0034 §8.3 의 권장 ID + payload 그대로 채택.
 */
export const FRAME_TYPE = {
  // tmux-domain (0x01–0x07)
  CTRL: 0x01,
  PANE_OUT: 0x02,
  PANE_IN: 0x03,
  PANE_RESIZE: 0x04,
  PANE_PAUSE: 0x05,
  PANE_RESUME: 0x06,
  NOTIFY_MIRROR: 0x07,
  // web-domain (0x80–0x87)
  LAYOUT_CHANGED: 0x80,
  M_CHANGED: 0x81,
  I_CHANGED: 0x82,
  VIEWPORT_CHANGED: 0x83,
  FOCUS_MODE_CHANGED: 0x84,
  TERMINAL_DIED: 0x85,
  MOUNT_CASCADE: 0x86,
  TERMINAL_LIST_UPDATE: 0x87,
  TERMINAL_SPAWNED: 0x88,
} as const;

/** 정의된 frame type 코드의 union. */
export type FrameTypeCode = (typeof FRAME_TYPE)[keyof typeof FRAME_TYPE];

/** 모든 정의된 코드 (런타임 set — `isKnownFrameType` 용). */
const KNOWN_CODES: ReadonlySet<number> = new Set<number>(Object.values(FRAME_TYPE));

function isKnownFrameType(b: number): b is FrameTypeCode {
  return KNOWN_CODES.has(b);
}

// ── Envelope 타입 ──────────────────────────────────────────────────────────

/**
 * 디코드된 envelope. `payload` 는 *수신 ArrayBuffer 의 zero-copy view* —
 * dispatcher 가 다음 frame 을 디코드하기 전에 처리해야 한다 (호출 측 책임).
 */
export interface Envelope {
  readonly kind: FrameTypeCode;
  readonly payload: Uint8Array;
}

// ── Per-frame payload 타입 ─────────────────────────────────────────────────

export interface PaneOutPayload {
  readonly paneId: number;
  readonly bytes: Uint8Array;
}

export interface LayoutChangedPayload {
  /** 16-byte raw ETag (SSoT §2.2). hex 문자열 변환은 호출 측 책임. */
  readonly etag: Uint8Array;
}

export interface MChangedPayload {
  readonly panelIds: readonly number[];
}

export interface IChangedPayload {
  /** SSoT §2.2: `0` 은 I 미설정 sentinel — 본 타입은 그 자리에 `null` 을 둔다. */
  readonly paneId: number | null;
}

export interface ViewportChangedPayload {
  readonly x: number;
  readonly y: number;
  readonly zoom: number;
}

export interface FocusModeChangedPayload {
  readonly enabled: boolean;
  /** `enabled === false` 일 때 의미 없음 (SSoT §2.2). */
  readonly targetPanelId: number | null;
}

export interface NotifyMirrorPayload {
  readonly paneId: number;
  /** SSoT §2.3 의 `{ kind, ... }` JSON. 알지 못하는 `kind` 는 호출 측에서 무시. */
  readonly body: Readonly<Record<string, unknown>>;
}

/** 0x85 TERMINAL_DIED — BE Stage 5-B (0034 §3). UUID-carrying. */
export type TerminalDiedReason = 'exit' | 'killed';
export interface TerminalDiedPayload {
  readonly terminalId: string;
  readonly reason: TerminalDiedReason;
}

/**
 * 0x86 MOUNT_CASCADE — BE Stage 5-D (0034 §8.3 recommended).
 *
 * BE 가 *trigger session 의 webpage* 에만 fan-out (5-A session_table 라우팅).
 * payload: server-determined coordinates for a freshly-spawned terminal.
 * FE 는 sessionStore.active 의 layout 에 TerminalItem append (idempotent).
 */
export interface MountCascadePayload {
  /**
   * Session name the BE intended this cascade for. The owner's attached
   * session at frame-send time. The FE handler MUST compare against the
   * connection's currently-attached session before appending the item —
   * a session-switch race window (0072 BE follow-up §1) can deliver this
   * frame after the owner already flipped to a different session, in
   * which case the BE per-frame filter is too late and the FE must drop
   * the cascade.
   */
  readonly triggerSession: string;
  readonly terminalId: string;
  readonly x: number;
  readonly y: number;
  readonly w: number;
  readonly h: number;
}

/**
 * 0x87 TERMINAL_LIST_UPDATE — BE Stage 5-D (0034 §8.3 recommended).
 *
 * BE 가 *non-trigger session 의 webpage* 에 fan-out — pool 변동의 hint delta.
 * Authoritative source 는 여전히 `GET /api/terminals`; 본 frame 은 5-s 폴링
 * latency 단축용. FE 는 terminalPool.refresh() 만 호출.
 */
export interface TerminalListUpdatePayload {
  readonly added: readonly string[];
  readonly removed: readonly string[];
}

/**
 * 0x88 TERMINAL_SPAWNED — BE Stage 5 batch `d00db66` (0039 §1.2).
 *
 * spawn_terminal_with_uuid 의 register 직후 server-wide broadcast.
 * FE 의 terminalPool 가 UUID → numeric PaneId 매핑 갱신 → XtermHost 의 terminal
 * 모드가 PANE_OUT subscriber 등록 + PANE_IN/RESIZE 송신 가능.
 */
export interface TerminalSpawnedPayload {
  readonly terminalId: string;
  readonly paneId: number;
}

// ── Varint (unsigned LEB128) ───────────────────────────────────────────────

/**
 * Read one unsigned LEB128 varint from `view` starting at `offset`.
 * Returns the value and the next offset, or `null` on overflow / OOB.
 *
 * SSoT §1.3: 5 바이트 초과는 거부 — length-of-length 공격 방어. JS number 는 2^53
 * 까지 안전하므로 5 바이트(= 최대 35 비트) 안에서는 손실 없음.
 */
function readVarintU(
  view: DataView,
  offset: number,
): { value: number; next: number } | null {
  let result = 0;
  let shift = 0;
  let cursor = offset;
  // 최대 5 바이트.
  for (let i = 0; i < 5; i += 1) {
    if (cursor >= view.byteLength) return null;
    const byte = view.getUint8(cursor);
    cursor += 1;
    // `>>> 0` 으로 강제 unsigned — 32-bit shift 의 sign-extension 회피.
    result = (result + ((byte & 0x7f) * 2 ** shift)) >>> 0;
    if ((byte & 0x80) === 0) {
      return { value: result, next: cursor };
    }
    shift += 7;
  }
  // 5 바이트 모두 continuation bit set → 형식 위반.
  return null;
}

/** Encode an unsigned int as LEB128. JS number → at most 5 bytes (≥ 32-bit). */
function writeVarintU(value: number): Uint8Array {
  if (!Number.isInteger(value) || value < 0) {
    throw new RangeError(`writeVarintU: not a non-negative int: ${value}`);
  }
  const bytes: number[] = [];
  let v = value;
  // value 가 0 이면 single byte 0.
  do {
    let b = v & 0x7f;
    v = Math.floor(v / 128);
    if (v > 0) b |= 0x80;
    bytes.push(b);
  } while (v > 0);
  return Uint8Array.from(bytes);
}

// ── Envelope outer codec ────────────────────────────────────────────────────

/**
 * Decode one envelope from `buf`. Returns `null` for any malformed shape:
 * - shorter than 5-byte header
 * - declared length exceeds `MAX_PAYLOAD`
 * - actual buffer shorter than declared total
 * - type byte not in the defined slots (reserved or unknown — forward-compat)
 *
 * The returned `payload` is a *view* into the input buffer (zero-copy).
 */
export function decodeEnvelope(buf: ArrayBuffer): Envelope | null {
  if (buf.byteLength < HEADER_LEN) return null;
  const view = new DataView(buf);
  const typeByte = view.getUint8(0);
  if (!isKnownFrameType(typeByte)) return null;
  const length = view.getUint32(1, true);
  if (length > MAX_PAYLOAD) return null;
  const total = HEADER_LEN + length;
  if (buf.byteLength < total) return null;
  const payload = new Uint8Array(buf, HEADER_LEN, length);
  return { kind: typeByte, payload };
}

/**
 * Encode an envelope to a self-contained `Uint8Array`. Throws `RangeError` if
 * `payload.length > MAX_PAYLOAD` — symmetric to `decodeEnvelope` so callers can
 * round-trip without dropping their own frames.
 */
export function encodeEnvelope(kind: FrameTypeCode, payload: Uint8Array): Uint8Array {
  if (payload.length > MAX_PAYLOAD) {
    throw new RangeError(`encodeEnvelope: payload ${payload.length}B > ${MAX_PAYLOAD}B max`);
  }
  const out = new Uint8Array(HEADER_LEN + payload.length);
  const view = new DataView(out.buffer);
  view.setUint8(0, kind);
  view.setUint32(1, payload.length, /* littleEndian */ true);
  out.set(payload, HEADER_LEN);
  return out;
}

// ── Per-frame payload helpers ──────────────────────────────────────────────
//
// 각 helper 는 *envelope payload* (decodeEnvelope 의 `.payload`) 를 받아 frame 별
// 의미적 필드로 분해한다. 형식 위반 시 `null` 반환 — 호출 측은 그 frame 만 drop.

/**
 * `0x02 PANE_OUT` payload = `varint paneId + raw bytes`. SSoT §2.1.
 *
 * `raw bytes` 길이는 *outer length* 에서 varint 의 길이를 뺀 값으로 자연 결정 —
 * 별도 length 필드를 두지 않는다 (outer codec 이 이미 framing 을 끝낸 상태).
 */
export function decodePaneOut(payload: Uint8Array): PaneOutPayload | null {
  const view = makeView(payload);
  const v = readVarintU(view, 0);
  if (!v) return null;
  // bytes 는 view-into-view — payload buffer 의 sub-slice.
  const bytes = payload.subarray(v.next);
  return { paneId: v.value, bytes };
}

/**
 * `0x03 PANE_IN` payload = `varint paneId + raw bytes`. SSoT §2.1.
 * 클라이언트 → 서버 방향이므로 인코딩만 일반적으로 쓰이나, 대칭을 위해 디코더도
 * 제공 (테스트 / loopback echo 처리).
 */
export function decodePaneIn(payload: Uint8Array): PaneOutPayload | null {
  return decodePaneOut(payload); // 같은 형식
}

export function encodePaneIn(paneId: number, bytes: Uint8Array): Uint8Array {
  const prefix = writeVarintU(paneId);
  const out = new Uint8Array(prefix.length + bytes.length);
  out.set(prefix, 0);
  out.set(bytes, prefix.length);
  return out;
}

/** `0x04 PANE_RESIZE` payload = `varint paneId + varint cols + varint rows`. */
export interface PaneResizePayload {
  readonly paneId: number;
  readonly cols: number;
  readonly rows: number;
}

export function decodePaneResize(payload: Uint8Array): PaneResizePayload | null {
  const view = makeView(payload);
  const a = readVarintU(view, 0);
  if (!a) return null;
  const b = readVarintU(view, a.next);
  if (!b) return null;
  const c = readVarintU(view, b.next);
  if (!c) return null;
  return { paneId: a.value, cols: b.value, rows: c.value };
}

export function encodePaneResize(paneId: number, cols: number, rows: number): Uint8Array {
  return concat3(writeVarintU(paneId), writeVarintU(cols), writeVarintU(rows));
}

/** `0x05/0x06 PANE_PAUSE/RESUME` payload = `varint paneId` only. */
export function decodePaneBareId(payload: Uint8Array): number | null {
  const view = makeView(payload);
  const v = readVarintU(view, 0);
  if (!v) return null;
  if (v.next !== payload.length) return null;
  return v.value;
}

export function encodePaneBareId(paneId: number): Uint8Array {
  return writeVarintU(paneId);
}

/**
 * `0x01 CTRL` payload = `varint 0 + UTF-8 JSON`. SSoT §2.1 + §2.4.
 *
 * 요청 인코딩: `{id, cmd, args}` — 문자열 배열만 허용 (shell 문자열·자유 형식 금지).
 * 본 helper 는 *요청 송신용* 인코딩만 담당하며, 응답(JSON `{id, ok, ...}`) 파싱은
 * `decodeCtrl` 가 담당한다.
 */
export function encodeCtrl(id: string, cmd: string, args: readonly string[]): Uint8Array {
  const json = JSON.stringify({ id, cmd, args });
  const head = writeVarintU(0);
  const body = new TextEncoder().encode(json);
  const out = new Uint8Array(head.length + body.length);
  out.set(head, 0);
  out.set(body, head.length);
  return out;
}

/** `0x01 CTRL` payload decode (response 측). 형식 위반 시 null. */
export interface CtrlDecoded {
  readonly id: string | null;
  readonly body: Readonly<Record<string, unknown>>;
}
export function decodeCtrl(payload: Uint8Array): CtrlDecoded | null {
  const view = makeView(payload);
  const head = readVarintU(view, 0);
  if (!head || head.value !== 0) return null;
  const jsonBytes = payload.subarray(head.next);
  let body: unknown;
  try {
    body = JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(jsonBytes));
  } catch {
    return null;
  }
  if (body === null || typeof body !== 'object' || Array.isArray(body)) return null;
  const obj = body as Record<string, unknown>;
  const id = typeof obj['id'] === 'string' ? (obj['id'] as string) : null;
  return { id, body: obj as Readonly<Record<string, unknown>> };
}

/**
 * `0x07 NOTIFY_MIRROR` payload = `varint paneId + UTF-8 JSON`. SSoT §2.1 + §2.3.
 * JSON 파싱 실패 / non-object 는 null 반환 (호출 측은 그 frame drop).
 */
export function decodeNotifyMirror(payload: Uint8Array): NotifyMirrorPayload | null {
  const view = makeView(payload);
  const head = readVarintU(view, 0);
  if (!head) return null;
  const jsonBytes = payload.subarray(head.next);
  let body: unknown;
  try {
    body = JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(jsonBytes));
  } catch {
    return null;
  }
  if (body === null || typeof body !== 'object' || Array.isArray(body)) return null;
  return { paneId: head.value, body: body as Readonly<Record<string, unknown>> };
}

/**
 * `0x80 LAYOUT_CHANGED` payload = `varint 0 + 16B raw etag`. SSoT §2.2.
 * paneId 가 0 이 아니면 형식 위반.
 */
export function decodeLayoutChanged(payload: Uint8Array): LayoutChangedPayload | null {
  const view = makeView(payload);
  const head = readVarintU(view, 0);
  if (!head || head.value !== 0) return null;
  const rest = payload.subarray(head.next);
  if (rest.length !== 16) return null;
  // copy 해서 caller 가 view buffer 의 다음 frame 에 의존하지 않게 한다 — etag 는
  // store 에 들어가 dispatcher tick 을 벗어나 살아남는다.
  return { etag: new Uint8Array(rest) };
}

/**
 * `0x81 M_CHANGED` payload = `varint 0 + varint count + varint panel_ids[]`.
 * SSoT §2.2.
 */
export function decodeMChanged(payload: Uint8Array): MChangedPayload | null {
  const view = makeView(payload);
  const head = readVarintU(view, 0);
  if (!head || head.value !== 0) return null;
  const cnt = readVarintU(view, head.next);
  if (!cnt) return null;
  const ids: number[] = [];
  let cursor = cnt.next;
  for (let i = 0; i < cnt.value; i += 1) {
    const v = readVarintU(view, cursor);
    if (!v) return null;
    ids.push(v.value);
    cursor = v.next;
  }
  if (cursor !== payload.length) return null;
  return { panelIds: ids };
}

export function encodeMChanged(panelIds: readonly number[]): Uint8Array {
  const parts: Uint8Array[] = [writeVarintU(0), writeVarintU(panelIds.length)];
  for (const id of panelIds) parts.push(writeVarintU(id));
  return concatMany(parts);
}

/** `0x82 I_CHANGED` payload = `varint 0 + varint pane_id (0 = null)`. SSoT §2.2. */
export function decodeIChanged(payload: Uint8Array): IChangedPayload | null {
  const view = makeView(payload);
  const head = readVarintU(view, 0);
  if (!head || head.value !== 0) return null;
  const v = readVarintU(view, head.next);
  if (!v) return null;
  if (v.next !== payload.length) return null;
  return { paneId: v.value === 0 ? null : v.value };
}

export function encodeIChanged(paneId: number | null): Uint8Array {
  return concatMany([writeVarintU(0), writeVarintU(paneId ?? 0)]);
}

/**
 * `0x83 VIEWPORT_CHANGED` payload = `varint 0 + int32 LE x + int32 LE y + float32 LE zoom`.
 * SSoT §2.2 (12 byte fixed body). R8 F4 명시: `DataView.getInt32(offset, true)`.
 */
export function decodeViewport(payload: Uint8Array): ViewportChangedPayload | null {
  const view = makeView(payload);
  const head = readVarintU(view, 0);
  if (!head || head.value !== 0) return null;
  const off = head.next;
  if (payload.length - off !== 12) return null;
  return {
    x: view.getInt32(off, true),
    y: view.getInt32(off + 4, true),
    zoom: view.getFloat32(off + 8, true),
  };
}

export function encodeViewport(x: number, y: number, zoom: number): Uint8Array {
  if (!Number.isInteger(x) || !Number.isInteger(y)) {
    throw new RangeError('encodeViewport: x/y must be integers');
  }
  const head = writeVarintU(0);
  const body = new Uint8Array(12);
  const view = new DataView(body.buffer);
  view.setInt32(0, x, true);
  view.setInt32(4, y, true);
  view.setFloat32(8, zoom, true);
  const out = new Uint8Array(head.length + 12);
  out.set(head, 0);
  out.set(body, head.length);
  return out;
}

/**
 * `0x84 FOCUS_MODE_CHANGED` payload = `varint 0 + 1B enabled + varint target_panel_id`.
 * SSoT §2.2.
 */
export function decodeFocusMode(payload: Uint8Array): FocusModeChangedPayload | null {
  const view = makeView(payload);
  const head = readVarintU(view, 0);
  if (!head || head.value !== 0) return null;
  if (head.next >= payload.length) return null;
  const enabled = view.getUint8(head.next) !== 0;
  const v = readVarintU(view, head.next + 1);
  if (!v) return null;
  if (v.next !== payload.length) return null;
  return {
    enabled,
    // 관습: enabled=false 면 target 은 의미 없음 — null 로 정규화.
    // enabled=true 라도 0 sentinel 은 null (I_CHANGED 와 동일 컨벤션).
    targetPanelId: !enabled || v.value === 0 ? null : v.value,
  };
}

export function encodeFocusMode(enabled: boolean, targetPanelId: number | null): Uint8Array {
  const head = writeVarintU(0);
  const target = writeVarintU(targetPanelId ?? 0);
  const out = new Uint8Array(head.length + 1 + target.length);
  out.set(head, 0);
  out[head.length] = enabled ? 1 : 0;
  out.set(target, head.length + 1);
  return out;
}

/**
 * `0x85 TERMINAL_DIED` payload = `varint 0 + UTF-8 JSON {terminal_id, reason}`.
 * BE 정본: `crates/ws-server/src/payload.rs::encode_terminal_died` (0034 §3.2).
 *
 * server-wide broadcast — mirror 정합 (같은 UUID 가 여러 session 의 panel 일 수
 * 있으므로 session_id 라우팅 X).
 */
export function decodeTerminalDied(payload: Uint8Array): TerminalDiedPayload | null {
  const obj = decodeVarintZeroJsonObject(payload);
  if (!obj) return null;
  const terminalId = obj['terminal_id'];
  const reason = obj['reason'];
  if (typeof terminalId !== 'string' || terminalId.length === 0) return null;
  if (reason !== 'exit' && reason !== 'killed') return null;
  return { terminalId, reason };
}

/**
 * `0x86 MOUNT_CASCADE` payload = `varint 0 + UTF-8 JSON {terminal_id, x, y, w, h}`.
 * BE Stage 5-D wire (0034 §8.3 권장).
 */
export function decodeMountCascade(payload: Uint8Array): MountCascadePayload | null {
  const obj = decodeVarintZeroJsonObject(payload);
  if (!obj) return null;
  const triggerSession = obj['trigger_session'];
  const terminalId = obj['terminal_id'];
  const x = obj['x'];
  const y = obj['y'];
  const w = obj['w'];
  const h = obj['h'];
  if (typeof triggerSession !== 'string' || triggerSession.length === 0) return null;
  if (typeof terminalId !== 'string' || terminalId.length === 0) return null;
  if (typeof x !== 'number' || typeof y !== 'number') return null;
  if (typeof w !== 'number' || typeof h !== 'number') return null;
  if (!Number.isFinite(x) || !Number.isFinite(y) || !Number.isFinite(w) || !Number.isFinite(h)) {
    return null;
  }
  if (w <= 0 || h <= 0) return null;
  return { triggerSession, terminalId, x, y, w, h };
}

/**
 * `0x87 TERMINAL_LIST_UPDATE` payload = `varint 0 + UTF-8 JSON {added: [..], removed: [..]}`.
 * BE Stage 5-D wire (0034 §8.3 권장).
 */
export function decodeTerminalListUpdate(payload: Uint8Array): TerminalListUpdatePayload | null {
  const obj = decodeVarintZeroJsonObject(payload);
  if (!obj) return null;
  const added = parseStringArray(obj['added']);
  const removed = parseStringArray(obj['removed']);
  if (added === null || removed === null) return null;
  return { added, removed };
}

/**
 * `0x88 TERMINAL_SPAWNED` payload = `varint 0 + UTF-8 JSON {terminal_id, pane_id}`.
 * BE 정본: 0039 §1.2. PaneId 는 JS Number — 2⁵³-1 미만 보장하지만 long-running
 * server overflow 대비 isSafeInteger guard.
 */
export function decodeTerminalSpawned(payload: Uint8Array): TerminalSpawnedPayload | null {
  const obj = decodeVarintZeroJsonObject(payload);
  if (!obj) return null;
  const terminalId = obj['terminal_id'];
  const paneId = obj['pane_id'];
  if (typeof terminalId !== 'string' || terminalId.length === 0) return null;
  if (typeof paneId !== 'number' || !Number.isSafeInteger(paneId) || paneId <= 0) return null;
  return { terminalId, paneId };
}

function parseStringArray(v: unknown): string[] | null {
  if (!Array.isArray(v)) return null;
  const out: string[] = [];
  for (const x of v) {
    if (typeof x !== 'string' || x.length === 0) return null;
    out.push(x);
  }
  return out;
}

/**
 * Shared helper — `varint 0 + UTF-8 JSON object` 형식의 frame body 디코딩.
 * 0x85/0x86/0x87 등 web-domain JSON-bodied frame 의 공통 prefix 처리.
 */
function decodeVarintZeroJsonObject(payload: Uint8Array): Record<string, unknown> | null {
  const view = makeView(payload);
  const head = readVarintU(view, 0);
  if (!head || head.value !== 0) return null;
  const jsonBytes = payload.subarray(head.next);
  let body: unknown;
  try {
    body = JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(jsonBytes));
  } catch {
    return null;
  }
  if (body === null || typeof body !== 'object' || Array.isArray(body)) return null;
  return body as Record<string, unknown>;
}

// ── 내부 helper ────────────────────────────────────────────────────────────

function makeView(payload: Uint8Array): DataView {
  // payload 는 ArrayBuffer 의 임의 subarray 일 수 있다 — byteOffset/byteLength 를
  // 명시해야 view 가 올바른 영역만 본다.
  return new DataView(payload.buffer, payload.byteOffset, payload.byteLength);
}

function concat3(a: Uint8Array, b: Uint8Array, c: Uint8Array): Uint8Array {
  const out = new Uint8Array(a.length + b.length + c.length);
  out.set(a, 0);
  out.set(b, a.length);
  out.set(c, a.length + b.length);
  return out;
}

function concatMany(parts: readonly Uint8Array[]): Uint8Array {
  let total = 0;
  for (const p of parts) total += p.length;
  const out = new Uint8Array(total);
  let cursor = 0;
  for (const p of parts) {
    out.set(p, cursor);
    cursor += p.length;
  }
  return out;
}
