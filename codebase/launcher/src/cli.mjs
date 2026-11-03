import { resolve } from 'node:path';
import { homedir } from 'node:os';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';
import { Supervisor } from './supervisor.mjs';
import { ProxyManager } from './proxy.mjs';
import { controlServer } from './control.mjs';
const { values } = parseArgs({ options: { 'data-dir': { type: 'string' }, binary: { type: 'string' }, frontend: { type: 'string' } } });
const resources = fileURLToPath(new URL('../resources/', import.meta.url));
const supervisor = new Supervisor({ root: values['data-dir'] ?? resolve(homedir(), '.local/share/gtmux-manager'),
  binary: values.binary ?? resolve(resources, process.platform === 'win32' ? 'gtmux.exe' : 'gtmux'), frontend: values.frontend ?? resolve(resources, 'frontend') });
await supervisor.load(); const proxy = new ProxyManager(supervisor);
const control = await controlServer({ supervisor, proxy });
console.log(`gtmux local manager: ${control.url}`);
console.log('The server starts only when you choose Start. This manager does not expose its control API publicly.');
let quitting = false;
const quit = async () => {
  if (quitting) return; quitting = true;
  try { await supervisor.stop(); await proxy.stop(); await control.close(); process.exitCode = 0; }
  catch (error) { quitting = false; console.error(error.message); }
};
process.on('SIGINT', quit); process.on('SIGTERM', quit);
