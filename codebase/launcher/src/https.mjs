import { isIP } from 'node:net';
import { get } from 'node:https';
export function endpoint(value, certificate = 'public', externalPort = 443) {
  if (!['public', 'local'].includes(certificate)) throw new Error('Choose a public certificate or a local CA certificate.');
  if (!Number.isInteger(externalPort) || externalPort < 1 || externalPort > 65535) throw new Error('External HTTPS port must be from 1 to 65535.');
  if (typeof value !== 'string' || !value || value !== value.trim() || /[\s/@?#%]/.test(value)) throw new Error('Enter a hostname or IP address without a scheme, path or port.');
  let host = value.startsWith('[') && value.endsWith(']') ? value.slice(1, -1) : value;
  const ip = isIP(host);
  if (!ip && (host.length > 253 || !host.includes('.') || !host.split('.').every(s => /^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?$/i.test(s)) || /^[0-9.]+$/.test(host))) throw new Error('Enter a valid DNS hostname, IPv4 or IPv6 address.');
  host = host.toLowerCase();
  if (ip === 6) host = new URL(`https://[${host}]`).hostname.slice(1, -1);
  let publicIP = false;
  if (ip === 4) {
    const [a,b,c] = host.split('.').map(Number);
    publicIP = !(a === 0 || a === 10 || a === 127 || a >= 224 || (a === 100 && b >= 64 && b <= 127) || (a === 169 && b === 254) || (a === 172 && b >= 16 && b <= 31) || (a === 192 && (b === 168 || b === 0 || (b === 88 && c === 99))) || (a === 198 && (b === 18 || b === 19 || (b === 51 && c === 100))) || (a === 203 && b === 0 && c === 113));
  } else if (ip === 6) publicIP = /^[23]/.test(host) && !/^2001:(db8|0|2|10|20):/.test(host) && !host.startsWith('2002:');
  if (certificate === 'public' && (ip ? !publicIP : /\.(localhost|local|test|invalid|internal|home\.arpa)$/.test(host))) throw new Error('This address needs Local CA certificates. Public CAs cannot validate private or reserved addresses.');
  const authority = (ip === 6 ? `[${host}]` : host) + (externalPort === 443 ? '' : `:${externalPort}`);
  return { host, ip, certificate, externalPort, authority, origin: `https://${authority}` };
}
export function certificateInfo(cert) {
  if (!cert?.valid_to) throw new Error('The endpoint did not provide a certificate.');
  const expiresAt = new Date(cert.valid_to).toISOString();
  return { issuer: cert.issuer?.CN || cert.issuer?.O || 'Unknown issuer', expiresAt,
    validFrom: new Date(cert.valid_from).toISOString(), fingerprint256: cert.fingerprint256,
    daysRemaining: Math.floor((Date.parse(expiresAt) - Date.now()) / 86400000) };
}
export function checkHTTPS(origin, instanceId, ca) {
  return new Promise((resolve, reject) => {
    const req = get(`${origin}/healthz`, { ca, rejectUnauthorized: true, timeout: 10000 }, res => {
      try {
        const certificate = certificateInfo(res.socket.getPeerCertificate());
        res.resume();
        if (res.statusCode !== 200) throw new Error(`HTTPS endpoint returned ${res.statusCode}.`);
        if (!instanceId || res.headers['x-gtmux-server-id'] !== instanceId) throw new Error('HTTPS responds, but it is not this running gtmux server. Check the proxy target.');
        resolve({ verified: true, url: origin, checkedAt: new Date().toISOString(), certificate });
      } catch (e) { res.resume(); reject(e); }
    });
    req.on('timeout', () => req.destroy(new Error('HTTPS verification timed out. Check DNS, forwarding and firewall.')));
    req.on('error', reject);
  });
}
