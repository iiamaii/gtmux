import { homedir } from 'node:os';
import { join, isAbsolute } from 'node:path';
import { lstat } from 'node:fs/promises';
import { randomUUID } from 'node:crypto';
import { parseDocument } from 'yaml';
import { atomicWrite, readOptional, revision } from './files.mjs';
import { agentConfiguration } from './agents.mjs';
export function mergeHooks(agent, before, generated) {
  if (agent === 'opencode') {
    if (before !== null && before !== generated) throw new Error('The OpenCode plugin path already exists. Preserve it and install this integration under another filename manually.');
    return generated;
  }
  const addition = JSON.parse(generated);
  if (agent === 'aider') {
    const doc = parseDocument(before ?? '{}'); if (doc.errors.length) throw new Error('Existing Aider YAML is invalid; no changes made.');
    const existing = doc.toJS(); if (!existing || Array.isArray(existing) || typeof existing !== 'object') throw new Error('Aider settings must be a mapping.');
    if (existing['notifications-command'] && existing['notifications-command'] !== addition['notifications-command']) throw new Error('Aider already has a notification command. Chain it manually rather than replacing it.');
    for (const [key,value] of Object.entries(addition)) doc.set(key,value);
    return doc.toString();
  }
  let current; try { current = JSON.parse(before ?? '{}'); } catch { throw new Error('Existing settings are not plain valid JSON. Merge the generated hooks manually to preserve comments.'); }
  if (!current || Array.isArray(current) || typeof current !== 'object') throw new Error('Agent settings must be an object.');
  if (current.hooks !== undefined && (!current.hooks || Array.isArray(current.hooks) || typeof current.hooks !== 'object')) throw new Error('Existing hooks are not an event mapping.');
  current.hooks ??= {};
  for (const [event,entries] of Object.entries(addition.hooks)) {
    if (current.hooks[event] !== undefined && !Array.isArray(current.hooks[event])) throw new Error('Existing event hooks must be an array.');
    current.hooks[event] ??= [];
    for (const entry of entries) if (!current.hooks[event].some(v=>JSON.stringify(v)===JSON.stringify(entry))) current.hooks[event].push(entry);
  }
  if (addition.version !== undefined) {
    if (current.version !== undefined && current.version !== addition.version) throw new Error('Existing hook configuration uses another schema version.');
    current.version = addition.version;
  }
  return JSON.stringify(current,null,2)+'\n';
}
async function regular(path) {
  const stat = await lstat(path).catch(e=>{if(e.code==='ENOENT')return null;throw e;});
  if (stat && (!stat.isFile() || stat.isSymbolicLink())) throw new Error('The hook target must be a regular file, not a symlink.');
}
export class AgentInstall {
  constructor(supervisor,{home=homedir(),generate=agentConfiguration}={}) {this.server=supervisor;this.home=home;this.generate=generate;this.plan=null;}
  target(agent) {
    const home=this.home,workspace=this.server.preferences?.workspace;
    const names={claude:join(home,'.claude','settings.json'),codex:join(home,'.codex','hooks.json'),gemini:join(home,'.gemini','settings.json'),cursor:join(home,'.cursor','hooks.json'),aider:join(home,'.aider.conf.yml')};
    if (names[agent]) return names[agent];
    if (!workspace || !isAbsolute(workspace)) throw new Error('Choose a workspace first.');
    if (agent==='copilot')return join(workspace,'.github','hooks','gtmux-activity.json');
    if (agent==='opencode')return join(workspace,'.opencode','plugins','gtmux-activity.js');
    throw new Error('Unknown agent.');
  }
  async preview(agent,remove=false) {
    const target=this.target(agent);await regular(target);
    const before=await readOptional(target);const manifest=join(this.server.root,'agent-integration',`${agent}-installed.json`);
    const installed=JSON.parse(await readOptional(manifest)??'null');
    if(installed && installed.target!==target)throw new Error('This integration was installed for another workspace. Use that workspace to restore it.');
    let after;
    if(remove){
      if(!installed)throw new Error('No installation made by this manager is recorded.');
      if(revision(before??'')!==installed.appliedRevision)throw new Error('Agent settings changed after installation. Remove only the gtmux hook entries manually to preserve those changes.');
      after=installed.before??(agent==='opencode'?'// gtmux Activity integration disabled.\n':'{}\n');
    }else{
      if(installed && revision(before??'')!==installed.appliedRevision)throw new Error('Agent settings changed after installation. Review and merge the generated integration manually.');
      after=mergeHooks(agent,before,(await this.generate(this.server,agent)).configuration);
    }
    this.plan={id:randomUUID(),agent,target,manifest,before,after,remove,installed};
    return {id:this.plan.id,agent,target,remove,changed:before!==after,description:remove?'Restore the settings backed up by this manager.':'Append gtmux hooks while retaining existing hooks and other settings. Back up before writing. Restart the agent and review any hook trust prompt.'};
  }
  async apply({planId,confirmed}) {
    const p=this.plan;if(!confirmed||!p||p.id!==planId)throw new Error('Review and confirm the integration change first.');
    await regular(p.target);
    if(await readOptional(p.target)!==p.before)throw new Error('Agent settings changed. Review again before applying.');
    const backup=p.target+'.gtmux-backup-'+Date.now();
    if(p.before!==null)await atomicWrite(backup,p.before);
    if(await readOptional(p.target)!==p.before)throw new Error('Agent settings changed while creating the backup. Nothing overwritten.');
    await atomicWrite(p.target,p.after);
    try {
      await atomicWrite(p.manifest,JSON.stringify(p.remove?null:{target:p.target,before:p.installed?p.installed.before:p.before,appliedRevision:revision(p.after)}));
    } catch (error) {
      // Do not overwrite a concurrent user edit while attempting recovery.
      if (p.before !== null && await readOptional(p.target) === p.after) await atomicWrite(p.target,p.before);
      this.plan = null;
      throw new Error(`Could not save integration recovery information: ${error.message}. Review ${p.target}${p.before === null ? ' and remove the newly added hooks if needed' : ` and its backup ${backup}`}.`);
    }
    this.plan=null;return {applied:true,target:p.target,backup:p.before===null?null:backup};
  }
}
