<script lang="ts">
  import '../../styles/settings-controls.css';
  // Shared settings on/off control. Preserve native keyboard and label behavior.
  let { checked, disabled = false, onchange }: {
    checked: boolean;
    disabled?: boolean;
    onchange: (event: Event & { currentTarget: EventTarget & HTMLInputElement }) => void;
  } = $props();
</script>
<input class="native-toggle" type="checkbox" role="switch" {checked} {disabled} {onchange} />
