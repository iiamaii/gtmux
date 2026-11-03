// Downloads the pinned Caddy binary, issues only a private local certificate.
// Never contacts a public ACME CA, edits trust stores or exposes a public listener.
import assert from 'node:assert/strict';
import { mkdtemp, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createServer } from 'node:http';
import { get } from 'node:https';
import { checkServerIdentity } from 'node:tls';
import { ProxyManager, proxyConfiguration } from '../src/proxy.mjs';
import { checkHTTPS } from '../src/https.mjs';
const root = await mkdtemp(join(tmpdir(),'gtmux-local-tls-'));
const backend = createServer((_req,res) => { res.setHeader('x-gtmux-server-id','local-tls-test'); res.end('ok'); });
await new Promise(r => backend.listen(0,'127.0.0.1',r)); const port = backend.address().port;
async function freePort() { const s = createServer(); await new Promise(r => s.listen(0,'127.0.0.1',r)); const p = s.address().port; await new Promise(r => s.close(r)); return p; }
const httpsPort = await freePort(), httpPort = await freePort();
const origin = `https://127.0.0.1:${httpsPort}`;
const proxy = new ProxyManager({root,instanceId:'local-tls-test',serverPath:async p=>p,config:async()=>({public_origin:origin,server:{port}})});
try {
  await proxy.install();
  const config = proxyConfiguration('127.0.0.1',port,httpPort,httpsPort,'local',httpsPort);
  const adminPort = await freePort();
  config.admin = process.platform === 'win32' ? {listen:`127.0.0.1:${adminPort}`, enforce_origin:true, origins:[`127.0.0.1:${adminPort}`]} : {listen:'unix/'+join(proxy.root,'admin.sock')};
  config.apps.http.servers.gtmux.listen = [`127.0.0.1:${httpsPort}`];
  config.apps.http.servers.gtmux.automatic_https = {disable_redirects:true};
  await writeFile(join(proxy.root,'caddy.json'),JSON.stringify(config));
  await writeFile(join(proxy.root,'exposure.json'),JSON.stringify({domain:'127.0.0.1',origin,mode:'managed',certificate:'local'}));
  await proxy.start();
  let result;
  for(let i=0;i<30;i++){ try {result = await proxy.verify();break;} catch(e){if(i===29)throw e;await new Promise(r=>setTimeout(r,200));} }
  assert.equal(result.verified,true); assert(result.certificate.fingerprint256); assert(result.certificate.daysRemaining>=0);
  await assert.rejects(checkHTTPS(origin,'local-tls-test'),/certificate|issuer|self.signed|verify/i);
  const rootCert = await proxy.rootCertificate(); assert(rootCert.pem.includes('BEGIN CERTIFICATE'));
  await assert.rejects(checkHTTPS(origin,'another-server',rootCert.pem),/not this running/);
  await proxy.stop();
  // Simulate NAT: certificate/HTTP host differs from the listener's local IP,
  // and an IP client sends no SNI. No public address is contacted.
  const natHost='192.0.2.10', natOrigin=`https://${natHost}:${httpsPort}`;
  const natConfig=proxyConfiguration(natHost,port,httpPort,httpsPort,'local',httpsPort);
  natConfig.admin=config.admin;natConfig.apps.http.servers.gtmux.listen=[`127.0.0.1:${httpsPort}`];
  natConfig.apps.http.servers.gtmux.automatic_https={disable_redirects:true};
  await writeFile(join(proxy.root,'caddy.json'),JSON.stringify(natConfig));
  await writeFile(join(proxy.root,'exposure.json'),JSON.stringify({domain:natHost,origin:natOrigin,mode:'managed',certificate:'local'}));
  proxy.server.config=async()=>({public_origin:natOrigin,server:{port}});await proxy.start();
  const probeNAT=()=>new Promise((resolve,reject)=>{
    const request=get({hostname:'127.0.0.1',port:httpsPort,path:'/healthz',servername:'',ca:rootCert.pem,headers:{Host:`${natHost}:${httpsPort}`},checkServerIdentity:(_host,cert)=>checkServerIdentity(natHost,cert),timeout:3000},res=>{res.resume();try{assert.equal(res.statusCode,200);assert.equal(res.headers['x-gtmux-server-id'],'local-tls-test');resolve();}catch(e){reject(e);}});
    request.on('error',reject);request.on('timeout',()=>request.destroy(new Error('NAT probe timeout')));
  });
  for(let i=0;i<20;i++){try{await probeNAT();break;}catch(e){if(i===19)throw e;await new Promise(r=>setTimeout(r,200));}}
  await proxy.stop();
  // Validate the exact pinned binary's public-IP ACME schema without running it.
  const publicConfig = proxyConfiguration('8.8.8.8',port,httpPort,httpsPort);
  await writeFile(join(proxy.root,'public-schema.json'),JSON.stringify(publicConfig));
  await proxy.run(proxy.binary,['validate','--config',join(proxy.root,'public-schema.json')],{env:{...process.env,XDG_DATA_HOME:join(root,'schema-data'),XDG_CONFIG_HOME:join(root,'schema-config')},timeout:15000});
  console.log('Local CA issuance, pinned trust, expiry, wrong-server rejection, owned stop, IP routing without SNI behind NAT and public-IP schema validation passed. No public ACME issuance or trust-store change.');
} finally {await proxy.stop();await new Promise(r=>backend.close(r));await rm(root,{recursive:true,force:true});}
