import test from 'node:test';
import assert from 'node:assert/strict';
import { endpoint, certificateInfo } from '../src/https.mjs';
import { proxyConfiguration } from '../src/proxy.mjs';
test('IP HTTPS distinguishes public ACME and private CA without disabling trust', () => {
  assert.equal(endpoint('8.8.8.8').origin, 'https://8.8.8.8');
  assert.equal(endpoint('[2001:4860:4860::8888]').origin, 'https://[2001:4860:4860::8888]');
  for (const host of ['127.0.0.1','192.168.1.20','10.0.0.1','100.64.0.1','203.0.113.1','::1','fc00::1','2001:db8::1','::ffff:8.8.8.8']) assert.throws(() => endpoint(host));
  assert.equal(endpoint('192.168.1.20','local',8443).origin, 'https://192.168.1.20:8443');
  assert.equal(endpoint('::1','local',8443).authority, '[::1]:8443');
  const local = proxyConfiguration('::1', 9001, 8080, 8443, 'local', 8443);
  assert.equal(local.apps.pki.certificate_authorities.local.install_trust, false);
  assert.deepEqual(local.apps.tls.automation.policies[0].issuers, [{ module: 'internal' }]);
  const publicIP = proxyConfiguration('8.8.8.8', 9001);
  assert.equal(publicIP.apps.http.servers.gtmux.tls_connection_policies[0].default_sni, '8.8.8.8');
  assert.equal(publicIP.apps.tls.automation.policies[0].issuers[0].profile, 'shortlived');
  for (const host of ['https://a.com','a.com:443','[::1]:443','x@y.com','1.2.3.999','a.com\n','a.com/path']) assert.throws(() => endpoint(host,'local'));
});
test('certificate expiry is derived from the verified peer certificate', () => {
  const info = certificateInfo({ valid_from: '2026-01-01T00:00:00Z', valid_to: '2026-01-07T00:00:00Z', issuer: { CN: 'Test CA' }, fingerprint256: 'AA' });
  assert.equal(info.expiresAt,'2026-01-07T00:00:00.000Z');
  assert.equal(info.issuer,'Test CA');
  assert.throws(() => certificateInfo({}));
});
