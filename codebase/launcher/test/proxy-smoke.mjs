// Explicit network opt-in: downloads Caddy, but never requests a certificate.
import assert from 'node:assert/strict';
import { mkdtemp, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createServer } from 'node:http';
import { ProxyManager, proxyConfiguration } from '../src/proxy.mjs';
const root = await mkdtemp(join(tmpdir(), 'gtmux-caddy-smoke-'));
const backend = createServer((_req, res) => { res.setHeader('x-gtmux-server-id', 'isolated-smoke'); res.end('isolated backend'); });
await new Promise(r => backend.listen(0, '127.0.0.1', r)); const port = backend.address().port;
const probe = createServer(); await new Promise(r => probe.listen(0, '127.0.0.1', r)); const proxyPort = probe.address().port; await new Promise(r => probe.close(r));
const proxy = new ProxyManager({ root, serverPath: async p => p, config: async () => ({ public_origin: 'https://terminal.example.com', server: { port } }) });
try {
  await proxy.install();
  const config = proxyConfiguration('terminal.example.com', port, 80, proxyPort);
  delete config.apps.tls; // This forwarding check must never contact an ACME CA.
  config.admin = { listen: 'unix/' + join(proxy.root, 'admin.sock') };
  config.apps.http.servers.gtmux.listen = [`127.0.0.1:${proxyPort}`];
  config.apps.http.servers.gtmux.automatic_https = { disable: true };
  config.apps.http.servers.gtmux.routes[0].match = undefined;
  await writeFile(join(proxy.root, 'caddy.json'), JSON.stringify(config));
  await writeFile(join(proxy.root, 'exposure.json'), JSON.stringify({ domain: 'terminal.example.com', mode: 'managed' }));
  await proxy.start();
  const response = await fetch(`http://127.0.0.1:${proxyPort}/healthz`);
  assert.equal(await response.text(), 'isolated backend');
  assert.equal(response.headers.get('x-gtmux-server-id'), 'isolated-smoke');
  await proxy.stop(); assert.equal(proxy.child, null);
  console.log('Verified Caddy download, checksum, config validation, local proxy forwarding and owned shutdown passed. No ACME request was made.');
} finally {
  await proxy.stop(); await new Promise(r => backend.close(r)); await rm(root, { recursive: true, force: true });
}
