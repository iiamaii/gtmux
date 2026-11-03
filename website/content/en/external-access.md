# External HTTPS access

## Prepare

Use a public DNS hostname you control. Point its A/AAAA records to your computer or gateway. Public TCP ports 80 and 443 must reach the chosen local proxy ports. DNS, router forwarding and firewall changes are outside this manager's control.

The manager does not buy cloud servers, change DNS accounts, open firewall rules or register system services. It installs an application-local Caddy binary, writes a reviewed configuration, runs the proxy and lets Caddy obtain and renew certificates. Protected ports can require OS permission; use high local ports with gateway forwarding or your existing proxy when appropriate. A bind permission error is shown rather than requesting blanket elevation.

## Managed proxy

1. Open External access and enter your domain and local proxy ports.
2. Review the DNS lookup, network prerequisites and exact configuration changes.
3. Install the proxy. The pinned official release archive is checked against its release SHA-256 checksum before extraction.
4. Apply and restart. This ends managed terminal programs, backs up the previous TOML, writes the new configuration and starts server and proxy.
5. Verify public HTTPS. The response must identify this same running server. A running proxy alone is not proof of reachability or a valid certificate.

The generated configuration keeps the backend on `127.0.0.1`, enables cloud authentication policy, sets secure cookies and permits only the public origin and exact local management host. Only `127.0.0.1/32` is trusted to supply forwarded client addresses. The management control page is not proxied.

## Existing proxy

Select an existing proxy on the same computer. Forward HTTPS and WebSocket traffic to `127.0.0.1:<server-port>`, preserve Host, and pass the actual client address in X-Forwarded-For. The preview produces the corresponding server configuration but does not modify your proxy. Remote proxy topologies require manual configuration and are not claimed by this wizard.

## Stop or restore

Restore local access stops the owned proxy and restores the backed-up TOML, then restarts the server. If the file has changed since deployment, automatic restore refuses to overwrite those edits. Inspect `proxy/server.before.toml` and merge the intended values manually. Certificates and proxy data remain in the manager's data folder.

A failed deployment attempts to restore the prior configuration. Read the reported error if recovery also fails. Keep the management page open while troubleshooting; it remains local even when public access is broken.

## Reference

[Caddy automatic HTTPS](https://caddyserver.com/docs/automatic-https) describes certificate issuance and renewal. Existing gateway/subpath deployments remain separate from this root-origin wizard.

## Turn HTTPS off and on again

Use the External HTTPS switch in the manager’s External access section. Turning it off asks for confirmation before restarting the server and restoring the original local configuration. Caddy stays installed and the last domain, proxy mode and ports are remembered, without storing your password. To enable it again, turn the switch on, review the saved fields, and choose Apply and restart. The switch stays off until the configuration is applied. An enabled switch means configured, not verified public connectivity; use Verify public HTTPS to check reachability. An existing external proxy remains under its owner’s control.

## IP addresses and certificate status

The same wizard accepts a public IPv4/IPv6 address as well as a domain. Choose **Public CA (Let's Encrypt)** for a publicly reachable address you control. Caddy uses the short-lived ACME profile for IP certificates; keep Caddy running for renewal. A public IP still needs incoming validation traffic on public ports 80/443. Enter the externally visible HTTPS port separately from the local listener when gateway forwarding differs. The manager cannot provision a public IP, change your router, or bypass carrier-grade NAT.

For a private LAN IP or loopback address, choose **Local CA**. Caddy issues a private certificate and never automatically modifies the operating system trust store. After applying, **Download local CA certificate** exports only the public root certificate. Trust it on each intended client through that client's certificate manager; the private CA key remains in the manager data directory. Verification pins that CA only for this manager's own probe. Other clients remain untrusted until configured.

**Verify public HTTPS** shows issuer, expiration, remaining days and verification time, and confirms the endpoint belongs to the current server. It rejects untrusted certificates and a proxy pointing at another server. This is an on-demand snapshot, not continuous remote monitoring. A successful local probe does not prove that an external network can reach your router; also test from another network before relying on public access.

The pinned Caddy binary has been tested with actual local CA issuance and its public-IP configuration schema. Actual public-domain/IP ACME issuance requires a reachable address controlled by the operator and is not part of isolated tests. See [Let's Encrypt IP certificates](https://letsencrypt.org/2026/01/15/6day-and-ip-general-availability/).

IP listeners set Caddy’s [default SNI](https://caddyserver.com/docs/caddyfile/options#default-sni) to the configured address so clients without SNI receive the right certificate behind NAT. The regression test connects to loopback with a different certificate IP and validates its identity without disabling certificate checks.
