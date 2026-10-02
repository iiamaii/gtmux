import { copyFile, mkdir, stat, chmod, rename } from 'node:fs/promises';
import { join, dirname, basename } from 'node:path';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { atomicWrite, readOptional } from './files.mjs';
const exec = promisify(execFile);
export async function agentConfiguration(supervisor, agent) {
  if (!['claude','codex','gemini','copilot','cursor','aider','opencode'].includes(agent)) throw new Error('Choose a supported agent.');
  // AppImage mount paths are temporary. Hooks need a persistent executable path.
  const root = join(supervisor.root,'agent-integration'); await mkdir(root,{recursive:true,mode:0o700});
  const binary = join(root,basename(supervisor.binary));
  const source = await stat(supervisor.binary); const stamp = `${supervisor.binary}:${source.size}:${source.mtimeMs}`;
  if (await readOptional(join(root,'source')) !== stamp) {
    for (const name of ['libwinpthread-1.dll','libgcc_s_seh-1.dll']) {
      const dll = join(dirname(supervisor.binary),name);
      if (await stat(dll).then(()=>true,e=>{if(e.code==='ENOENT')return false;throw e;})) await copyFile(dll,join(root,name));
    }
    await copyFile(supervisor.binary,binary+'.new'); await chmod(binary+'.new',0o700); await rename(binary+'.new',binary);
    await atomicWrite(join(root,'source'),stamp);
  }
  const { stdout } = await exec(binary,['agent','hooks',agent],{timeout:5000,maxBuffer:131072,windowsHide:true});
  return { configuration: stdout };
}
