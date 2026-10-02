import test from 'node:test';
import { request as httpRequest } from 'node:http';
import assert from 'node:assert/strict';
import { controlServer } from '../src/control.mjs';
test('management needs bootstrap cookie, exact Host, Origin and JSON; remote API cannot install', async t => {
  let installed = 0;
  const control = await controlServer({ supervisor: { status: () => ({ state: 'stopped' }) },
    proxy: { status: async () => ({}), install: async () => { installed++; return {}; } } });
  t.after(() => control.close());
  assert.equal((await fetch(control.origin + '/api/status')).status, 401);
  const bootstrap = await fetch(control.url, { redirect: 'manual' }); assert.equal(bootstrap.status, 303);
  const cookie = bootstrap.headers.get('set-cookie').split(';')[0];
  assert.equal((await fetch(control.origin + '/api/status', { headers: { Cookie: cookie } })).status, 200);
  for (const origin of ['https://evil.example', undefined]) {
    const headers = { Cookie: cookie, 'Content-Type': 'application/json' }; if (origin) headers.Origin = origin;
    assert.equal((await fetch(control.origin + '/api/proxy/install', { method: 'POST', headers, body: '{}' })).status, 403);
  }
  assert.equal(installed, 0);
  assert.equal((await fetch(control.origin + '/api/proxy/install', { method: 'POST', headers: { Cookie: cookie, Origin: control.origin, 'Content-Type': 'application/json' }, body: '{}' })).status, 200);
  assert.equal(installed, 1);
  const badHostStatus = await new Promise((resolve, reject) => { const req = httpRequest(control.origin + '/api/status', { headers: { Cookie: cookie, Host: 'evil.example' } }, res => { res.resume(); resolve(res.statusCode); }); req.on('error', reject); req.end(); });
  assert.equal(badHostStatus, 403);
});

test('a rejected server stop preserves the public proxy and manager profiles use distinct cookies', async t => {
  let stopped = 0;
  const make = () => controlServer({ supervisor: { status: () => ({}), stop: async () => { throw new Error('Shutdown rejected'); } },
    proxy: { status: async () => ({}), stop: async () => { stopped++; } } });
  const first = await make(), second = await make();
  t.after(async () => { await first.close(); await second.close(); });
  const bootstrap = async control => (await fetch(control.url, { redirect: 'manual' })).headers.get('set-cookie').split(';')[0];
  const cookie = await bootstrap(first), other = await bootstrap(second);
  assert.notEqual(cookie.split('=')[0], other.split('=')[0]);
  const response = await fetch(first.origin + '/api/stop', { method: 'POST',
    headers: { Cookie: cookie, Origin: first.origin, 'Content-Type': 'application/json' }, body: JSON.stringify({ confirmed: true, credential: 'wrong' }) });
  assert.equal(response.status, 400); assert.equal(stopped, 0);
});

test('open starts the stopped server and managed proxy before opening; startup failure never opens', async t => {
  const calls=[]; let fail=false;
  const control=await controlServer({
    supervisor:{preferences:{mode:'app'},start:async()=>{calls.push('start');if(fail)throw new Error('Port is occupied');}},
    proxy:{status:async()=>({configuration:{mode:'managed'}}),start:async()=>calls.push('proxy'),workspaceURL:async()=>{calls.push('url');return 'http://127.0.0.1:9999/';}},
    onOpen:async(url,mode)=>{assert.equal(mode,'app');calls.push('open');}
  });
  t.after(()=>control.close());
  assert.deepEqual(await control.openWorkspace(),{opened:true});
  assert.deepEqual(calls,['start','proxy','url','open']);
  calls.length=0; fail=true;
  await assert.rejects(control.openWorkspace(),/occupied/); assert.deepEqual(calls,['start']);
  calls.length=0; fail=false;
  await control.openWorkspace(); assert.deepEqual(calls,['start','proxy','url','open']);
});
