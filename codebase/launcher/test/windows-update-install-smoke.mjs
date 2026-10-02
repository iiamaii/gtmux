// Opt-in real Windows installer test. Requires separate test-app identity and two
// NSIS builds; never runs against a normal gtmux installation or user profile.
import { _electron as electron } from 'playwright';
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { createServer as createProbe } from 'node:net';
import { createReadStream } from 'node:fs';
import { access, mkdir, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import { join, resolve, basename } from 'node:path';
import { execFile, spawn } from 'node:child_process';
import { promisify } from 'node:util';
const exec=promisify(execFile);
if(process.platform!=='win32')throw new Error('Windows only.');
const repo=resolve('../..'), artifacts=join(repo,'.artifacts');
const installer=resolve('dist-update-test-0.1.0/gtmux-update-test-0.1.0.exe');
const feedDir=resolve('dist-update-test-0.1.1');
const profile=join(process.env.APPDATA,'gtmux-update-test');
const cache=join(process.env.LOCALAPPDATA,'gtmux-update-test-updater');
for(const path of [profile,cache])assert.equal(await access(path).then(()=>true,()=>false),false,`Refusing existing test data: ${path}`);
const testRoot=join(artifacts,`update-install-${Date.now()}`), installDir=join(testRoot,'gtmux Update Test');
await mkdir(testRoot,{recursive:true});
const executable=join(installDir,'gtmux Update Test.exe');
const env={...process.env,GTMUX_TEST_INSTALL_PATH:executable};
delete env.ELECTRON_RUN_AS_NODE;delete env.GTMUX_DESKTOP_DATA_DIR;
const powershell=join(process.env.SystemRoot,'System32','WindowsPowerShell','v1.0','powershell.exe');
const ps=async command=>(await exec(powershell,['-NoProfile','-NonInteractive','-Command',command],{env,timeout:30000})).stdout.trim();
let app,page,server,installed=false,ownsProfile=false,ui;
const requested=[];
try {
  server=createServer(async(req,res)=>{
    const name=basename(new URL(req.url,'http://localhost').pathname);
    if(!['latest.yml','gtmux-update-test-0.1.1.exe','gtmux-update-test-0.1.1.exe.blockmap'].includes(name)){res.writeHead(404);return res.end();}
    requested.push(name);createReadStream(join(feedDir,name)).on('error',()=>{res.writeHead(404);res.end();}).pipe(res);
  });
  await new Promise((resolve,reject)=>{server.once('error',reject);server.listen(39241,'127.0.0.1',resolve);});
  await exec(installer,['/S','/currentuser',`/D=${installDir}`],{env,timeout:120000});installed=true;
  await access(executable);
  await access(join(process.env.APPDATA,'Microsoft','Windows','Start Menu','Programs','gtmux Update Test.lnk'));
  console.log('Isolated 0.1.0 installation and Start-menu registration complete.');
  const probe=createProbe();await new Promise(r=>probe.listen(0,'127.0.0.1',r));const port=probe.address().port;await new Promise(r=>probe.close(r));
  app=await electron.launch({executablePath:executable,env,timeout:30000});
  assert.equal(await app.evaluate(({app})=>app.getVersion()),'0.1.0');
  assert.equal(await app.evaluate(({app})=>app.getPath('userData')),profile);ownsProfile=true;
  const originalPid=app.process().pid;
  page=await app.firstWindow();await page.waitForLoadState('domcontentloaded');
  await page.locator('[name=workspace]').fill(testRoot);
  await page.locator('[name=port]').fill(String(port));
  await page.locator('#setup [name=mode]').selectOption('app');
  await page.locator('#setup-save').click();
  await page.locator('#setup-status[data-state=saved]').waitFor();
  await page.locator('#start').click();await page.locator('#state').filter({hasText:/^running$/}).waitFor({timeout:30000});
  const serverPid=await page.evaluate(async()=> (await (await fetch('/api/status')).json()).pid);
  const configPath=join(profile,'managed-server','server.toml');const config=await readFile(configPath,'utf8');
  const marker=join(profile,'managed-server','store','update-test-marker');await mkdir(join(profile,'managed-server','store'),{recursive:true});await writeFile(marker,'preserve-store');
  await page.getByRole('button',{name:'Updates',exact:true}).click();
  await page.locator('#update-check').click();
  await page.waitForFunction(()=>document.querySelector('#update-status').textContent.startsWith('available'),{},{timeout:60000});
  await page.locator('#update-download').click();
  await page.waitForFunction(()=>document.querySelector('#update-status').textContent.startsWith('ready'),{},{timeout:180000});
  console.log('0.1.1 discovered, downloaded and verified by the real updater.');
  ui=spawn(powershell,['-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',resolve('test/complete-update-test-installer.ps1')],{env,stdio:['ignore','pipe','pipe']});
  let uiOutput='';ui.stdout.on('data',chunk=>{uiOutput+=chunk;process.stdout.write(chunk);});ui.stderr.on('data',chunk=>{uiOutput+=chunk;process.stderr.write(chunk);});
  const uiDone=new Promise((resolve,reject)=>{ui.once('error',reject);ui.once('exit',code=>code===0?resolve():reject(new Error(uiOutput)));});uiDone.catch(()=>{});
  const exit=app.waitForEvent('close',{timeout:180000}); exit.catch(()=>{});
  await page.locator('#update-install').click();await page.locator('#confirm-stop').click();
  await uiDone;
  await exit;app=null;
  let running;
  for(let attempt=0;attempt<40;attempt++){
    const result=await ps("Get-Process | Where-Object { $_.Path -eq $env:GTMUX_TEST_INSTALL_PATH -and $_.MainWindowHandle -ne 0 } | Select-Object -First 1 Id,Responding,@{n='Version';e={$_.MainModule.FileVersionInfo.ProductVersion}} | ConvertTo-Json -Compress");
    if(result){running=JSON.parse(result);if(running.Responding&&running.Version.startsWith('0.1.1'))break;}
    await new Promise(r=>setTimeout(r,500));
  }
  assert(running?.Responding);assert(running.Version.startsWith('0.1.1'));assert.notEqual(running.Id,originalPid);
  assert.equal(await ps(`Get-Process | Where-Object Id -eq ${serverPid} | Select-Object -ExpandProperty Id`),'','owned server stopped before installation');
  assert.equal(await readFile(configPath,'utf8'),config);assert.equal(await readFile(marker,'utf8'),'preserve-store');
  assert(requested.includes('latest.yml'));assert(requested.includes('gtmux-update-test-0.1.1.exe'));
  await writeFile(join(artifacts,'windows-update-install-result.json'),JSON.stringify({from:'0.1.0',to:running.Version,restarted:true,serverStopped:true,configPreserved:true,storeMarkerPreserved:true,feed:'loopback',requested},null,2));
  console.log('PASS: installed 0.1.0 → downloaded 0.1.1 → stopped owned server → installed → restarted 0.1.1; configuration and Store marker preserved.');
} finally {
  if(ui&&ui.exitCode===null)ui.kill();
  if(app&&page&&!page.isClosed())await page.evaluate(async()=>{await fetch('/api/stop',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({confirmed:true})});}).catch(()=>{});
  await app?.close();
  if(installed){
    await ps("$p=Get-Process | Where-Object { $_.Path -eq $env:GTMUX_TEST_INSTALL_PATH -and $_.MainWindowHandle -ne 0 }; foreach($item in $p){[void]$item.CloseMainWindow(); if(-not $item.WaitForExit(15000)){throw 'Test app did not close normally'}}");
    const uninstaller=(await readdir(installDir)).find(name=>/^Uninstall.*\.exe$/i.test(name));
    if(uninstaller)await exec(join(installDir,uninstaller),['/S','/currentuser'],{env,timeout:60000});
  }
  if(server)await new Promise(r=>server.close(r));
  if(ownsProfile){await rm(profile,{recursive:true,force:true});await rm(cache,{recursive:true,force:true});}
}
