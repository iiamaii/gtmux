import test from 'node:test';
import assert from 'node:assert/strict';
import { EventEmitter } from 'node:events';
import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { Updates } from '../src/updates.mjs';
async function fixture(t) {
  const root = await mkdtemp(join(tmpdir(),'gtmux-updates-')); t.after(() => rm(root,{recursive:true,force:true}));
  const updater = new EventEmitter(); let downloaded = 0, installed = 0, stopped = 0;
  updater.checkForUpdates = async () => { updater.emit('update-available'); return { updateInfo: { version: '0.2.0' } }; };
  updater.downloadUpdate = async () => { downloaded++; updater.emit('download-progress',{percent:100}); };
  updater.quitAndInstall = () => { installed++; };
  const updates = new Updates({ root, updater, stopForInstall: async password => { if (password !== 'correct') throw new Error('Shutdown rejected'); stopped++; } });
  return { root, updater, updates, counts: () => ({downloaded,installed,stopped}) };
}
test('updates download separately and cannot interrupt sessions without confirmed shutdown', async t => {
  const f = await fixture(t); await f.updates.load();
  assert.equal(f.updater.autoInstallOnAppQuit,false);
  await f.updates.check(); assert.equal(f.updates.phase,'available'); assert.equal(f.counts().downloaded,0);
  await f.updates.check(true); assert.equal(f.updates.phase,'ready');
  await assert.rejects(f.updates.install({confirmed:false}),/Confirm/);
  await assert.rejects(f.updates.install({confirmed:true,credential:'wrong'}),/Shutdown rejected/);
  assert.equal(f.counts().installed,0); assert.equal(f.updates.phase,'ready');
  await f.updates.install({confirmed:true,credential:'correct'});
  assert.deepEqual(f.counts(),{downloaded:1,installed:1,stopped:1});
});
test('current releases do not download and automatic preference survives reopening', async t => {
  const f = await fixture(t); await f.updates.preferences({automatic:true});
  const reopened = new Updates({root:f.root,updater:f.updater}); await reopened.load(); assert.equal(reopened.automatic,true);
  f.updater.checkForUpdates = async () => { f.updater.emit('update-not-available'); return {updateInfo:{version:'0.1.0'}}; };
  await reopened.check(true); assert.equal(reopened.phase,'current'); assert.equal(f.counts().downloaded,0);
});
test('failed verification never permits installation and unconfigured packages explain why', async t => {
  const f = await fixture(t); f.updater.downloadUpdate = async () => { throw new Error('SHA512 mismatch'); };
  await assert.rejects(f.updates.check(true),/SHA512/);
  await assert.rejects(f.updates.install({confirmed:true}),/Download and verify/);
  const unavailable = new Updates({root:f.root,supported:false,reason:'No feed'});
  await assert.rejects(unavailable.check(),/No feed/);
  await assert.rejects(unavailable.preferences({automatic:true}),/No feed/);
});

test('an installer error restores application controls instead of leaving shutdown latched', async t => {
  const f = await fixture(t); let recovered = 0;
  f.updates.installFailed = () => recovered++;
  await f.updates.check(true);
  f.updater.quitAndInstall = () => f.updater.emit('error',new Error('Installer could not launch'));
  await f.updates.install({confirmed:true,credential:'correct'});
  assert.equal(recovered,1); assert.equal(f.updates.phase,'error'); assert.match(f.updates.error,/Installer/);
});
