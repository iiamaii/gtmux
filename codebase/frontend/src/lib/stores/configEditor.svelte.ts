import type { ConfigSnapshot } from '$lib/http/config';
// Preserve unsaved edits across Settings sections, never store credentials.
export const configEditor = $state({ snapshot: null as ConfigSnapshot | null, contents: '', port: 9001,
  workspace: '', sessionWorkspace: '', fieldsDirty: false });
