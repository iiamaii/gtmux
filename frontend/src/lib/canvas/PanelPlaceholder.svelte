<script lang="ts">
  // Zoom != 1 시 표시되는 플레이스홀더 — R8 §F8 정책 (b).
  let { foo }: { foo?: unknown } = $props();
</script>

<div class="panel-placeholder">
  <!-- label + 배경색 (Group.color inherit) + minimize/lock badge. -->
</div>
