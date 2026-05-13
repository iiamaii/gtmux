<script lang="ts">
  // xterm.js hand-rolled wrapper — R8 §F1.
  // lifecycle: $effect 안에서 Terminal 생성 → addon load → open → fit → dispatcher 등록.
  let { foo }: { foo?: unknown } = $props();
</script>

<div class="xterm-host">
  <!-- term.open(containerEl) 마운트 위치. -->
</div>
