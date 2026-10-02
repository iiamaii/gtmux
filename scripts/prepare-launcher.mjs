// Stage only generated release resources. Never touches a running server's dist.
import { pathToFileURL } from 'node:url';
import { cp, mkdir, access, chmod } from 'node:fs/promises';
import { resolve, basename, join } from 'node:path';
const [binaryArg, frontendArg, targetArg] = process.argv.slice(2);
if (!binaryArg || !frontendArg) throw new Error('Usage: node scripts/prepare-launcher.mjs <gtmux binary> <frontend dist>');
const binary = resolve(binaryArg), frontend = resolve(frontendArg);
await access(binary); await access(join(frontend, 'index.html'));
const target = targetArg ? pathToFileURL(resolve(targetArg) + '/') : new URL('../codebase/launcher/resources/', import.meta.url);
await mkdir(target, { recursive: true });
const name = basename(binary).endsWith('.exe') ? 'gtmux.exe' : 'gtmux';
await cp(binary, new URL(name, target)); await chmod(new URL(name, target), 0o755);
await cp(frontend, new URL('frontend/', target), { recursive: true, force: true });
console.log('Staged desktop server and frontend resources.');
