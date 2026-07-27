// noteCaret — pure offset mapping for note body caret-at-point edit entry
// (ADR-0018 D9 amend 2026-07-23). The displayed note body is plain text
// (`<pre>{body}</pre>`), so a caret hit inside the display DOM maps 1:1 onto
// an offset in the body string. DOM glue (caretPositionFromPoint + TreeWalker)
// lives in NoteNode; this module owns the DOM-free summation/clamping.

/**
 * Map a caret hit inside the displayed body element to an offset in the body
 * string.
 *
 * @param nodeLengths lengths of the display element's text nodes in document
 *   order (usually a single entry — Svelte renders `{body}` as one text node).
 * @param hitIndex index into `nodeLengths` of the text node the caret landed
 *   in. Pass -1 when the hit is not a text node inside the display element
 *   (head padding, gap, element-node hit) — clamps to end of text.
 * @param hitOffset character offset within the hit text node.
 * @param bodyLength length of the body string (clamp upper bound).
 */
export function mapDisplayHitToBodyOffset(
  nodeLengths: number[],
  hitIndex: number,
  hitOffset: number,
  bodyLength: number,
): number {
  const hitLength = nodeLengths[hitIndex];
  if (hitIndex < 0 || hitLength === undefined) return bodyLength;
  let offset = 0;
  for (let i = 0; i < hitIndex; i += 1) offset += nodeLengths[i] ?? 0;
  offset += Math.min(Math.max(0, hitOffset), hitLength);
  return Math.min(Math.max(0, offset), bodyLength);
}
