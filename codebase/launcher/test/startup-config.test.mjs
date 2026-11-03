import test from 'node:test';
import assert from 'node:assert/strict';
import { replaceStartupPort, replaceStartupWorkspace } from '../src/startup-config.mjs';
import { parse } from 'smol-toml';
test('startup port editing preserves comments and existing proxy policy',()=>{
  const before='# keep\n[server]\nport = 9_001 # my port\nbind="127.0.0.1"\n[security]\nhost_allowlist=["127.0.0.1:9001", "external.example"]\ncors_origins=[\'http://localhost:9001\']\n';
  const text=replaceStartupPort(before,9123), result=parse(text);
  assert(text.includes('# keep'));assert(text.includes('# my port'));
  assert.equal(result.server.port,9123);
  assert.deepEqual(result.security.host_allowlist,['127.0.0.1:9123','external.example']);
  assert.deepEqual(result.security.cors_origins,['http://localhost:9123']);
  assert.throws(()=>replaceStartupPort('server={port=9001}',9123),/TOML/);
});

test('multiline loopback policy is refused without a partial port edit',()=>{
  assert.throws(()=>replaceStartupPort('[server]\nport=9001\n[security]\nhost_allowlist=[\n\"127.0.0.1:9001\"\n]\n',9123),/multiline/);
});

test('workspace editing preserves separate defaults, comments, Windows paths and other tables',()=>{
  const before = `server_workspace = '/old' # project\ndefault_session_workspace = '/separate'\nworkspace_path = '/store'\n[server]\nport = 9001\n`;
  const path = 'D:\\Projects\\my folder';
  const next = replaceStartupWorkspace(before,path);
  assert.equal(parse(next).server_workspace,path);
  assert.equal(parse(next).default_session_workspace,'/separate');
  assert.equal(parse(next).workspace_path,'/store');
  assert(next.includes('# project')); assert(next.includes('[server]\nport = 9001'));
  assert.throws(()=>replaceStartupWorkspace(`server_workspace = """/old"""`,'/new'),/TOML/);
});
