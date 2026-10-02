import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, readFile, writeFile, mkdir, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { AgentInstall, mergeHooks } from '../src/agent-install.mjs';
const generated=JSON.stringify({hooks:{Stop:[{hooks:[{type:'command',command:'gtmux agent event claude'}]}]}});
test('hook installation preserves other settings/hooks and refuses stale confirmation',async t=>{
 const root=await mkdtemp(join(tmpdir(),'gtmux-hook-install-'));t.after(()=>rm(root,{recursive:true,force:true}));
 const target=join(root,'.claude','settings.json');await mkdir(join(root,'.claude'));
 const original=JSON.stringify({permissions:{deny:['Bash(rm *)']},hooks:{Stop:[{hooks:[{type:'command',command:'my-check'}]}]}});
 await writeFile(target,original);
 const installer=new AgentInstall({root,preferences:{workspace:root}},{home:root,generate:async()=>({configuration:generated})});
 let plan=await installer.preview('claude');assert.equal(await readFile(target,'utf8'),original);
 await assert.rejects(installer.apply({planId:plan.id}),/confirm/);
 await writeFile(target,original+' ');await assert.rejects(installer.apply({planId:plan.id,confirmed:true}),/changed/);
 await writeFile(target,original);plan=await installer.preview('claude');await installer.apply({planId:plan.id,confirmed:true});
 let saved=JSON.parse(await readFile(target,'utf8'));assert.equal(saved.hooks.Stop.length,2);assert.deepEqual(saved.permissions,{deny:['Bash(rm *)']});
 plan=await installer.preview('claude');await installer.apply({planId:plan.id,confirmed:true});
 saved=JSON.parse(await readFile(target,'utf8'));assert.equal(saved.hooks.Stop.length,2);
 plan=await installer.preview('claude',true);await installer.apply({planId:plan.id,confirmed:true});assert.equal(await readFile(target,'utf8'),original);
});
test('aider comments and custom notifications are protected',()=>{
 const extra=JSON.stringify({notifications:true,'notifications-command':'gtmux agent event aider --event completed'});
 const merged=mergeHooks('aider','# keep\nmodel: test\n',extra);assert(merged.includes('# keep'));assert(merged.includes('model: test'));
 assert.throws(()=>mergeHooks('aider','notifications-command: my-notifier\n',extra),/already/);
 assert.throws(()=>mergeHooks('opencode','my existing plugin','new plugin'),/already exists/);
});
