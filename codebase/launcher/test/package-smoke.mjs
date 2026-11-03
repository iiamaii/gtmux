// Locate electron-builder's native unpacked output; lifecycle fixture owns all data.
import { access } from 'node:fs/promises';
import { resolve } from 'node:path';
const candidates=process.platform==='win32'?['dist/win-unpacked/gtmux.exe']:process.platform==='darwin'?['dist/mac/gtmux.app/Contents/MacOS/gtmux','dist/mac-arm64/gtmux.app/Contents/MacOS/gtmux']:['dist/linux-unpacked/gtmux-desktop','dist/linux-arm64-unpacked/gtmux-desktop'];
for(const path of candidates)if(await access(path).then(()=>true,()=>false)){process.env.GTMUX_TEST_APP=resolve(path);break;}
if(!process.env.GTMUX_TEST_APP)throw new Error('No native unpacked package found.');
await import('./electron-smoke.mjs');
