// Build two isolated, unsigned Windows test installers. No publishing.
import { readFile, writeFile, mkdir, access } from 'node:fs/promises';
import { resolve, join } from 'node:path';
import { spawn } from 'node:child_process';
if(process.platform!=='win32')throw new Error('Build these fixtures with Windows Node.');
const base=JSON.parse(await readFile('package.json','utf8')).build;
const artifacts=resolve('../../.artifacts');await mkdir(artifacts,{recursive:true});
const serverResources=resolve(process.env.GTMUX_TEST_SERVER_RESOURCES??'resources');
await access(join(serverResources,'gtmux.exe'));
for(const version of ['0.1.0','0.1.1']){
  const config={...base,appId:'dev.gtmux.desktop.update-test',productName:'gtmux Update Test',
    extraMetadata:{name:'gtmux-update-test',version},
    directories:{output:`dist-update-test-${version}`},
    extraResources:[{from:serverResources,to:'server'},...base.extraResources.slice(1)],
    win:{...base.win,signExecutable:false},
    nsis:{...base.nsis,shortcutName:'gtmux Update Test',createDesktopShortcut:false,createStartMenuShortcut:true,runAfterFinish:true,artifactName:'gtmux-update-test-${version}.${ext}'},
    publish:{provider:'generic',url:'http://127.0.0.1:39241/'}};
  const path=join(artifacts,`windows-update-test-${version}.json`);await writeFile(path,JSON.stringify(config,null,2));
  await new Promise((done,reject)=>{
    const child=spawn(process.execPath,['node_modules/electron-builder/cli.js','--win','nsis','--config',path,'--publish','never'],{stdio:'inherit',env:{...process.env,CSC_IDENTITY_AUTO_DISCOVERY:'false'}});
    child.once('error',reject);child.once('exit',code=>code===0?done():reject(new Error(`Fixture build failed: ${code}`)));
  });
}
