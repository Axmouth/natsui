# Native HTTPS without a proxy

Natsui can serve HTTPS directly using a mounted server certificate and private key. Authentication remains required. NATS sign-in with session-only history works with this setup and needs no collector identity.

Dashboard HTTPS protects the browser connection. NATSUI_TLS_* protects the separate connection from Natsui to NATS. A NATS client certificate is not a dashboard server certificate.

## 1. Prepare the dashboard certificate

Start with the [NATS sign-in setup](SETUP.md). Keep its .env and compose.nats-login.yaml files. Download [compose.https.yaml](../deploy/compose.https.yaml) into the same folder.

Place these files in local-secrets:

```text
local-secrets/
  dashboard.crt
  dashboard.key
```

dashboard.crt must identify the dashboard hostname in its subject alternative names and permit TLS server authentication. It accepts a PEM certificate chain, with the leaf certificate first, or a single DER certificate. PEM chains include the issuing intermediates. dashboard.key must be the matching unencrypted PEM private key. Files must be readable by UID/GID 10001 inside the container. The [mount permissions guide](SETUP.md#mounted-file-permissions) covers Linux and Docker Desktop.

Use a certificate issued for this dashboard by the organization's CA or another trusted issuer. Certificate issuance and renewal remain deployment responsibilities. Natsui does not request certificates automatically.

## 2. Set the browser address

Add these values to .env, replacing the hostname with the certificate's actual dashboard name:

```dotenv
NATSUI_PUBLIC_URL=https://natsui.example.com:8443
NATSUI_HTTPS_PORT=8443
NATSUI_HTTPS_BIND=127.0.0.1
```

NATSUI_PUBLIC_URL is the address opened in the browser. Its port is the published host port. The container continues to listen on port 4321, now with TLS. DNS must resolve the dashboard name to the Docker host. A local hosts-file entry can supply name resolution for a local test.

The default bind address permits access from the Docker host only. For access from other machines, set NATSUI_HTTPS_BIND to the host's intended network-interface address, or 0.0.0.0 for all host interfaces, and allow the published port through the intended firewall. The Compose overlay replaces the original HTTP port mapping. It requires Compose 2.34+ as used in the setup guide.

## 3. Add trust for a private CA

For a publicly trusted issuer, skip this step. For an organization's CA or a self-signed local certificate, download [compose.https-ca.yaml](../deploy/compose.https-ca.yaml) and place the trusted CA certificate at local-secrets/dashboard-ca.crt. For a self-signed server certificate, that file contains the server certificate itself.

This overlay sets NATSUI_HTTPS_CA for the container's health check and local login-link command. It accepts PEM CA bundles or a single DER certificate. It does not install trust in browsers. Client machines must trust the issuing CA through their normal certificate-management process. The CA private key must never be mounted.

## 4. Start and check

For a publicly trusted dashboard certificate:

```sh
docker compose --env-file .env -f compose.nats-login.yaml -f compose.https.yaml up -d --wait
docker compose --env-file .env -f compose.nats-login.yaml -f compose.https.yaml exec -T dashboard natsui doctor --require-session-only
docker compose --env-file .env -f compose.nats-login.yaml -f compose.https.yaml exec -T dashboard natsui healthcheck
```

For a private CA:

```sh
docker compose --env-file .env -f compose.nats-login.yaml -f compose.https.yaml -f compose.https-ca.yaml up -d --wait
docker compose --env-file .env -f compose.nats-login.yaml -f compose.https.yaml -f compose.https-ca.yaml exec -T dashboard natsui doctor --require-session-only
docker compose --env-file .env -f compose.nats-login.yaml -f compose.https.yaml -f compose.https-ca.yaml exec -T dashboard natsui healthcheck
```

Include compose.tls.yaml as well if the NATS broker connection uses the private-CA TLS recipe. That file mounts the broker CA independently. Open NATSUI_PUBLIC_URL and sign in with the existing NATS identity.

doctor parses configuration and files without contacting the server. healthcheck connects to the running dashboard over loopback and verifies the configured public hostname, certificate chain and validity before requesting /healthz. A successful health check does not prove external DNS, firewall rules, browser trust or NATS permissions.

The usual natsui login command also works over verified native TLS when a dashboard access key is configured. Its socket always connects to loopback, even when the public hostname resolves elsewhere or uses a different published port. NATS-only deployments continue to use the browser sign-in form.

## Native binary settings

Set these environment variables in the service definition before starting the binary:

| Setting | Purpose |
| --- | --- |
| NATSUI_HTTPS_CERT | Path to the dashboard server certificate or PEM chain |
| NATSUI_HTTPS_KEY | Path to its matching unencrypted PEM private key |
| NATSUI_PUBLIC_URL | Exact HTTPS browser origin, including a nonstandard published port |
| NATSUI_HTTPS_CA | Optional private CA trust for local health and login-link commands |
| NATSUI_PORT | Listener port, defaults to 4321 |

NATSUI_NATS_LOGIN=1 or dashboard authentication must also be configured. Native mode retains the normal loopback binding default. NATSUI_CONTAINER=1 binds all interfaces inside a container. The Docker recipe supplies it through the image.

## Renewal and troubleshooting

Replace the mounted certificate files and recreate the dashboard using the same Compose file list. Certificate reload is not automatic. Restart ends active dashboard sessions and discards session-only observations. Keep the persistent /data volume.

| Symptom | Check |
| --- | --- |
| Startup rejects HTTPS configuration | Both certificate and key are required, with authentication and an HTTPS NATSUI_PUBLIC_URL. Check file permissions and matching keys. |
| Container is unhealthy with a private CA | Add compose.https-ca.yaml and the correct CA file. Check certificate expiry and the hostname in NATSUI_PUBLIC_URL. |
| Browser shows a certificate error | Check browser trust, certificate validity, chain and hostname. Container CA settings do not install browser trust. |
| Browser reports forbidden | The address and port must match NATSUI_PUBLIC_URL. Forwarded headers do not expand allowed origins. |
| Plain HTTP does not load | Native HTTPS serves TLS only on the configured port. It has no separate HTTP redirect listener. |
| Streams work but Nodes is empty | Follow [session node monitoring](NATS_LOGIN.md#monitoring-through-nats). Dashboard HTTPS does not grant NATS system access. |

The [reverse-proxy option](SHARED_ACCESS.md#https-through-a-trusted-reverse-proxy) remains supported. NATSUI_PUBLIC_URL alone keeps the existing proxy configuration. Setting NATSUI_HTTPS_CERT and NATSUI_HTTPS_KEY enables direct TLS instead.
