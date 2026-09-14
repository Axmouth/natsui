# Configuration and operations

A local NATS and JetStream observability dashboard with read-only access by default with inspectable history, consumer diagnostics and incident investigation. Rust serves the bundled interface; SQLite stores local settings and operational history. No Node runtime or external database is needed to run the native binary.

The local beta has no shared login or public HTTP access policy. Production NATS connections require a restricted NATS identity. The dashboard never pulls or acknowledges application messages.

## Try a real cluster

From a source checkout, Docker is sufficient for the complete demo:

```sh
docker compose -f demo/compose.yaml -f demo/dashboard.yaml up --build -d --wait
```

Open [the dashboard](http://127.0.0.1:4321). Three unmodified NATS servers and a traffic generator produce real retained records, consumers, backlogs, recoveries and process metrics. Port 4321 must be free. The existing Windows launcher remains available as `scripts/try-cluster.ps1` for a native Rust dashboard with the same containers.

```sh
docker compose -f demo/compose.yaml -f demo/dashboard.yaml down
```

Stopping preserves volumes. Adding `--volumes` permanently discards demo broker and dashboard data. See [demo lifecycle](../demo/README.md).

## Connect to an existing server

A native binary connects to localhost by default:

```powershell
$env:NATSUI_URL = 'nats://127.0.0.1:4222'
$env:NATSUI_PROFILE = 'local-development'
.\natsui.exe
```

```sh
NATSUI_URL=nats://127.0.0.1:4222 NATSUI_PROFILE=local-development ./natsui
```

The root Compose file in the source checkout runs the published dashboard for an existing broker. Its default broker address is `host.docker.internal:4222`. A broker listening only on the host's loopback may not be reachable through that Docker gateway; a shared private Docker network or the native binary avoids exposing broker ports.

```sh
docker compose up -d --wait
```

The container runs as UID 10001, with a read-only root filesystem and a writable named data volume. The published dashboard port is restricted to host loopback. Credentials and TLS files can be mounted read-only with the environment variables below. [Access and credential setup](../SECURITY.md) contains the permission template and container example.

## Build and package

Rust 1.94 or later and a platform C toolchain are required. SQLite is bundled.

```sh
cargo build --release --locked
node scripts/package.mjs
```

The binary is in `target/release`. The optional packaging script produces a platform-named archive, documentation and SHA-256 checksum under `dist-release`; it requires Node 22 or later and tar. CI prepares artifacts for Linux, Windows and macOS. Configured CI targets are not a claim that every platform has already been tested. GitHub Actions publishes native artifacts, container images and the Pages demo after verification.

## Simulation without a broker

`natsui --demo` serves a clearly labeled local simulation. `node scripts/export-demo.mjs` produces `dist-demo`, a static web demo with no backend connection. The static demo models workload phases, consumers, histories, incidents and replica placement. Node metrics, clients and subscriptions are also synthetic. The static export can be served by an ordinary static web server.

## Included

- Fibril's theme collection, subtle theme selector and pixel kitten favicon.
- Largest pending-delivery backlog, threshold crossings and acknowledgment pressure.
- Streams, consumer details and identity-aware historical trends with hover, touch and keyboard inspection.
- CPU, resident RAM, traffic, connections and subscriptions from optional native monitoring endpoints.
- Subject matching, native subscription interest, replica placement and connection inventory.
- Retained-record browsing, subject filters, newest matching record and exact sequence inspection; decoded headers and text/JSON/binary payload views.
- Persistent observed changes and allowlisted JetStream advisories, with explicit coverage limits.
- Linked investigation charts, historical windows, pause and JSON export without message payloads.
- Local settings, storage-health reporting and guarded profile history.

## Persistent storage and dashboard login

[Setup guide](SETUP.md) covers named volumes, UID 10001 permissions, access-key generation, secret rotation and stopped-process backup/restore. History and secrets use separate mounts. The cluster tryout stays unauthenticated and local.

## Configuration

| Variable | Default | Purpose |
| --- | --- | --- |
| NATSUI_URL | nats://127.0.0.1:4222 | One nats:// or tls:// server address; discovered cluster servers are handled by the NATS client |
| NATSUI_CREDS | Unset | NATS JWT credentials file; incompatible with URL authentication |
| NATSUI_TLS_REQUIRED | 0 | Set to 1 to require TLS; tls:// or configured TLS files also require it |
| NATSUI_TLS_CA | Unset | Additional PEM CA trust for the NATS connection |
| NATSUI_TLS_CERT / NATSUI_TLS_KEY | Unset | PEM client certificate and private key; both are required together |
| NATSUI_PROFILE | Local NATS | Profile label and history namespace |
| NATSUI_DOMAIN | Unset | JetStream API domain |
| NATSUI_MONITOR_URLS | Unset | Comma-separated native monitoring origins, at most 32 |
| NATSUI_PORT | 4321 | Dashboard HTTP port |
| NATSUI_DATA_DIR | data (image: /data) | Persistent SQLite directory; mount the entire directory |
| NATSUI_AUTH_TOKEN_FILE | Unset | Generated dashboard access-key file; enables session authentication |
| NATSUI_ALLOW_WRITES | 0 | Opt-in reviewed JetStream updates, subject to broker permissions |
| NATSUI_HISTORY_MAX_MB | 128 | Serialized history budget across all profiles, 16-4096 MiB |
| NATSUI_CONTAINER | 0 | Set to 1 only inside a container with host-loopback port publication |
| NATSUI_ADOPT_LEGACY_PROFILE | Unset | One-time value 1 binds verified existing unbound history to the configured connection |

Connection settings require a dashboard restart. Backlog threshold, collection interval and retention days are editable live. Profiles bind to a digest of configured endpoint, domain and authentication identity; a conflicting binding fails startup instead of mixing history. Existing pre-binding data requires explicit adoption. Credential-file or client-certificate rotation conservatively requires a new profile. URL password rotation for the same username is allowed. This is a configuration guard, not broker-attested account identity: replacing an account behind an unchanged endpoint and username still requires a new profile or data directory.

## Evidence and operational limits

- Backlog is the largest observed consumer pending count, not the sum of overlapping consumers. Filter matches indicate eligibility, not delivery or business completion.
- Collection is capped at 300 streams, 2,000 consumers and 20 seconds. Partial observations remain partial. HTTP readers use cached inventory instead of triggering scans.
- Native monitoring is optional and independent of JetStream collection. CPU and RAM describe NATS processes, not container limits. Monitoring may include accounts outside the connected JetStream identity. No Docker socket is used by the dashboard.
- Message browsing reads actual stored messages via STREAM.INFO and STREAM.MSG.GET, without creating a consumer. Pages are capped at 20 records, 8 seconds and 8 MB of record JSON; each received broker response is capped at 2 MB after receipt. Listed records can expire before inspection. Listing metadata still requires broker permission to read payloads.
- Resource history begins when collection starts. Charts use at most 240 observations; historical requests cover at most six hours and 16 MB. Identity changes, resets and missing observations create gaps.
- Advisory collection has no replay. Events before subscription, during outages or beyond the recording cap cannot be recovered. API audit and per-message NAK events are excluded. Subscription permission is not inferred from an absence of events.
- SQLite keeps the newest samples within both retention days and the serialized history budget. Incident bodies have a separate 16 MiB budget; activity retains at most 10,000 rows. Budgets span profiles. Physical database/WAL files include overhead and reusable pages, so file size can exceed these logical budgets. Deletes do not immediately shrink files.
- Storage errors remain visible even if inventory collection succeeds. A failed settings read retains the last validated settings. Restart requires readable, valid settings. Storage errors have per-operation last-success and last-failure timestamps at `/api/snapshot`.
- `/healthz` reports HTTP liveness. `/readyz` returns 503 unless inventory is complete and fresh and storage operations are healthy. Optional monitoring coverage remains separate. Neither endpoint proves application-worker health.
- SQLite backup requires the backup API or stopping the dashboard before copying the database and accompanying files. Data directory access should be restricted; history contains resource names and operational information.

## Verification

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
node scripts/test-trends.mjs
node scripts/test-demo.mjs
node scripts/test-site.mjs
```

Full integration tests require a disposable NATS executable and temporary TLS fixtures. OpenSSL is required only for fixture generation:

```powershell
$env:NATSUI_TEST_SERVER = 'C:\path\to\nats-server.exe'
$env:NATSUI_TLS_FIXTURES = node scripts/tls-fixtures.mjs
cargo test --locked -- --include-ignored
```

`NATSUI_OPENSSL` can select an OpenSSL executable. The fixture script prints its temporary directory; private keys must not be committed. The Docker verification target runs the integration tests on Linux:

```sh
docker build --target verification -t natsui:verified .
```

`node scripts/load-check.mjs` checks eight concurrent local readers. `node scripts/soak.mjs` records local API readiness, latency and optional SQLite growth for 24 hours by default. Environment variables `NATSUI_SOAK_SECONDS`, `NATSUI_SOAK_INTERVAL`, `NATSUI_SOAK_URL`, `NATSUI_SOAK_DATA_DIR` and `NATSUI_SOAK_FILE` control the run. Optional NATSUI_SOAK_PID records Windows dashboard RSS and cumulative CPU time. The JSON summary distinguishes elapsed evidence from a completed run. See [release evidence and outstanding gates](../RELEASE_CHECKLIST.md).

## Project references

- [Porting and additions ledger](../PORTING_LEDGER.md)
- [Access boundaries](../SECURITY.md)
- [Settings and future shared access](../SETTINGS_AND_ACCESS.md)
- [Metric semantics](../RESOURCE_METRICS_AND_TRENDS.md)
- [Initial assessment](../ASSESSMENT.md)

Design inspired by [Fibril](https://github.com/Axmouth/fibril). Theme styles are adapted from its admin interface. MIT attribution is preserved in LICENSE. Natsui is an independent project.

## Reviewed JetStream settings

`NATSUI_ALLOW_WRITES=1` enables the JetStream configuration section in Settings for the current connection. The default is `0`; only the dashboard needs restarting to enable this capability. Applying supported resource edits does not require a NATS restart. NATS 2.11+ in the 2.x series and native update permissions are required. The standard read-only permission template remains valid for inspection.

Settings loads a fresh resource configuration, previews changed fields, and verifies an applied update with a native INFO read. Activity retains the attempted change and outcome. Retention reductions can delete records immediately. Shared authentication and server-file editing are separate capabilities. See [editor details](../SETTINGS_AND_ACCESS.md#implemented-jetstream-editor) and [write permissions](../SECURITY.md#optional-local-configuration-editing).

Container authentication and persistence verification: build a local image and run `node scripts/smoke-auth-storage.mjs IMAGE`. The check uses disposable named volumes to verify login/logout, restart session revocation, container replacement and stopped-process backup/restore. It removes its test containers and volumes afterward and never prints generated keys or session cookies.
