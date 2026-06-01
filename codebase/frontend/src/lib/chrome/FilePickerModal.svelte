<script lang="ts">
  /**
   * Compatibility wrapper for the old file_path picker API.
   *
   * The real UI is the unified FileExplorer. Existing callers keep using this
   * component until all file/image/document/workspace selection flows are
   * migrated to FileExplorer directly.
   */

  import FileExplorer from './FileExplorer.svelte';

  interface Props {
    open: boolean;
    onCancel: () => void;
    onSelect: (absolutePath: string) => void;
    onUnauthorized?: () => void;
    initialDir?: string;
    accept?: { extensions: string[]; description: string } | null;
  }

  const {
    open,
    onCancel,
    onSelect,
    onUnauthorized,
    initialDir = '',
    accept = null,
  }: Props = $props();
</script>

<FileExplorer
  {open}
  mode="file"
  title={accept === null ? 'Pick a file' : `Pick ${accept.description}`}
  filter={accept?.extensions ?? []}
  filterDescription={accept?.description ?? 'files'}
  {initialDir}
  onCancel={onCancel}
  onPick={onSelect}
  onUnauthorized={onUnauthorized}
/>
