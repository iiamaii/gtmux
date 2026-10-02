// Real browser and real hook CLI, isolated Store/config/ports. No agent API calls.
import { chromium } from 'playwright';
import { Supervisor } from '../src/supervisor.mjs';
import { mkdtemp, rm, mkdir } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createServer } from 'node:net';
import { spawn } from 'node:child_process';
import assert from 'node:assert/strict';
const root = await mkdtemp(join(tmpdir(),'gtmux-workspace-smoke-'));
const probe=createServer();await new Promise(r=>probe.listen(0,'127.0.0.1',r));const port=probe.address().port;await new Promise(r=>probe.close(r));
const supervisor = new Supervisor({root,binary:process.env.GTMUX_TEST_BINARY,frontend:process.env.GTMUX_TEST_FRONTEND});
let browser;
try {
  await supervisor.load();await supervisor.configure({port,workspace:root,mode:'web',background:false});await supervisor.start();
  const token=new URL(supervisor.openURL()).searchParams.get('token');
  const api=async(path,body)=>{const r=await fetch(`http://127.0.0.1:${port}/api/${path}`,{method:body?'POST':'GET',headers:{Authorization:`Bearer ${token}`,'Content-Type':'application/json'},body:body?JSON.stringify(body):undefined});assert(r.ok,`${path}: ${r.status}`);return r.status===204?null:r.json();};
  await api('sessions',{name:'smoke',workspace_root:root,confirm:true});
  await api('sessions/smoke/layout/ops',{ops:[{op:'spawn'}]});
  const terminal=(await api('terminals'))[0];assert(terminal?.id);
  async function hook(agent,event,expected) {
    const env={...process.env,XDG_STATE_HOME:join(root,'state'),XDG_CONFIG_HOME:join(root,'config'),XDG_DATA_HOME:join(root,'data'),GTMUX_TERMINAL_ID:terminal.id,GTMUX_SERVER_INSTANCE:'desktop',GTMUX_SERVER_URL:`http://127.0.0.1:${port}`};
    const child=spawn(supervisor.binary,['agent','event',agent],{env,stdio:['pipe','pipe','pipe']});
    let out='',err='';child.stdout.on('data',x=>out+=x);child.stderr.on('data',x=>err+=x);
    child.stdin.end(JSON.stringify(event));const code=await new Promise(r=>child.on('exit',r));assert.equal(code,0,err);assert.equal(err.trim(),'');assert.equal(out.trim(),'{}');
    const row=(await api('terminals/activity')).terminals.find(x=>x.id===terminal.id);assert.equal(row.activity.state,expected);
  }
  await hook('claude',{hook_event_name:'UserPromptSubmit'},'working');
  await hook('claude',{hook_event_name:'PermissionRequest'},'needs_input');
  await hook('codex',{hook_event_name:'Stop'},'completed');
  await hook('codex',{hook_event_name:'SubagentStop',agent_id:'child'},'completed');
  // Exercise environment injection through a real spawned shell, not only a
  // simulated hook process. The test owns this terminal and its temporary Store.
  const executable = "'" + supervisor.binary.replaceAll("'", "'\\''") + "'";
  const command = `printf '{"hook_event_name":"Stop"}' | ${executable} agent event claude; printf '\\nHOOK_ENDPOINT=%s\\n' "$GTMUX_SERVER_URL"\n`;
  await api(`terminals/${terminal.id}/input`,{bytes_base64:Buffer.from(command).toString('base64')});
  for(let attempt=0;attempt<50;attempt++) {
    const output=Buffer.from((await api(`terminals/${terminal.id}/output`)).bytes_base64,'base64').toString();
    if(output.includes(`HOOK_ENDPOINT=http://127.0.0.1:${port}`))break;
    assert(attempt<49,'spawned shell must inherit the actual manager endpoint');
    await new Promise(r=>setTimeout(r,100));
  }
  assert.equal((await api('terminals/activity')).terminals.find(x=>x.id===terminal.id).activity.state,'completed');
  browser=await chromium.launch({headless:true});const page=await browser.newPage({viewport:{width:1440,height:1000}});
  await page.goto(supervisor.openURL());
  await page.getByRole('button',{name:'Open existing',exact:false}).click();
  await page.locator('.session-name').filter({hasText:/^smoke$/}).click();
  const panel=page.getByLabel('Left panel',{exact:true});await panel.waitFor();
  const width=async expected=>page.waitForFunction(w=>Math.abs((document.querySelector('[aria-label="Left panel"]')?.getBoundingClientRect().width ?? 0)-w)<2,expected);
  const settings=async()=>{await page.getByRole('button',{name:'Session menu',exact:true}).click();await page.getByRole('button',{name:'Settings…',exact:true}).click();await page.getByRole('button',{name:'Appearance',exact:true}).click();};
  await width(268);await settings();
  await page.getByRole('switch',{name:/^Terminal activity/}).check();
  assert.equal(await page.getByRole('switch',{name:/^Unread in browser tab/}).isChecked(),false);
  await page.getByRole('button',{name:'Close settings',exact:true}).click();await width(340);
  const handle=page.getByRole('button',{name:'Resize left panel',exact:true});const box=await handle.boundingBox();const p=await panel.boundingBox();
  await page.mouse.move(box.x+2,box.y+30);await page.mouse.down();await page.mouse.move(p.x+410,box.y+30);await page.mouse.up();await width(410);
  await handle.focus();await page.keyboard.press('ArrowRight');await width(420);
  await page.reload();await width(420);
  await settings();await page.getByRole('switch',{name:/^Activity list/}).uncheck();await page.getByRole('button',{name:'Close settings',exact:true}).click();await width(268);
  await settings();await page.getByRole('switch',{name:/^Activity list/}).check();await page.getByRole('button',{name:'Close settings',exact:true}).click();await width(420);
  await page.getByRole('tab',{name:'Activity',exact:true}).click();
  await page.getByLabel('Terminal activity',{exact:true}).getByText('Completed',{exact:true}).waitFor();
  await mkdir(process.env.GTMUX_SCREENSHOTS ?? root,{recursive:true});
  for(const theme of ['light','dark']) {
    await settings();
    await page.getByRole('button',{name:theme==='dark'?'Dark':'Light',exact:true}).click();
    await page.getByRole('button',{name:'Close settings',exact:true}).click();
    await page.waitForFunction(t=>localStorage.getItem('gtmux-theme')===t,theme);
    await page.waitForFunction(t => getComputedStyle(document.querySelector('.panel-tab.active')).color === (t === 'dark' ? 'rgb(245, 245, 245)' : 'rgb(0, 0, 0)'), theme);
    await page.screenshot({animations:'disabled',path:join(process.env.GTMUX_SCREENSHOTS ?? root,`workspace-${theme}.png`)});
  }
  console.log('Real Claude/Codex hook payloads reach terminal activity; sidebar defaults, drag, keyboard resize, reload and list toggle preserve separate widths.');
} finally {await browser?.close();await supervisor.stop();await rm(root,{recursive:true,force:true});}
