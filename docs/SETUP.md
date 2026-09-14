# Dashboard setup, authentication and persistent storage

The dashboard runs beside an existing NATS server. SQLite is embedded. No database service is required. The [hosted setup guide](https://axmouth.github.io/natsui/setup.html) includes the same startup path. The disposable cluster demo remains unauthenticated on host loopback. This guide configures a separate authenticated dashboard.

## Extended deployment options

[Shared dashboard users and HTTPS](SHARED_ACCESS.md), [monitoring certificates and credentials](MONITORING_SECURITY.md), [multiple profiles](PROFILES.md) and [optional managed NATS users/settings](MANAGED.md) have dedicated guides. The local demo remains unauthenticated. Explicit configuration enables each production capability.

## Ansible deployment

The [Ansible guide](ANSIBLE.md) includes a playbook for Vault-backed dashboard credentials, optional NATS/TLS files, persistent storage, change-triggered recreation, readiness checks and SSH-tunneled access. Deployment is non-interactive once automation credentials are configured.

## Docker with an existing broker

The commands below use Docker Desktop with Linux containers or Docker Engine. NATS_NETWORK and NATS_SERVICE are placeholders for an existing private Docker network and broker service name. The broker must be reachable from that network. A dedicated restricted NATS identity is described in [SECURITY.md](../SECURITY.md).

Generate an access key into a separate named volume. This command refuses to overwrite an existing file:

```sh
docker run --rm -v natsui-auth:/data ghcr.io/axmouth/natsui:latest --init-auth /data/access.key
```

Read the key locally for sign-in. This deliberately displays a secret. The output belongs in a password manager, not logs or source control:

```sh
docker run --rm --entrypoint cat -v natsui-auth:/data:ro ghcr.io/axmouth/natsui:latest /data/access.key
```

Start the dashboard with separate mounts for history and the read-only secret:

```sh
docker run -d --name natsui --network NATS_NETWORK -p 127.0.0.1:4321:4321 -e NATSUI_URL=nats://NATS_SERVICE:4222 -e NATSUI_PROFILE=production-observer -e NATSUI_AUTH_TOKEN_FILE=/run/natsui-auth/access.key -v natsui-data:/data -v natsui-auth:/run/natsui-auth:ro --read-only --cap-drop ALL --security-opt no-new-privileges:true ghcr.io/axmouth/natsui:latest
```

Run docker exec natsui natsui login and open its one-time link within 60 seconds, or open http://127.0.0.1:4321 and enter the access key. Native NATS credentials and TLS files use separate read-only mounts. An unauthenticated broker is not required. Add NATSUI_CREDS or supported URL credentials for the target deployment. NATSUI_MONITOR_URLS is optional and independent of broker authentication. NATSUI_ALLOW_WRITES defaults to 0. Enabling reviewed edits also requires suitable NATS permissions.

A Compose alternative for a broker reachable through host.docker.internal is available as [compose.auth.yaml](../deploy/compose.auth.yaml). It uses the same natsui-auth and natsui-data volumes. The Compose initializer creates and validates the key in natsui-auth automatically. It preserves an existing valid key and refuses invalid files. NATSUI_URL can override the default broker address. A broker bound only to host loopback may not be reachable through the Docker gateway. The shared private network example above avoids that limitation.

## Connect a Docker cluster, step by step

For a machine without NATS, the local demo starts a real three-node cluster, Natsui and scripted traffic (Docker Compose 2.34+):

```sh
docker compose -f oci://ghcr.io/axmouth/natsui-demo:latest up -d --wait
```

Open http://127.0.0.1:4321. This is an unauthenticated local tryout, with no certificates or existing cluster required. The following recipe attaches Natsui to an existing cluster. It does not create servers or change broker authentication. Docker Desktop must use Linux containers. Docker Engine requires the Compose plugin. Compose 2.34+ supports all commands in this guide. Commands run in PowerShell, Bash or Zsh unless labeled otherwise. Stop a previous dashboard using port 4321 before starting this recipe. Only one Natsui process may use the natsui-data volume.

### 1. Find the network and listening ports

List containers, then inspect one broker using its actual container name:

```sh
docker ps --format "table {{.Names}}\t{{.Ports}}"
docker inspect nats1 --format '{{json .NetworkSettings.Networks}}'
```

The network object key, for example myapp_default, is the value for NATS_NETWORK. The service names or network aliases, such as nats1 and nats2, must resolve on that network. The dashboard needs access to every advertised client address and every configured monitoring endpoint.

A mapping such as 14222:4222 means port 14222 on the host forwards to port 4222 inside the container. On the shared network, use nats1:4222. Separate containers can all listen on 4222 and 8222. Different internal ports also work, but the URL must match each server's configuration. NATS client ports, commonly 4222, are separate from route ports, commonly 6222, and HTTP monitoring ports, commonly 8222. Natsui does not connect to route ports.

### 2. Prepare a local deployment folder

Create an empty folder called natsui-deployment and open a terminal there. Download [compose.network.yaml](../deploy/compose.network.yaml) into it. Save a file named .env (not .env.txt) beside the Compose file, with the values adjusted to the existing cluster:

```dotenv
NATS_NETWORK=myapp_default
NATSUI_URL=nats://nats1:4222,nats://nats2:4222,nats://nats3:4222
NATSUI_MONITOR_URLS=http://nats1:8222,http://nats2:8222,http://nats3:8222
NATSUI_PROFILE=cluster-observer-v2
```

### What NATSUI_URL means

In this example, Docker Compose reads the .env file and passes NATSUI_URL to the dashboard container as an environment variable. Natsui reads the value at startup to choose its initial broker connection.

| Part | Meaning |
| --- | --- |
| `NATSUI_URL` | The Natsui setting for one or more initial NATS server addresses. |
| `=` | Assigns the address on the right to the setting on the left. |
| `nats://` | The NATS protocol scheme. A tls:// address explicitly requires TLS. |
| `nats1` | The broker hostname. In this Docker example, it is a service name or network alias on the shared network. |
| `4222` | The client port on which that broker listens inside its container. |

For example, a broker service named messaging listening on port 4223 needs NATSUI_URL=nats://messaging:4223. The browser still opens http://127.0.0.1:4321 to reach the dashboard. Natsui makes the broker connection from its backend. This example supplies no broker credentials. Authenticated clusters also need the credentials described below.

NATSUI_URL accepts one address or up to 32 comma-separated starting addresses for the same cluster and account. Natsui attempts the starting addresses concurrently and keeps the first connection that completes the NATS handshake, including required authentication and TLS checks. A stalled or unavailable seed does not hold up a working seed. The winning client retains the configured addresses and discovers advertised NATS peers for later reconnects. The four-second startup attempt budget bounds the race. Subsequent reconnects use the NATS client's retry behavior rather than racing every node again.

Example with three servers listening on different client ports:

```dotenv
NATSUI_URL=nats://nats1:4222,nats://nats2:4223,nats://nats3:4224
```

These addresses are alternatives for a single broker connection. All seeds must belong to the same intended cluster and account. Reachability alone cannot prove that unrelated servers expose the same data. Discovered peers must also be reachable and accept the same identity. URL credentials may appear on one seed and apply to the entire set. If repeated, they must match exactly. Conflicting credentials are rejected. NATSUI_CREDS cannot be combined with credentials in any seed URL. If any seed uses tls://, TLS is required for every configured and discovered peer.

### Why the broker and monitoring addresses are separate

The settings use different protocols and collect different data. NATSUI_URL contains NATS client addresses. One working connection carries JetStream API requests for streams, consumers and stored records within the selected account. NATSUI_MONITOR_URLS contains HTTP(S) monitoring addresses. Each endpoint describes its own NATS process, so Natsui queries every configured endpoint for that node's CPU, RAM, connections and subscriptions.

NATS discovery supplies additional NATS client addresses. It does not supply HTTP monitoring URLs, their ports or their authentication settings. Natsui therefore does not derive a monitoring URL by changing 4222 to 8222, add monitoring endpoints for discovered brokers, or replace a failed node's metrics with another node's values. Adding a server requires updating NATSUI_MONITOR_URLS to include its monitoring endpoint and recreating the dashboard container. If one of three monitoring endpoints fails, the other two still report and the failed endpoint is shown as unavailable.

NATSUI_MONITOR_URLS is a comma-separated list of up to 32 HTTP(S) origins. For servers actually listening on different internal monitoring ports, this line becomes:

```dotenv
NATSUI_MONITOR_URLS=http://nats1:8222,http://nats2:8223,http://nats3:8224
```

Monitoring must already be enabled on those servers. An empty NATSUI_MONITOR_URLS disables node/connection monitoring while JetStream inventory remains available. Monitoring endpoints are configured separately and are not discovered from the cluster. For a named JetStream domain, add NATSUI_DOMAIN=DOMAIN_NAME with the actual domain, not a server or cluster name.

The .env file supplies Compose substitutions. An existing shell variable with the same name takes precedence. Keep .env and local-secrets outside source control. The profile identifies persisted history: use a new profile name when changing the configured seed set, domain, credential file, client certificate or NATS identity. Reordering or repeating the same seed addresses does not change the binding. Moving from a single seed to a larger set requires a new profile name. Do not delete history to resolve a profile-binding error.

### 3. Initialize dashboard login once

```sh
docker run --rm -v natsui-auth:/data ghcr.io/axmouth/natsui:latest --init-auth /data/access.key
docker run --rm --entrypoint cat -v natsui-auth:/data:ro ghcr.io/axmouth/natsui:latest /data/access.key
```

The second command displays the dashboard sign-in secret. Store it privately. If this volume was initialized earlier, reuse the existing key. Generation intentionally refuses to overwrite it. This key protects Natsui and is not a NATS password.

### 4. Select broker authentication and start

For a broker without authentication on a trusted private network:

```sh
docker compose --env-file .env -f compose.network.yaml up -d --wait
```

For authenticated or encrypted brokers, select the recipe below before starting. Every recipe retains the login volume, writable SQLite directory, private broker network, read-only root filesystem and host-loopback dashboard port from compose.network.yaml. These files start only Natsui. They can attach to a cluster managed by another Compose project.

## Broker credentials and TLS recipes

The cluster administrator supplies an observer identity in the application account, with JetStream observation permissions. A system-account login is not required for this inventory. The [restricted permissions template](../deploy/permissions.conf) and [NATS identity guide](../SECURITY.md#nats-identity) describe the allowed requests. The application-side read-only switch does not reduce permissions granted by the broker.

Choose the matching recipe below. Prepare its files and permissions before running its start command. Paths in the overlays resolve relative to compose.network.yaml. Keep all downloaded files in the same deployment folder.

| Broker setup | Compose files after the base file |
| --- | --- |
| Username/password or token | None. Credentials go in NATSUI_URL |
| JWT credentials | compose.creds.yaml |
| Private-CA TLS | compose.tls.yaml |
| Private-CA TLS and JWT | compose.tls.yaml, compose.creds.yaml |
| Mutual TLS | compose.tls.yaml, compose.mtls.yaml |
| Mutual TLS and JWT | compose.tls.yaml, compose.mtls.yaml, compose.creds.yaml |

### Mounted-file permissions

The dashboard container runs as UID/GID 10001. All mounted secrets must be readable by that identity. On a standard rootful Linux Docker host, run these commands from natsui-deployment after adding the required files:

```sh
sudo chgrp 10001 local-secrets
sudo chmod 750 local-secrets
sudo chgrp 10001 local-secrets/*
sudo chmod 640 local-secrets/*
```

The folder should contain only the intended connection files. These commands preserve the owner and grant the container's group read access without making private keys world-readable. Rootless Docker and user-namespace remapping need permissions for the mapped host identity. Docker Desktop on Windows uses host filesystem sharing and ACLs. Restrict the folder to the local operator and Docker's required access rather than running Unix chmod commands. Do not mount a CA private key or an entire home directory.

### Username/password or token

Replace the NATSUI_URL line in .env with one of these forms. The uppercase values below are placeholders, not usable credentials:

```dotenv
NATSUI_URL='nats://OBSERVER:URL_ENCODED_PASSWORD@nats1:4222'
```

```dotenv
NATSUI_URL='nats://URL_ENCODED_TOKEN@nats1:4222'
```

Percent-encode reserved characters inside the username, password or token, not the entire URL. For example, a comma becomes %2C, @ becomes %40, : becomes %3A, / becomes %2F, # becomes %23, $ becomes %24 and % becomes %25. Use the base start command above. URL credentials become container environment values, visible to Docker administrators. Do not paste rendered Compose configuration or environment dumps into logs or issue reports.

For credentials sent across an untrusted network, use TLS as described below. TLS changes the transport. It does not replace required NATS user authentication.

### JWT credentials file

Download [compose.creds.yaml](../deploy/compose.creds.yaml) beside the base file. Create local-secrets beside those files and place the administrator-issued observer.creds there. This file contains a user JWT and its private seed. It is not an account JWT, operator JWT, password file, or dashboard access key.

```text
natsui-deployment/
  .env
  compose.network.yaml
  compose.creds.yaml
  local-secrets/
    observer.creds
```

Use NATSUI_URL=nats://nats1:4222 without URL credentials. NATSUI_CREDS and URL authentication cannot be combined. Start with:

```sh
docker compose --env-file .env -f compose.network.yaml -f compose.creds.yaml up -d --wait
```

The overlay mounts only ./local-secrets/observer.creds at /run/nats/observer.creds, read-only, and sets NATSUI_CREDS to that container path. Missing source files fail startup instead of silently becoming directories. For encrypted transport, combine this overlay with the TLS recipe.

### TLS with a private certificate authority

Download [compose.tls.yaml](../deploy/compose.tls.yaml). Create local-secrets beside compose.network.yaml if it does not exist. Obtain the cluster's trusted CA certificate bundle as local-secrets/ca.pem and change every URL scheme in the .env seed list to tls://, retaining any required URL credentials. This is a public CA certificate bundle, not the CA private key. The overlay mounts it read-only at /run/nats/ca.pem and sets NATSUI_TLS_CA and NATSUI_TLS_REQUIRED=1.

```sh
docker compose --env-file .env -f compose.network.yaml -f compose.tls.yaml up -d --wait
```

For TLS plus JWT credentials, use:

```sh
docker compose --env-file .env -f compose.network.yaml -f compose.tls.yaml -f compose.creds.yaml up -d --wait
```

The server certificates must validate for the names or IP addresses used to connect, including discovered peers. A certificate for localhost does not automatically validate for nats1. The trusted bundle must cover all peer certificate chains. With a publicly trusted certificate, a tls:// URL can use the client's standard trust without the private-CA overlay. There is no skip-certificate-verification option.

### Mutual TLS, with optional JWT credentials

Mutual TLS means Natsui also presents a client certificate. The NATS servers must already require or accept that certificate, trust its issuer, and permit the intended identity. Obtain a client-auth certificate and its matching unencrypted PEM private key from the cluster administrator. A server certificate or the CA signing key is not a substitute. File protection is required because unattended startup cannot prompt for a key passphrase.

Download [compose.mtls.yaml](../deploy/compose.mtls.yaml) and arrange the files as follows:

```text
natsui-deployment/
  .env
  compose.network.yaml
  compose.tls.yaml
  compose.mtls.yaml
  local-secrets/
    ca.pem
    client.pem
    client.key
```

The client certificate file should contain the client certificate followed by any required intermediate certificates. The additional overlay sets NATSUI_TLS_CERT=/run/nats/client.pem and NATSUI_TLS_KEY=/run/nats/client.key. Each file is mounted individually and read-only. With NATSUI_URL=tls://nats1:4222, start with:

```sh
docker compose --env-file .env -f compose.network.yaml -f compose.tls.yaml -f compose.mtls.yaml up -d --wait
```

Mutual TLS alone does not necessarily authenticate a NATS account. If the cluster also requires a user JWT, add observer.creds and compose.creds.yaml from the JWT recipe, then run:

```sh
docker compose --env-file .env -f compose.network.yaml -f compose.tls.yaml -f compose.mtls.yaml -f compose.creds.yaml up -d --wait
```

The mutual TLS overlay contains these exact environment settings and read-only file mounts:

```yaml
services:
  dashboard:
    environment:
      NATSUI_TLS_CERT: /run/nats/client.pem
      NATSUI_TLS_KEY: /run/nats/client.key
    volumes:
      - type: bind
        source: ./local-secrets/client.pem
        target: /run/nats/client.pem
        read_only: true
        bind:
          create_host_path: false
      - type: bind
        source: ./local-secrets/client.key
        target: /run/nats/client.key
        read_only: true
        bind:
          create_host_path: false
```

If it instead requires username/password, retain those credentials in the tls:// URL and omit compose.creds.yaml. Natsui does not issue certificates, provision NATS users or edit the cluster's authentication configuration in these recipes.

### Native credential and TLS paths

For native Natsui, the variables are identical but point to host files. Follow the native key/data setup below, then replace its NATSUI_URL assignment with these connection settings before launching the executable. PowerShell example for mutual TLS plus JWT:

```powershell
$env:NATSUI_URL = 'tls://broker.example.internal:4222'
$env:NATSUI_CREDS = "$PWD/local-secrets/observer.creds"
$env:NATSUI_TLS_CA = "$PWD/local-secrets/ca.pem"
$env:NATSUI_TLS_CERT = "$PWD/local-secrets/client.pem"
$env:NATSUI_TLS_KEY = "$PWD/local-secrets/client.key"
```

Bash/Zsh equivalent:

```sh
export NATSUI_URL=tls://broker.example.internal:4222
export NATSUI_CREDS="$PWD/local-secrets/observer.creds"
export NATSUI_TLS_CA="$PWD/local-secrets/ca.pem"
export NATSUI_TLS_CERT="$PWD/local-secrets/client.pem"
export NATSUI_TLS_KEY="$PWD/local-secrets/client.key"
```

A native process outside Docker usually cannot resolve Compose service names. Use reachable hostnames and published client/monitoring ports. Discovered peer addresses must also be reachable from the host.

## Verify the connection and troubleshoot

Open http://127.0.0.1:4321 and sign in with the dashboard access key. Check the cluster connection status and stream inventory, then open Nodes. Three successful monitoring endpoints should show 3/3 reporting. Empty inventory can be valid for a new application account. An unavailable/error status is not an empty cluster. Existing streams should match the selected NATS account and JetStream domain. HTTP readiness alone is not proof of broker access or healthy monitoring.

Use the same Compose file list as the selected start command when inspecting, updating or stopping the deployment. For example, the full mutual TLS/JWT recipe uses:

```sh
docker compose --env-file .env -f compose.network.yaml -f compose.tls.yaml -f compose.mtls.yaml -f compose.creds.yaml ps
docker compose --env-file .env -f compose.network.yaml -f compose.tls.yaml -f compose.mtls.yaml -f compose.creds.yaml logs --tail 50 dashboard
```

The HTTP monitoring connection is independent of NATS authentication and TLS. NATSUI_MONITOR_URLS supports ordinary HTTP and HTTPS with standard trust, but does not support HTTP credentials, custom monitoring CAs, or monitoring client certificates. NATSUI_TLS_* applies only to the broker connection. Keep unauthenticated monitoring on the private network. Do not publish its ports merely to make Natsui reach it. A protected monitoring endpoint can remain unconfigured while JetStream inventory works.

- Network not found: check the network name from docker inspect and whether the broker stack is running. The external network is not created by this recipe.
- Connection timeout: check the service alias, container listening port, network membership and advertised peer addresses. At least one configured seed must complete the NATS connection handshake.
- TLS failure: check certificate expiry, trusted CA bundle, server names, client certificate purpose and matching private key. Do not disable verification to bypass a mismatch.
- Authentication or permission failure: check that the NATS identity is accepted on every peer, belongs to the intended account and can make the required JetStream requests. A dashboard login key cannot authenticate to NATS.
- Streams work but nodes do not: check monitoring ports and the independent HTTP authentication/TLS limitations. A partial reporting count identifies unavailable configured endpoints, not necessarily a failed broker.
- Missing file or permission denied: check that source files exist as files beside the Compose file and are readable by UID 10001. Container environment paths must match the mount targets, not host paths.
- Profile already bound: select a new NATSUI_PROFILE after a connection identity change. Keep the existing data volume and its history.
- Dashboard port already in use: stop the earlier dashboard or change only the host side of the mapping, for example 127.0.0.1:4322:4321, and open that port.

To apply changed environment settings or mounted credentials, repeat the selected start command with --force-recreate. This reloads startup configuration and invalidates dashboard sessions. Keep the same history volume. Credential-file/client-certificate changes require a new profile name under the current identity guard. To stop, replace up -d --wait with down in that command. Omit -v to retain history. Pin a published image digest instead of latest when a repeatable deployment version is required.

NATS user creation and management remain planned. Dashboard login, broker credentials, TLS certificates and SQLite history are distinct resources. Sources: [NATS configuration management](https://docs.nats.io/learn/deployment/config-management) and [NATS JWT administration](https://github.com/nats-io/nats.docs/blob/master/running-a-nats-service/nats_admin/jwt.md).

## Native binary

Generate the key once and use stable data/secret paths. NATS credentials use the same variables as the container.

```powershell
New-Item -ItemType Directory -Force local-secrets, data
.\natsui.exe --init-auth .\local-secrets\dashboard.key
$env:NATSUI_AUTH_TOKEN_FILE = "$PWD/local-secrets/dashboard.key"
$env:NATSUI_DATA_DIR = "$PWD/data"
$env:NATSUI_URL = 'nats://127.0.0.1:4222'
$env:NATSUI_PROFILE = 'production-observer'
.\natsui.exe
```

```sh
mkdir -p local-secrets data
./natsui --init-auth ./local-secrets/dashboard.key
export NATSUI_AUTH_TOKEN_FILE="$PWD/local-secrets/dashboard.key"
export NATSUI_DATA_DIR="$PWD/data"
export NATSUI_URL=nats://127.0.0.1:4222
export NATSUI_PROFILE=production-observer
./natsui
```

Subsequent starts reuse the existing key. Read its file locally for sign-in and restrict secret-directory access with OS permissions.

## Login behavior

Named dashboard users, roles and one-time login links are described in the [shared-access guide](SHARED_ACCESS.md). The configured key is a recovery administrator.

- NATSUI_AUTH_TOKEN_FILE enables a single-operator access-key login. An unset variable retains trusted local access. An empty, unreadable or malformed configured file fails startup.
- --init-auth generates 256 random bits encoded as 64 hexadecimal characters. It does not create a human password or a NATS credential. The process retains a SHA-256 digest for comparison, not the raw access key.
- Sign-in creates an HttpOnly, SameSite=Strict cookie. Sessions expire after eight hours, are revoked by Sign out, and are all invalidated by a process restart. At most 32 sessions are retained. The oldest is evicted when full.
- Sign-in accepts at most 30 attempts per minute across the instance. Requests have a 1 KiB body limit. Mutation requests still require the same-origin request header and existing Host/Origin checks.
- All holders of the access key have the same configured dashboard capabilities. Separate accounts, roles, per-user audit attribution and OIDC are not implemented. Configuration audit entries still identify a local operator.
- Login assets and the minimal health/readiness endpoints are public. Inventory, payload inspection, exports, settings and editing APIs require a valid session when authentication is enabled.
- The cookie is for the existing HTTP loopback deployment and has no Secure flag. This does not enable public HTTP or reverse-proxy deployment. Remote HTTPS, explicit public origins and team authorization remain separate work. HTTP secrets must not be sent over an untrusted network.
- The static browser demo has no backend and no login. It contains no real broker credentials or data.

### Rotation and recovery

Generate a new key under a new filename in the auth volume, such as /data/access-next.key. Recreate the dashboard with NATSUI_AUTH_TOKEN_FILE pointing at the new filename and the same history volume. All old sessions are invalidated when the old process stops. The old key remains usable until that process stops. Remove obsolete key files only after the new deployment is verified.

Loss of the access key does not require deleting SQLite history. The host or Docker administrator can provision a new key and restart with its path. Anyone able to read the secret volume, change process configuration or control Docker remains inside the trusted administrator boundary.

## SQLite volume contract

The image sets NATSUI_DATA_DIR=/data. The native binary defaults to ./data relative to its working directory. The database is natsui.sqlite3, with natsui.sqlite3-wal and natsui.sqlite3-shm potentially present beside it.

**Mount the entire directory, not just natsui.sqlite3.** The writable volume stores observations, dashboard settings, activity, incidents and profile bindings. Message payloads are fetched on demand and are not stored by Natsui. The access key belongs in its separate secret mount. Sessions are deliberately memory-only.

| Deployment | Persistent mount | Lifecycle |
| --- | --- | --- |
| Docker run above | natsui-data:/data | Survives container stop, removal and replacement while the named volume remains |
| Authenticated Compose example | natsui-data:/data | Same stable named volume. Authentication uses a separate stable named volume |
| Root compose.yaml | Project-prefixed dashboard-data volume | Reuse the same Compose project name and file to retain the same history |
| Published cluster demo | Tryout project's dashboard-data plus three broker volumes | down retains volumes. `down -v` deletes demo history and broker storage |
| Native binary | NATSUI_DATA_DIR or ./data | Use a stable absolute directory when launching from different working directories |

A container writable layer is not persistent storage. Omitting the volume can lose history on replacement. The read-only root filesystem can also prevent startup without a writable data mount. A volume is persistence, not a backup. Do not run multiple dashboard processes against the same data directory. Use a local filesystem, not a shared NFS/SMB directory, for SQLite WAL mode.

The image runs as UID/GID 10001. Fresh named volumes mounted at /data inherit the image directory ownership. Bind mounts and restored directories must be writable by that UID, including directory creation of WAL/SHM files. Mount credentials read-only and ensure UID 10001 can read them. On Windows, restrict native secret/data directories with filesystem ACLs. Unix key generation creates the file with mode 0600.

Default retention is seven days and the serialized history budget is 128 MiB. NATSUI_HISTORY_MAX_MB adjusts that budget. SQLite indexes, WAL and reusable pages mean physical disk use can exceed the logical budget. Existing [profile identity safeguards](../SECURITY.md#history-identity-and-migration) remain in force after restart or restore.

## Backup and restore

The simplest supported procedure is a stopped-process copy of the complete data directory. A live copy of only natsui.sqlite3 can omit committed WAL transactions. The [SQLite backup API](https://www.sqlite.org/backup.html) is an alternative for online backups. Natsui does not yet expose a backup button. [SQLite WAL documentation](https://www.sqlite.org/wal.html) explains why the companion files matter.

The following commands use the natsui container and natsui-data volume from the Docker run example. For the Compose recipes, stop/start the dashboard service with the selected Compose file list instead of docker stop/start natsui. The volume archive command is unchanged. No other process may write to that volume during the copy. The archive is written into the current directory. The backup helper runs as root to preserve ownership, with the source volume mounted read-only:

```sh
docker stop natsui
docker run --rm --user 0 --entrypoint tar -v natsui-data:/data:ro --mount "type=bind,source=${PWD},target=/backup" ghcr.io/axmouth/natsui:latest -czf /backup/natsui-data.tar.gz -C /data .
docker start natsui
```

These commands work in PowerShell, Bash and Zsh from an existing local directory. In scripts, restart only after checking whether backup succeeded. Retain the prior successful archive if a new backup fails. Use timestamped archive filenames for multiple backups. Protect archives like operational data.

Restore a trusted archive into a fresh volume, avoiding an overwrite of active history:

```sh
docker volume create natsui-restored-data
docker run --rm --user 0 --entrypoint tar -v natsui-restored-data:/data --mount "type=bind,source=${PWD},target=/backup,readonly" ghcr.io/axmouth/natsui:latest -xzf /backup/natsui-data.tar.gz -C /data .
```

Start a replacement dashboard with -v natsui-restored-data:/data, the same NATS identity/profile, and the original image version or a documented compatible upgrade. Use a different container name and loopback port for a restore drill. Check /readyz, sign in, and inspect settings and history before replacing the original instance. Newer database schemas may not open in older binaries. Keep a stopped-process backup before upgrades. Pin image versions or digests when repeatable restores matter.

For Compose deployments, inspect the dashboard's /data mount with docker inspect before choosing a volume name. Docker compose down preserves named volumes. `docker compose down -v` removes non-external volumes. The authenticated example uses named auth and history volumes. Both can be removed by down -v, so backups must cover their separate recovery requirements. Native backups follow the same stop/copy-complete-directory/restart procedure.
