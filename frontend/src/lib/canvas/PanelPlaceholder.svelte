<script lang="ts">
  // Panel placeholder — R8 §F8 정책 (b) "placeholder on zoom".
  //
  // Zoom != 1 (|zoom - 1| >= 0.02) 구간 또는 D16 Suspended (visibility=false / minimized)
  // 구간에서 xterm.js DOM 대신 본 컴포넌트를 렌더한다. 본 컴포넌트는 *극경량* — xterm + WebGL
  // 렌더러의 cell metric 캐시 비용을 0으로 절감 (50 pane × zoom step 시 frame jank 회피).
  //
  // Last-frame snapshot 썸네일은 P1+ (R8 §F8 마지막 단락: serialize addon 거절 정합).
  // MVP는 label + 배경색 + hint 문구만.

  let {
    label,
    reason = 'zoom'
  }: {
    label: string;
    /**
     * Placeholder 표시 이유. 사용자에 어떤 액션으로 본문 복원이 가능한지 힌트를 다르게 표시.
     * - `zoom`: zoom in/out 중 — viewport zoom을 1로 맞추면 복원.
     * - `suspended`: Panel Streaming State Suspended — visibility/minimize 해제 시 복원.
     */
    reason?: 'zoom' | 'suspended';
  } = $props();

  const hint = $derived(
    reason === 'zoom' ? 'zoom in to view terminal' : 'panel suspended'
  );
</script>

<div class="placeholder" aria-label={`Placeholder for ${label}`}>
  <span class="placeholder-label">{label}</span>
  <span class="placeholder-hint">{hint}</span>
</div>

<style>
  .placeholder {
    width: 100%;
    height: 100%;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 6px;
    background: #0a0f1c;
    color: #94a3b8;
    box-sizing: border-box;
    padding: 8px;
    overflow: hidden;
  }

  .placeholder-label {
    font-size: 14px;
    font-weight: 500;
    color: #e5e7eb;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 100%;
  }

  .placeholder-hint {
    font-size: 11px;
    color: #64748b;
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }
</style>
