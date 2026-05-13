<script lang="ts">
  // Svelte Flow 캔버스 루트 — R3 보고서 채택 라이브러리.
  let { foo }: { foo?: unknown } = $props();
</script>

<div class="canvas-root">
  <!-- @xyflow/svelte SvelteFlow 마운트 위치. P0 구현에서 노드/엣지 매퍼 채움. -->
</div>

<style>
  .canvas-root {
    width: 100%;
    height: 100%;
  }
</style>
