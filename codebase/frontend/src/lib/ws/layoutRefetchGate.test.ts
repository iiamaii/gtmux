import { describe, it, expect } from 'vitest';
import { LayoutRefetchGate, etagBytesToHex } from '$lib/ws/layoutRefetchGate';

// ADR-0053 D7 — 0x80 LAYOUT_CHANGED 재활성의 순수 결정 로직 커버리지:
// etag hex 변환 (BE sha256_128 lowercase 정합), echo no-op, defer 가드
// (hold/release 짝, 중첩 hold), flush 시점 etag 재비교 (echo-race 흡수).

describe('etagBytesToHex', () => {
  it('converts 16 bytes to 32-char lowercase hex with zero padding', () => {
    const bytes = new Uint8Array([
      0x00, 0x01, 0x0a, 0x0f, 0x10, 0x7f, 0x80, 0xab,
      0xcd, 0xef, 0xff, 0x00, 0x42, 0x99, 0xde, 0x05,
    ]);
    expect(etagBytesToHex(bytes)).toBe('00010a0f107f80abcdefff004299de05');
  });

  it('is empty for empty input', () => {
    expect(etagBytesToHex(new Uint8Array(0))).toBe('');
  });

  it('round-trips against a BE-style hex string (lowercase, 2 digits/byte)', () => {
    const bytes = new Uint8Array(16).fill(0xff);
    expect(etagBytesToHex(bytes)).toBe('f'.repeat(32));
  });
});

describe('LayoutRefetchGate', () => {
  it('receive: matching etag → no-op (self-originated echo)', () => {
    const gate = new LayoutRefetchGate();
    expect(gate.receive('aa'.repeat(16), 'aa'.repeat(16))).toBe(false);
    expect(gate.pendingEtagHex).toBeNull();
  });

  it('receive: mismatching etag → schedule + pending set', () => {
    const gate = new LayoutRefetchGate();
    expect(gate.receive('bb'.repeat(16), 'aa'.repeat(16))).toBe(true);
    expect(gate.pendingEtagHex).toBe('bb'.repeat(16));
  });

  it('receive: unknown local etag (null) → treated as mismatch', () => {
    const gate = new LayoutRefetchGate();
    expect(gate.receive('bb'.repeat(16), null)).toBe(true);
    expect(gate.pendingEtagHex).toBe('bb'.repeat(16));
  });

  it('receive: newer notification replaces older pending', () => {
    const gate = new LayoutRefetchGate();
    gate.receive('bb'.repeat(16), null);
    gate.receive('cc'.repeat(16), null);
    expect(gate.pendingEtagHex).toBe('cc'.repeat(16));
  });

  it('takeRunnable: no pending → null', () => {
    const gate = new LayoutRefetchGate();
    expect(gate.takeRunnable('aa'.repeat(16))).toBeNull();
  });

  it('takeRunnable: pending without hold → returns etag and clears pending', () => {
    const gate = new LayoutRefetchGate();
    gate.receive('bb'.repeat(16), 'aa'.repeat(16));
    expect(gate.takeRunnable('aa'.repeat(16))).toBe('bb'.repeat(16));
    expect(gate.pendingEtagHex).toBeNull();
    // 소거 후 재호출은 null — 중복 refetch 없음.
    expect(gate.takeRunnable('aa'.repeat(16))).toBeNull();
  });

  it('takeRunnable: echo-race absorbed — known etag caught up during debounce', () => {
    const gate = new LayoutRefetchGate();
    // 0x80 이 HTTP 응답보다 먼저 도착 (당시 known = 옛 etag) → schedule.
    gate.receive('bb'.repeat(16), 'aa'.repeat(16));
    // debounce 사이 자기 발신 PUT 응답이 known 을 bb 로 갱신 → flush 생략.
    expect(gate.takeRunnable('bb'.repeat(16))).toBeNull();
    expect(gate.pendingEtagHex).toBeNull();
  });

  it('hold defers flush; release signals re-schedule; deferred etag survives', () => {
    const gate = new LayoutRefetchGate();
    gate.hold();
    expect(gate.receive('bb'.repeat(16), 'aa'.repeat(16))).toBe(true);
    // hold 중 flush 시점 → run 금지, pending 유지 (드래그 중 items.clear() 차단).
    expect(gate.takeRunnable('aa'.repeat(16))).toBeNull();
    expect(gate.pendingEtagHex).toBe('bb'.repeat(16));
    // release → pending 있으므로 re-schedule 신호.
    expect(gate.release()).toBe(true);
    expect(gate.takeRunnable('aa'.repeat(16))).toBe('bb'.repeat(16));
  });

  it('release without pending → no re-schedule signal', () => {
    const gate = new LayoutRefetchGate();
    gate.hold();
    expect(gate.release()).toBe(false);
  });

  it('nested holds: only the last release triggers re-schedule', () => {
    const gate = new LayoutRefetchGate();
    gate.hold();
    gate.hold();
    gate.receive('bb'.repeat(16), null);
    expect(gate.release()).toBe(false); // still 1 hold left
    expect(gate.takeRunnable(null)).toBeNull();
    expect(gate.release()).toBe(true);
    expect(gate.takeRunnable(null)).toBe('bb'.repeat(16));
  });

  it('release is clamped at zero holds (defensive imbalance)', () => {
    const gate = new LayoutRefetchGate();
    expect(gate.release()).toBe(false);
    expect(gate.holds).toBe(0);
    // 이후 흐름은 정상 동작.
    gate.receive('bb'.repeat(16), null);
    expect(gate.takeRunnable(null)).toBe('bb'.repeat(16));
  });
});
