// 디코더 헬퍼 — varint + int32 LE + float32 LE.
// SSoT wire-protocol.md 정본. JS DataView 은 명시적 littleEndian=true 필수.

/**
 * Varint(LEB128 unsigned) 디코더.
 * 반환: { value, nextOffset }.
 */
export function readVarint(view: DataView, offset: number): { value: number; nextOffset: number } {
  let result = 0;
  let shift = 0;
  let cursor = offset;
  while (true) {
    const byte = view.getUint8(cursor);
    cursor += 1;
    result |= (byte & 0x7f) << shift;
    if ((byte & 0x80) === 0) break;
    shift += 7;
    if (shift > 35) throw new Error('varint too long');
  }
  return { value: result, nextOffset: cursor };
}

/** int32 little-endian. */
export function readInt32LE(view: DataView, offset: number): number {
  return view.getInt32(offset, true);
}

/** float32 little-endian. */
export function readFloat32LE(view: DataView, offset: number): number {
  return view.getFloat32(offset, true);
}
