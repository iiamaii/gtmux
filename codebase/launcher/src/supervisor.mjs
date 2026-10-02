import { EventEmitter } from 'node:events';
import { spawn } from 'node:child_process';
import { mkdir, stat, appendFile } from 'node:fs/promises';
import { resolve, join, isAbsolute } from 'node:path';
import { createServer } from 'node:net';
import { parse, stringify } from 'smol-toml';
import { replaceStartupPort, replaceStartupWorkspace } from './startup-config.mjs';
import { atomicWrite, readOptional, revision } from './files.mjs';

export async function checkPort(port, host = '127.0.0.1') {
  if (!Number.isInteger(port) || port < 1024 || port > 65535) throw new Error('Port must be an integer from 1024 to 65535.');
  await new Promise((done, reject) => {
    const server = createServer(); server.once('error', reject);
    server.listen({ host, port, exclusive: true }, () => server.close(done));
  });
}
export function validatePreferences(value) {
  if (!['web', 'app', 'both'].includes(value.mode)) throw new Error('Choose web, app or both.');
  if (typeof value.background !== 'boolean') throw new Error('Background preference must be a boolean.');
  if (!Number.isInteger(value.port) || value.port < 1024 || value.port > 65535) throw new Error('Port must be from 1024 to 65535.');
  if (typeof value.workspace !== 'string' || !isAbsolute(value.workspace)) throw new Error('Choose an absolute workspace path.');
  if (value.theme !== undefined && !['system','light','dark'].includes(value.theme)) throw new Error('Choose system, light or dark theme.');
  return { theme: value.theme ?? 'system', mode: value.mode, background: value.background, port: value.port, workspace: value.workspace };
}
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
export class Supervisor extends EventEmitter {
  constructor({ root, binary, frontend, launch = spawn, environment = process.env, adapter = null }) {
    super(); this.adapter = adapter; this.serverPath = path => adapter ? adapter.toServer(path) : Promise.resolve(path);
    this.hostPath = path => adapter ? adapter.toHost(path) : Promise.resolve(path);
    this.root = resolve(root); this.binary = resolve(binary); this.frontend = resolve(frontend);
    this.launch = launch; this.environment = environment; this.child = null; this.phase = 'stopped';
    this.error = ''; this.bootstrap = null; this.tail = Promise.resolve(); this.preferences = null;
    this.configPath = join(this.root, 'server.toml'); this.logPath = join(this.root, 'server.log');
  }
  serial(action) { const pending = this.tail.then(action); this.tail = pending.catch(() => {}); return pending; }
  async load() {
    await mkdir(this.root, { recursive: true, mode: 0o700 });
    const text = await readOptional(join(this.root, 'preferences.json'));
    this.preferences = text ? validatePreferences(JSON.parse(text)) : null;
    if (this.preferences && await readOptional(this.configPath)) {
      const config = await this.config();
      this.preferences.port = config.server.port;
      if (config.server_workspace) this.preferences.workspace = await this.hostPath(config.server_workspace);
    }
    return this.status();
  }
  status() {
    return { state: this.phase, error: this.error, preferences: this.preferences,
      pid: this.child?.pid ?? null, configPath: this.configPath, logPath: this.logPath,
      configured: this.preferences !== null, canRestart: this.child !== null,
      address: this.bootstrap ? new URL(this.bootstrap).origin : null };
  }
  changed() { this.emit('state', this.status()); }
  configure(value) { return this.serial(async () => {
    const next = validatePreferences(value);
    if (!(await stat(next.workspace)).isDirectory()) throw new Error('Workspace is not a directory.');
    if (this.child && (next.port !== this.preferences.port || next.workspace !== this.preferences.workspace))
      throw new Error('Stop the server before changing its startup port or workspace. Running programs will end.');
    const existing = await readOptional(this.configPath);
    if (existing && next.port !== this.preferences?.port)
      throw new Error('This server already has a TOML configuration. Change its port/workspace in Settings → Server, then restart.');
    if (existing && next.workspace !== this.preferences?.workspace) {
      if (revision(existing) !== value.revision) throw new Error('Configuration changed on disk. Reload the workspace and try again.');
      const edited = replaceStartupWorkspace(existing, await this.serverPath(next.workspace));
      if (revision(await readOptional(this.configPath)) !== value.revision) throw new Error('Configuration changed while saving. Reload and retry.');
      await atomicWrite(this.configPath, edited);
    }
    if (!existing) await atomicWrite(this.configPath, stringify({ schema_version: 1, frontend_dist: await this.serverPath(this.frontend),
      server_workspace: await this.serverPath(next.workspace), default_session_workspace: await this.serverPath(next.workspace),
      workspace_path: await this.serverPath(join(this.root, 'store')), server: { session: 'desktop', port: next.port, bind: '127.0.0.1' } }));
    await atomicWrite(join(this.root, 'preferences.json'), JSON.stringify(next, null, 2));
    this.preferences = next; this.changed(); return this.status();
  }); }
  async savedWorkspace() {
    const text = await readOptional(this.configPath);
    if (!text || !this.preferences) throw new Error('Complete setup first.');
    const config = parse(text);
    return { workspace: config.server_workspace ? await this.hostPath(config.server_workspace) : this.preferences.workspace, revision: revision(text) };
  }
  async savedPort() {
    const text = await readOptional(this.configPath);
    if (!text) throw new Error('Complete setup first.');
    return { port: parse(text).server.port, revision: revision(text) };
  }
  changePort({ port, revision: expected }) { return this.serial(async () => {
    if (this.child) throw new Error('Stop this server before changing its startup port.');
    await checkPort(port);
    const text = await readOptional(this.configPath);
    if (!text || revision(text) !== expected) throw new Error('Configuration changed on disk. Reload the saved port and try again.');
    const next = replaceStartupPort(text, port);
    if (revision(await readOptional(this.configPath)) !== expected) throw new Error('Configuration changed while saving. Reload and retry.');
    await atomicWrite(this.configPath, next);
    this.preferences.port = port;
    await atomicWrite(join(this.root, 'preferences.json'), JSON.stringify(this.preferences, null, 2));
    return this.savedPort();
  }); }
  async config() {
    const config = parse(await readOptional(this.configPath) ?? '');
    if (config.server?.session !== 'desktop' || config.server.bind !== '127.0.0.1')
      throw new Error('Managed server must keep instance desktop and loopback bind. Public access uses the proxy.');
    return config;
  }
  start() { return this.serial(() => this.startOwned()); }
  async startOwned() {
    if (this.child) { if (this.phase === 'running') return this.status(); throw new Error('Server is still starting or stopping.'); }
    if (!this.preferences) throw new Error('Complete initial setup first.');
    const config = await this.config(); await checkPort(config.server.port);
    const env = Object.fromEntries(Object.entries(this.environment).filter(([key]) => !key.startsWith('GTMUX_') && key !== 'TMUX'));
    Object.assign(env, { XDG_STATE_HOME: await this.serverPath(join(this.root, 'state')), XDG_DATA_HOME: await this.serverPath(join(this.root, 'data')), XDG_CONFIG_HOME: await this.serverPath(join(this.root, 'config')) });
    this.phase = 'starting'; this.error = ''; this.bootstrap = null; this.changed();
    const args = ['start', '--name', 'desktop', '--config', await this.serverPath(this.configPath)];
    const command = this.adapter ? await this.adapter.command(this.binary, args, env) : { binary: this.binary, args, env };
    const child = this.launch(command.binary, command.args,
      { env: command.env, cwd: this.preferences.workspace, stdio: ['ignore', 'pipe', 'pipe'], windowsHide: true });
    this.child = child;
    const output = () => { let pending = ''; return chunk => {
      pending += chunk.toString();
      const lines = pending.split(/\r?\n/); pending = lines.pop().slice(-16384);
      for (const line of lines) {
        const match = line.match(/Open URL:\s+(http:\/\/[^\s]+)/);
        if (match) {
          const url = new URL(match[1]);
          if (url.hostname === '127.0.0.1' && Number(url.port) === config.server.port && url.pathname === '/auth/bootstrap') this.bootstrap = url.href;
        }
        void appendFile(this.logPath, line.replace(/token=[^\s&]+/g, 'token=[redacted]') + '\n', { mode: 0o600 }).catch(() => {});
      }
    }; };
    child.stdout.on('data', output()); child.stderr.on('data', output());
    child.on('error', e => { this.error = e.message; this.phase = 'failed'; this.child = null; this.changed(); });
    child.on('exit', (code, signal) => {
      if (this.child !== child) return;
      this.child = null; this.bootstrap = null;
      this.phase = [0, 6].includes(code) || this.phase === 'stopping' ? 'stopped' : 'failed';
      if (this.phase === 'failed') this.error = `Server exited (${signal ?? code}). See ${this.logPath}`;
      this.changed();
    });
    for (let attempt = 0; attempt < 150; attempt++) {
      if (!this.child) throw new Error(this.error || 'Server exited during startup.');
      if (this.bootstrap) {
        try {
          const response = await fetch(new URL('/healthz', this.bootstrap), { signal: AbortSignal.timeout(500) });
          if (response.ok) {
            this.preferences.port = config.server.port; this.preferences.workspace = config.server_workspace ? await this.hostPath(config.server_workspace) : this.preferences.workspace;
            this.instanceId = response.headers.get('x-gtmux-server-id');
            this.phase = 'running'; this.changed(); return this.status();
          }
        } catch { /* listener is not ready yet */ }
      }
      await delay(100);
    }
    this.error = 'Startup timed out. Stop the owned server and inspect its log before retrying.';
    this.phase = 'failed'; this.changed(); throw new Error(this.error);
  }
  stop(credential) { return this.serial(() => this.stopOwned(credential)); }
  async stopOwned(credential) {
    const child = this.child; if (!child) return this.status();
    if (this.bootstrap) {
      await this.refreshToken();
      const token = new URL(this.bootstrap).searchParams.get('token');
      let response;
      try { response = await fetch(new URL('/api/shutdown', this.bootstrap), { method: 'POST',
        headers: { Authorization: `Bearer ${token}`, 'Content-Type': 'application/json' },
        body: JSON.stringify({ credential: credential || token }), signal: AbortSignal.timeout(3000) }); }
      catch { if (process.platform === 'win32') throw new Error('Server cannot be reached. No forced termination was performed.'); }
      if (response && !response.ok) throw new Error(`Shutdown rejected (${response.status}). Enter the current server password/token.`);
      if (!response) child.kill('SIGTERM');
    } else {
      if (process.platform === 'win32') throw new Error('Startup has not completed. Inspect the server log before stopping it externally.');
      child.kill('SIGTERM');
    }
    this.phase = 'stopping'; this.changed();
    for (let attempt = 0; attempt < 250 && this.child === child; attempt++) await delay(100);
    if (this.child === child) throw new Error('Shutdown is still in progress. No forced termination was performed.');
    return this.status();
  }
  restart(credential) { return this.serial(async () => { await this.stopOwned(credential); return this.startOwned(); }); }
  async refreshToken() {
    if (!this.bootstrap) return;
    const token = (await readOptional(join(this.root, 'state', 'gtmux', 'desktop.token')))?.trim();
    if (token && /^[A-Za-z0-9_-]{43}$/.test(token)) { const url = new URL(this.bootstrap); url.searchParams.set('token', token); this.bootstrap = url.href; }
  }
  openURL() { if (!this.bootstrap || this.phase !== 'running') throw new Error('Start the server first.'); return this.bootstrap; }
}
