# Dashboard setup, authentication and persistent storage

The dashboard runs beside an existing NATS server. SQLite is embedded; no database service is required. The [hosted setup guide](https://axmouth.github.io/natsui/setup.html) includes the same startup path. The disposable cluster demo remains unauthenticated on host loopback; this guide configures a separate authenticated dashboard.

## Docker with an existing broker

The commands below use Docker Desktop with Linux containers or Docker Engine. NATS_NETWORK and NATS_SERVICE are placeholders for an existing private Docker network and broker service name. The broker must be reachable from that network. A dedicated restricted NATS identity is described in [SECURITY.md](../SECURITY.md).

Generate an access key into a separate named volume. This command refuses to overwrite an existing file:

```sh
docker run --rm -v natsui-auth:/data ghcr.io/axmouth/natsui:latest --init-auth /data/access.key
```

Read the key locally for sign-in. This deliberately displays a secret; the output belongs in a password manager, not logs or source control:

```sh
docker run --rm --entrypoint cat -v natsui-auth:/data:ro ghcr.io/axmouth/natsui:latest /data/access.key
```

Start the dashboard with separate mounts for history and the read-only secret:

```sh
docker run -d --name natsui --network NATS_NETWORK -p 127.0.0.1:4321:4321 -e NATSUI_URL=nats://NATS_SERVICE:4222 -e NATSUI_PROFILE=production-observer -e NATSUI_AUTH_TOKEN_FILE=/run/natsui-auth/access.key -v natsui-data:/data -v natsui-auth:/run/natsui-auth:ro --read-only --cap-drop ALL --security-opt no-new-privileges:true ghcr.io/axmouth/natsui:latest
```

Open http://127.0.0.1:4321 and enter the access key. Native NATS credentials and TLS files use separate read-only mounts. An unauthenticated broker is not required; add NATSUI_CREDS or supported URL credentials for the target deployment. NATSUI_MONITOR_URLS is optional and independent of broker authentication. NATSUI_ALLOW_WRITES defaults to 0; enabling reviewed edits also requires suitable NATS permissions.

A Compose alternative for a broker reachable through host.docker.internal is available as [compose.auth.yaml](../deploy/compose.auth.yaml). It uses the same natsui-auth and natsui-data volumes. The natsui-auth volume must first be initialized by the generation command above. NATSUI_URL can override the default broker address. A broker bound only to host loopback may not be reachable through the Docker gateway; the shared private network example above avoids that limitation.

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

- NATSUI_AUTH_TOKEN_FILE enables a single-operator access-key login. An unset variable retains trusted local access; an empty, unreadable or malformed configured file fails startup.
- --init-auth generates 256 random bits encoded as 64 hexadecimal characters. It does not create a human password or a NATS credential. The process retains a SHA-256 digest for comparison, not the raw access key.
- Sign-in creates an HttpOnly, SameSite=Strict cookie. Sessions expire after eight hours, are revoked by Sign out, and are all invalidated by a process restart. At most 32 sessions are retained; the oldest is evicted when full.
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

**Mount the entire directory, not just natsui.sqlite3.** The writable volume stores observations, dashboard settings, activity, incidents and profile bindings. Message payloads are fetched on demand and are not stored by Natsui. The access key belongs in its separate secret mount; sessions are deliberately memory-only.

| Deployment | Persistent mount | Lifecycle |
| --- | --- | --- |
| Docker run above | natsui-data:/data | Survives container stop, removal and replacement while the named volume remains |
| Authenticated Compose example | natsui-data:/data | Same stable named volume; authentication uses a separate external volume |
| Root compose.yaml | Project-prefixed dashboard-data volume | Reuse the same Compose project name and file to retain the same history |
| Published cluster demo | Tryout project's dashboard-data plus three broker volumes | down retains volumes; down -v deletes demo history and broker storage |
| Native binary | NATSUI_DATA_DIR or ./data | Use a stable absolute directory when launching from different working directories |

A container writable layer is not persistent storage. Omitting the volume can lose history on replacement; the read-only root filesystem can also prevent startup without a writable data mount. A volume is persistence, not a backup. Do not run multiple dashboard processes against the same data directory. Use a local filesystem, not a shared NFS/SMB directory, for SQLite WAL mode.

The image runs as UID/GID 10001. Fresh named volumes mounted at /data inherit the image directory ownership. Bind mounts and restored directories must be writable by that UID, including directory creation of WAL/SHM files. Mount credentials read-only and ensure UID 10001 can read them. On Windows, restrict native secret/data directories with filesystem ACLs; Unix key generation creates the file with mode 0600.

Default retention is seven days and the serialized history budget is 128 MiB. NATSUI_HISTORY_MAX_MB adjusts that budget. SQLite indexes, WAL and reusable pages mean physical disk use can exceed the logical budget. Existing [profile identity safeguards](../SECURITY.md#history-identity-and-migration) remain in force after restart or restore.

## Backup and restore

The simplest supported procedure is a stopped-process copy of the complete data directory. A live copy of only natsui.sqlite3 can omit committed WAL transactions. The [SQLite backup API](https://www.sqlite.org/backup.html) is an alternative for online backups; Natsui does not yet expose a backup button. [SQLite WAL documentation](https://www.sqlite.org/wal.html) explains why the companion files matter.

The following commands use the natsui container and natsui-data volume from the Docker run example. No other process may write to that volume during the copy. The archive is written into the current directory. The backup helper runs as root to preserve ownership, with the source volume mounted read-only:

```sh
docker stop natsui
docker run --rm --user 0 --entrypoint tar -v natsui-data:/data:ro --mount "type=bind,source=${PWD},target=/backup" ghcr.io/axmouth/natsui:latest -czf /backup/natsui-data.tar.gz -C /data .
docker start natsui
```

These commands work in PowerShell, Bash and Zsh from an existing local directory. In scripts, restart only after checking whether backup succeeded; retain the prior successful archive if a new backup fails. Use timestamped archive filenames for multiple backups. Protect archives like operational data.

Restore a trusted archive into a fresh volume, avoiding an overwrite of active history:

```sh
docker volume create natsui-restored-data
docker run --rm --user 0 --entrypoint tar -v natsui-restored-data:/data --mount "type=bind,source=${PWD},target=/backup,readonly" ghcr.io/axmouth/natsui:latest -xzf /backup/natsui-data.tar.gz -C /data .
```

Start a replacement dashboard with -v natsui-restored-data:/data, the same NATS identity/profile, and the original image version or a documented compatible upgrade. Use a different container name and loopback port for a restore drill. Check /readyz, sign in, and inspect settings and history before replacing the original instance. Newer database schemas may not open in older binaries. Keep a stopped-process backup before upgrades; pin image versions or digests when repeatable restores matter.

For Compose deployments, inspect the dashboard's /data mount with docker inspect before choosing a volume name. docker compose down preserves named volumes; docker compose down -v removes non-external volumes. The external auth volume in the authenticated example is retained, but its history volume is removed by down -v. Native backups follow the same stop/copy-complete-directory/restart procedure.
