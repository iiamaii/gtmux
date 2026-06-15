// Terminal panel label derivation — single source of truth per ADR-0050 D3.
//
// Background: the terminal panel label lives in the persisted layout
// `ItemCommon.label` (per-(session, panel)). It is *not* read from the
// in-memory `terminal_meta` (PATCH /api/terminals), which is wiped every boot.
// These helpers centralise the "derive a display string from item.label"
// fallback chain so PanelNode, ItemInfoView, and TerminalListView stay
// consistent and the rule is unit-testable.

import type { CanvasItem } from '$lib/types/canvas';

/**
 * Pure layout rename transform shared by every rename surface (PanelNode
 * header, LayerTreeView, ItemInfoView Inspector) — ADR-0050 D2.
 *
 * A note's user-visible name is its `title`; every other item type (terminal
 * included) writes its persisted `label`. This is the value that
 * applyMutation / optimisticMutation maps over `items` before PUT
 * /api/sessions/:name/layout. Extracting it keeps terminal labels on the exact
 * same persist path as text/note/figure labels.
 */
export function renameItemLabel(item: CanvasItem, next: string): CanvasItem {
  if (item.type === 'note') {
    return { ...item, title: next } as CanvasItem;
  }
  return { ...item, label: next } as CanvasItem;
}

/** Short id for fallback display — first 8 chars of a UUID (dashes dropped). */
export function shortTerminalId(id: string): string {
  return id.replace(/-/g, '').slice(0, 8);
}

/**
 * Header label for a terminal panel (ADR-0050 D3).
 * Priority: layout `item.label` (persisted) → `pane_id` → `id`.
 * A blank/whitespace-only label is treated as unset.
 */
export function terminalHeaderLabel(
  label: string | null | undefined,
  paneId: string | undefined,
  id: string,
): string {
  return label?.trim() || paneId || id;
}

/**
 * Pool-list display name for a terminal (ADR-0050 D3, TerminalListView).
 * Per-panel model: a terminal's label is only known when it has an item in
 * the *current* session layout. If `sessionLabel` is present we show it;
 * otherwise fall back to a short id (`t` + first 8 of the UUID). The
 * in-memory terminal_meta label is intentionally never consulted.
 */
export function terminalPoolDisplayName(
  sessionLabel: string | null | undefined,
  id: string,
): string {
  const trimmed = sessionLabel?.trim();
  if (trimmed != null && trimmed.length > 0) return trimmed;
  return `t${shortTerminalId(id)}`;
}
