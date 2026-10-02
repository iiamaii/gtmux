import { parse } from 'smol-toml';
import { isDeepStrictEqual } from 'node:util';
// Preserve comments and unrelated TOML. Unusual TOML spellings fail with a clear
// instruction instead of silently replacing the entire user's document.
export function replaceStartupPort(text, port) {
  const before = parse(text), old = before.server.port;
  let inServer = false, changed = false;
  const lines = text.split('\n').map(line => {
    const header = line.match(/^\s*\[([^\]]+)\]\s*(?:#.*)?$/);
    if (header) inServer = header[1].trim() === 'server';
    if (inServer && /^\s*port\s*=/.test(line)) {
      if (!/^\s*port\s*=\s*\d[\d_]*\s*(?:#.*)?$/.test(line)) throw new Error('Edit this port expression in the TOML file, then reload it.');
      changed = true; return line.replace(/(=\s*)\d[\d_]*/,`$1${port}`);
    }
    // Generated loopback allowlists may use either TOML quote style.
    if (/^\s*(host_allowlist|cors_origins)\s*=/.test(line)) {
      for (const host of [`127.0.0.1:${old}`,`localhost:${old}`,`http://127.0.0.1:${old}`,`http://localhost:${old}`])
        for (const q of ['"',"'"]) line = line.replaceAll(q+host+q,q+host.replace(`:${old}`,`:${port}`)+q);
    }
    return line;
  });
  const next = lines.join('\n');
  const after = parse(next);
  for (const key of ['host_allowlist', 'cors_origins']) {
    const stale = new Set([`127.0.0.1:${old}`, `localhost:${old}`, `http://127.0.0.1:${old}`, `http://localhost:${old}`]);
    if (port !== old && after.security?.[key]?.some(value => stale.has(value)))
      throw new Error('Edit multiline loopback allowlists and the port together in TOML, then reload. Nothing saved.');
  }
  if (!changed || parse(next).server.port !== port) throw new Error('Use a [server] port entry in TOML, then reload it here.');
  return next;
}

// Edit root workspace keys only. Preserve independently configured session roots.
export function replaceStartupWorkspace(text, workspace) {
  const before = parse(text);
  const expected = parse(text);
  const keys = ['server_workspace'];
  if (before.default_session_workspace === before.server_workspace) keys.push('default_session_workspace');
  for (const key of keys) expected[key] = workspace;
  let atRoot = true;
  const seen = new Set();
  const lines = text.split('\n').map(line => {
    if (/^\s*\[/.test(line)) atRoot = false;
    if (!atRoot) return line;
    for (const key of keys) {
      const prefix = new RegExp(`^\\s*${key}\\s*=`);
      if (!prefix.test(line)) continue;
      const match = line.match(/^(\s*\w+\s*=\s*)("(?:[^"\\]|\\.)*"|'[^']*')(\s*(?:#.*)?)$/);
      if (!match) throw new Error('Edit this workspace expression in TOML, then reload it.');
      seen.add(key);
      return match[1] + JSON.stringify(workspace) + match[3];
    }
    return line;
  });
  for (const key of keys) if (!seen.has(key)) {
    if (Object.hasOwn(before, key)) throw new Error('Edit this workspace expression in TOML, then reload it.');
    lines.unshift(`${key} = ${JSON.stringify(workspace)}`);
  }
  const next = lines.join('\n');
  if (!isDeepStrictEqual(parse(next), expected)) throw new Error('Workspace edit would change unrelated TOML. Edit the file and reload.');
  return next;
}
