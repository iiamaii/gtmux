import test from 'node:test';
import assert from 'node:assert/strict';
import vm from 'node:vm';
import { readFile } from 'node:fs/promises';
import { EventEmitter } from 'node:events';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
const require = createRequire(import.meta.url);
const source = await readFile(new URL('../src/electron.cjs', import.meta.url), 'utf8');
function harness() {
  const app = new EventEmitter();
  Object.assign(app, { requestSingleInstanceLock: () => true, whenReady: () => new Promise(() => {}) });
  class Window extends EventEmitter {
    destroyed = false;
    shows = 0;
    webContents = Object.assign(new EventEmitter(), {
      setWindowOpenHandler() {}, session: { setPermissionRequestHandler() {} },
    });
    isDestroyed() { return this.destroyed; }
    show() { if (this.destroyed) throw new Error('Object has been destroyed'); this.shows++; }
    focus() { if (this.destroyed) throw new Error('Object has been destroyed'); }
    isMinimized() { return false; }
    loads = 0;
    async loadURL() { this.loads++; }
    close() { this.destroyed = true; this.emit('closed'); }
  }
  const context = vm.createContext({ __dirname: fileURLToPath(new URL('../src/', import.meta.url)), require: name => name === 'electron' ? { app, BrowserWindow: Window } : require(name), process: { env: {} }, URL });
  vm.runInContext(source + '\nglobalThis.state = { openWorkspace, setManager: w => managerWindow = w, workspace: () => workspaceWindow, quitting: () => quitting = true, pending: () => quitPending = true };', context);
  return { ...context.state, Window, app };
}
test('closing a workspace after its manager is destroyed is safe', async () => {
  const h = harness(), manager = new h.Window(); h.setManager(manager);
  await h.openWorkspace('http://localhost:1234', 'app');
  manager.close();
  assert.doesNotThrow(() => h.workspace().close());
  assert.doesNotThrow(() => h.app.emit('activate'));
  assert.doesNotThrow(() => h.app.emit('second-instance'));
});
test('ordinary workspace close reveals controls, but shutdown never does', async () => {
  for (const state of ['normal', 'pending', 'quitting']) {
    const h = harness(), manager = new h.Window(); h.setManager(manager);
    await h.openWorkspace('http://localhost:1234', 'app');
    if (state !== 'normal') h[state]();
    h.workspace().close();
    assert.equal(manager.shows, state === 'normal' ? 1 : 0);
  }
});

test('new windows coexist and focusing an existing window preserves its session', async () => {
  const h = harness(), manager = new h.Window(); h.setManager(manager);
  await h.openWorkspace('http://localhost:1234/?token=a','app'); const first=h.workspace();
  await h.openWorkspace('http://localhost:1234/?token=a','app',{newWindow:true}); const second=h.workspace();
  assert.notEqual(first,second); assert.equal(first.destroyed,false);
  await h.openWorkspace('http://localhost:1234/?token=a','app'); assert.equal(second.loads,1);
  second.close(); assert.equal(first.destroyed,false); assert.equal(manager.shows,0);
  await h.openWorkspace('http://localhost:1234/?token=b','app'); assert.equal(h.workspace(),first); assert.equal(first.loads,2);
  first.close(); assert.equal(manager.shows,1);
});
