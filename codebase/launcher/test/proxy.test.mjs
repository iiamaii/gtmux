import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, writeFile, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { ProxyManager, validateDomain, proxyConfiguration } from '../src/proxy.mjs';
test('proxy domain and listener inputs cannot inject configuration', () => {
  for (const value of ['localhost', 'https://example.com', 'example.com/path', '127.0.0.1', 'a.com\nbad', '*.example.com']) assert.throws(() => validateDomain(value));
  assert.equal(validateDomain('TERM.Example.com'), 'term.example.com');
  assert.throws(() => proxyConfiguration('example.com', 9001, 9001, 443));
  const json = proxyConfiguration('example.com', 9001);
  assert.equal(json.apps.http.servers.gtmux.routes[0].handle[0].upstreams[0].dial, '127.0.0.1:9001');
});
test('preview has no side effects; changed config blocks apply before stopping server', async t => {
  const root = await mkdtemp(join(tmpdir(), 'gtmux-proxy-test-')); t.after(() => rm(root, { recursive: true, force: true }));
  const configPath = join(root, 'server.toml'); const original = '[server]\nsession="desktop"\nport=19091\nbind="127.0.0.1"\n';
  await writeFile(configPath, original); let stopped = 0;
  const manager = new ProxyManager({ root, configPath, serverPath: async path => path, stop: async () => stopped++ }, { resolveDNS: async () => [{ address: '203.0.113.1' }] });
  const plan = await manager.preview({ domain: 'terminal.example.com', mode: 'existing' });
  assert.equal(await readFile(configPath, 'utf8'), original); assert(plan.changes.length > 0);
  await assert.rejects(manager.apply({ planId: plan.id }), /confirm/);
  await writeFile(configPath, original + '# external edit\n');
  await assert.rejects(manager.apply({ planId: plan.id, confirmed: true }), /changed/); assert.equal(stopped, 0);
});

test('failed proxy startup rolls back config; existing deployment cannot overwrite local backup', async t => {
  const root = await mkdtemp(join(tmpdir(), 'gtmux-proxy-rollback-')); t.after(() => rm(root, { recursive: true, force: true }));
  const configPath = join(root, 'server.toml'); const original = '[server]\nsession="desktop"\nport=19091\nbind="127.0.0.1"\n';
  await writeFile(configPath, original);
  const server = { root, configPath, child: {}, serverPath: async p => p,
    async stop() { this.child = null; }, async start() { this.child = {}; } };
  const manager = new ProxyManager(server, { resolveDNS: async () => [] });
  const { mkdir } = await import('node:fs/promises'); await mkdir(manager.root);
  await writeFile(join(manager.root, 'version'), '2.11.4');
  const plan = await manager.preview({ domain: 'terminal.example.com', mode: 'managed' });
  manager.start = async () => { throw new Error('occupied proxy port'); };
  await assert.rejects(manager.apply({ planId: plan.id, confirmed: true }), /occupied proxy port/);
  assert.equal(await readFile(configPath, 'utf8'), original);
  assert.equal((await manager.status()).configuration, null); assert(server.child);
  const local = await manager.preview({ domain: 'terminal.example.com', mode: 'existing' });
  await manager.apply({ planId: local.id, confirmed: true });
  await assert.rejects(manager.preview({ domain: 'other.example.com', mode: 'existing' }), /Restore local/);
  assert.equal(await readFile(join(manager.root, 'server.before.toml'), 'utf8'), original);
});

test('rollback preserves concurrent external edits and public URL requires successful verification', async t => {
  const root = await mkdtemp(join(tmpdir(), 'gtmux-proxy-race-')); t.after(() => rm(root, { recursive: true, force: true }));
  const configPath = join(root, 'server.toml'); await writeFile(configPath, '[server]\nsession="desktop"\nport=19091\nbind="127.0.0.1"\n');
  const server = { root, configPath, child: null, serverPath: async p => p, stop: async () => {},
    start: async () => { await writeFile(configPath, '# external edit during failed start'); throw new Error('start failed'); },
    openURL: () => 'http://127.0.0.1:19091/auth/bootstrap?token=secret' };
  const manager = new ProxyManager(server, { resolveDNS: async () => [] });
  const plan = await manager.preview({ domain: 'terminal.example.com', mode: 'existing' });
  await assert.rejects(manager.apply({ planId: plan.id, confirmed: true }), /edited externally/);
  assert.equal(await readFile(configPath, 'utf8'), '# external edit during failed start');
  manager.verify = async () => { throw new Error('wrong HTTPS backend'); };
  await assert.rejects(manager.workspaceURL(), /wrong HTTPS backend/);
  manager.verify = async () => ({ verified: true });
  assert.equal(await manager.workspaceURL(), 'https://terminal.example.com/auth/bootstrap?token=secret');
});


test('disabling HTTPS preserves reusable settings across manager launches without retaining credentials', async t => {
  const root = await mkdtemp(join(tmpdir(), 'gtmux-proxy-toggle-')); t.after(() => rm(root, { recursive: true, force: true }));
  const configPath = join(root, 'server.toml');
  const original = '[server]\nsession="desktop"\nport=19091\nbind="127.0.0.1"\n';
  await writeFile(configPath, original);
  const server = { root, configPath, serverPath: async p => p, stop: async () => {}, start: async () => {} };
  const manager = new ProxyManager(server, { resolveDNS: async () => [] });
  const input = { domain: 'terminal.example.com', mode: 'existing', httpPort: 8080, httpsPort: 8443 };
  const plan = await manager.preview(input);
  await manager.apply({ planId: plan.id, confirmed: true, credential: 'never-save-this' });
  // Simulate a deployment created before reusable settings were introduced.
  await rm(join(manager.root, 'last-settings.json'));
  await manager.disable('never-save-this');
  assert.equal(await readFile(configPath, 'utf8'), original);
  const reopened = new ProxyManager(server, { resolveDNS: async () => [] });
  const status = await reopened.status();
  assert.equal(status.configuration, null);
  assert.deepEqual(status.lastSettings, { ...input, certificate: 'public', externalPort: 443 });
  const next = await reopened.preview(status.lastSettings);
  await reopened.apply({ planId: next.id, confirmed: true });
  assert.equal((await reopened.status()).configuration.domain, input.domain);
});
