// Real electron-updater metadata/download/hash pipeline against a loopback feed.
// Uses Node networking instead of Electron net; never installs or executes files.
import { createRequire } from 'node:module';
import { createServer } from 'node:http';
import { mkdtemp, writeFile, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createHash } from 'node:crypto';
import assert from 'node:assert/strict';
import { Updates } from '../src/updates.mjs';
const require=createRequire(import.meta.url);
const {AppImageUpdater}=require('electron-updater');
const {NodeHttpExecutor}=require('builder-util/out/nodeHttpExecutor');
const {ElectronHttpExecutor}=require('electron-updater/out/electronHttpExecutor');
const root=await mkdtemp(join(tmpdir(),'gtmux-update-feed-'));
const original=process.env.APPIMAGE;process.env.APPIMAGE=join(root,'old.AppImage');
const payload=Buffer.from('Not executable. gtmux isolated updater fixture.\n');
const digest=createHash('sha512').update(payload).digest('base64');let corrupt=false;
const server=createServer((req,res)=>{if(req.url.split('?')[0].endsWith('.yml')) {res.end(`version: 0.2.0\nfiles:\n  - url: fixture.AppImage\n    sha512: ${digest}\n    size: ${payload.length}\npath: fixture.AppImage\nsha512: ${digest}\nreleaseDate: 2026-01-01T00:00:00.000Z\n`);}else{const data=corrupt?Buffer.from('bad contents'):payload;res.setHeader('Content-Length',data.length);res.end(data);}});
try {
  await new Promise(r=>server.listen(0,'127.0.0.1',r));
  const url=`http://127.0.0.1:${server.address().port}`;
  await writeFile(join(root,'config.yml'),`provider: generic\nurl: ${url}\nupdaterCacheDirName: isolated-updater\n`);
  for(const bad of [false,true]) {
    corrupt=bad;const app={version:'0.1.0',name:'gtmux-test',isPackaged:true,userDataPath:root,baseCachePath:join(root,String(bad)),appUpdateConfigPath:join(root,'config.yml'),whenReady:async()=>{},onQuit:()=>{throw new Error('Must not install on exit');}};
    const sdk=new AppImageUpdater(null,app);
    const executor=new NodeHttpExecutor();executor.download=ElectronHttpExecutor.prototype.download;
    sdk.httpExecutor=executor;sdk.setFeedURL({provider:'generic',url});sdk.disableDifferentialDownload=true;sdk.logger={info(){},warn(){},error(){},debug(){}};
    const updates=new Updates({root,updater:sdk,stopForInstall:()=>{throw new Error('No installation in this test');}});
    if(bad){await assert.rejects(updates.check(true),/checksum|sha512|mismatch/i);assert.equal(updates.phase,'error');await assert.rejects(updates.install({confirmed:true}),/Download and verify/);}
    else{await updates.check(true);assert.equal(updates.phase,'ready');assert.deepEqual(await readFile(sdk.installerPath),payload);}
  }
  console.log('Actual updater feed lookup, download, SHA512 success/corruption rejection passed; no installation performed.');
} finally {if(original===undefined)delete process.env.APPIMAGE;else process.env.APPIMAGE=original;await new Promise(r=>server.close(r));await rm(root,{recursive:true,force:true});}
