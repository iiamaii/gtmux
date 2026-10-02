import { createServer } from 'node:http';
import { readFileSync } from 'node:fs';
import { parse } from 'smol-toml';
const config = parse(readFileSync(process.argv[process.argv.indexOf('--config') + 1], 'utf8'));
const server = createServer(async (req, res) => {
  if (req.url === '/healthz') { res.setHeader('x-gtmux-server-id', 'fixture'); res.end('{"ok":true}'); return; }
  if (req.url === '/api/shutdown') {
    let body = ''; for await (const data of req) body += data;
    if (JSON.parse(body).credential !== 'fixture-token') { res.writeHead(401); res.end('{}'); return; }
    res.writeHead(202); res.end('{}'); setTimeout(() => server.close(() => process.exit(6)), 20); return;
  }
  res.writeHead(404); res.end('{}');
});
server.listen(config.server.port, '127.0.0.1', () => console.log(`Open URL: http://127.0.0.1:${config.server.port}/auth/bootstrap?token=fixture-token`));
process.on('SIGTERM', () => server.close(() => process.exit(0)));
