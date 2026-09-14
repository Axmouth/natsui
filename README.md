# Natsui

**See what is happening in NATS and JetStream.**

A local dashboard with read-only access by default for consumer backlogs, stream storage, node resources and incident investigation. Inspect a point on a graph, follow it to a consumer, and check what is actually retained in the stream.

[![Verify and package](https://github.com/Axmouth/natsui/actions/workflows/verify.yml/badge.svg)](https://github.com/Axmouth/natsui/actions/workflows/verify.yml)
[Website](https://axmouth.github.io/natsui/) | [Browser demo](https://axmouth.github.io/natsui/demo/) | [Docker image](https://github.com/Axmouth/natsui/pkgs/container/natsui)

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

![Natsui overview showing consumer backlogs and recorded history from a real three-node NATS cluster](docs/screenshots/overview.png)

*Real NATS data, generated demo traffic. Screenshots use the included three-node cluster.*

## What it helps answer

- **Which consumer is falling behind?** Compare pending deliveries, acknowledgment pressure and delivery-attempt trends. Backlog uses the least caught-up observed consumer, without adding overlapping consumers together.
- **What changed around that time?** Follow recorded changes and JetStream advisories into a shared investigation window. Inspect timestamps, pause the view and export the evidence.
- **Is the message still there?** Browse retained records, filter by subject or look up an exact sequence. Inspection never pulls from or acknowledges an application consumer.
- **Where is the pressure?** See per-node CPU, resident RAM, connections, subscriptions and traffic, alongside stream storage trends and replica placement.
- **What matches this subject?** Explore stream capture, consumer filters and observed subscription interest. A filter match indicates eligibility, not proof of delivery.

Reported values, derived metrics and unavailable evidence stay distinct. Gaps remain gaps. CPU and RAM describe NATS processes, not container capacity.

<details>
<summary><strong>Consumer diagnostics and inspectable history</strong></summary>

![Consumer detail with pending deliveries, acknowledgment pressure and historical charts](docs/screenshots/consumer.png)

</details>

<details>
<summary><strong>Node trends and light themes</strong></summary>

![Light-themed node view with CPU, RAM, connections and subscription trends for three real NATS processes](docs/screenshots/nodes.png)

</details>

## Try a real cluster

Docker Desktop with Linux containers, or Docker Engine with Compose 2.34+, is enough. This command works in PowerShell, Bash and Zsh:

```sh
docker compose -f oci://ghcr.io/axmouth/natsui-demo:latest up -d --wait
```

Open **[localhost:4321](http://localhost:4321)**. No clone, local build or language toolchain is required. The bundle starts three unmodified NATS servers, Natsui and demo traffic. Real streams, consumers and Core NATS traffic move through steady, slow-consumer, burst and recovery phases. AMD64 and ARM64 container images are published.

Stop the demo while preserving its data:

```sh
docker compose -f oci://ghcr.io/axmouth/natsui-demo:latest down
```

The start command resumes it. Adding `-v` to the stop command deletes this demo's broker volumes and dashboard history. Port 4321 must be available. `NATSUI_DEMO_PORT` selects another dashboard port. The unauthenticated demo stays on an isolated Docker network, with only the dashboard published on host loopback. Reviewed configuration edits are enabled for the demo.

[Download the standalone Compose file](https://axmouth.github.io/natsui/try/compose.yaml) for Compose 2.23.1+ and run `docker compose -f compose.yaml up -d --wait`. [Demo setup and lifecycle](demo/README.md) covers source builds, workload phases, ports and cleanup.

## Connect an existing NATS server

The standalone image runs alongside an existing NATS service on its Docker network:

```sh
docker run -d --name natsui --network NATS_NETWORK -p 127.0.0.1:4321:4321 -e NATSUI_URL=nats://NATS_SERVICE:4222 -v natsui-data:/data --read-only --cap-drop ALL --security-opt no-new-privileges:true ghcr.io/axmouth/natsui:latest
```

`NATS_NETWORK` and `NATS_SERVICE` identify the existing network and broker. Natsui requires no Docker socket access. The root [compose.yaml](compose.yaml) also supports a broker on the host through `host.docker.internal`.

Alternatively, build the embedded native binary with Rust 1.94 or later and a C toolchain:

```sh
cargo build --release --locked
NATSUI_URL=nats://127.0.0.1:4222 NATSUI_PROFILE=local ./target/release/natsui
```

<details>
<summary>PowerShell</summary>

```powershell
cargo build --release --locked
$env:NATSUI_URL = 'nats://127.0.0.1:4222'
$env:NATSUI_PROFILE = 'local'
.\target\release\natsui.exe
```

</details>

The binary serves its own UI and stores history in SQLite. No Node runtime, separate database service, broker plugin or Docker socket is required. The root `compose.yaml` also packages the dashboard for an existing broker.

Use a dedicated restricted NATS identity. [Security and permissions](SECURITY.md) includes the permission template, credentials-file setup, private CA and mutual TLS options. `NATSUI_URL` also accepts comma-separated server addresses for the same cluster/account. Startup uses the first successful authenticated connection and retains the other seeds for reconnects. Optional `NATSUI_MONITOR_URLS` lists separate HTTP endpoints for per-node process and connection monitoring.

[Full configuration, storage limits and troubleshooting context](docs/OPERATIONS.md)

## Dashboard login and persistent history

The image stores SQLite in `/data`. **Mount the whole directory** with `-v natsui-data:/data`. Mounting only the database file omits SQLite's WAL/SHM companions. Named volumes survive container replacement. A volume is not a backup, and `docker compose down -v` deletes non-external project volumes.

`NATSUI_AUTH_TOKEN_FILE` enables an access-key login with eight-hour sessions and sign-out. `natsui --init-auth PATH` securely generates the key file without overwriting an existing file. Store it in a separate read-only mount. Missing or malformed configured keys stop startup. An unset variable retains trusted local access. This is single-operator authentication, not individual accounts or roles, and it does not enable public HTTP exposure.

The [Pages setup guide](https://axmouth.github.io/natsui/setup.html) covers cluster network discovery, multiple monitoring hosts, authenticated Docker startup, JWT credentials and mutual TLS with downloadable Compose files. [Detailed setup, volume ownership, backup and restore](docs/SETUP.md) includes exact commands, read-only credential mounts, connection checks and key rotation. The one-command cluster tryout remains an explicitly unauthenticated local demo.

## A demo without a backend

`natsui --demo` runs a clearly labeled local simulation. For a browser-only version:

```sh
node scripts/export-demo.mjs
```

The hosted [browser demo](https://axmouth.github.io/natsui/demo/) models workload phases, consumer histories, retained records and incidents without connecting to NATS. Synthetic node CPU/RAM histories, connection turnover, traffic counters and subscriptions illustrate the monitoring views without measuring real processes. A Content Security Policy disables network connections from the demo.

`dist-demo` can also be served by any static web server. `node scripts/build-site.mjs` builds the landing page, demo and downloadable Compose file together in `dist-site`.

## Status

**Local operator beta.** Stream and consumer inspection, node monitoring, retained-record browsing and investigation history are implemented. Settings includes opt-in, reviewed JetStream configuration edits. Optional single-operator login is implemented. Individual accounts, roles, public HTTP deployment and server configuration control remain outside the current access model.

The integration suite has passed on Windows and Linux with NATS Server 2.11.8, including TLS, denied operations, reconnects and bounded large inventories. CI also targets macOS. Support claims depend on successful runner results. The first endurance recording was interrupted and is not a completed 24-hour qualification. [Release evidence and remaining gates](RELEASE_CHECKLIST.md)

## Development and ideas

```sh
cargo test --locked
node scripts/test-trends.mjs
node scripts/test-demo.mjs
```

Full disposable-broker tests and Docker verification are described in [operations](docs/OPERATIONS.md#verification). [Issues](https://github.com/Axmouth/natsui/issues) are a place for reproducible bugs, confusing evidence and operational questions the dashboard should help answer. Reports should omit credentials and private message payloads.

The [porting ledger](PORTING_LEDGER.md) records which Fibril ideas were adapted, changed or left out, and which NATS-specific capabilities were added.

## Acknowledgments

Inspired by [Fibril](https://github.com/Axmouth/fibril), with its theme collection and incident-oriented interface as the starting point. Original MIT attribution is preserved in [LICENSE](LICENSE). Natsui uses its own pixel kitten favicon and is an independent project, not an official NATS project.

### Reviewed configuration editing

Settings includes stream storage limits, capture subjects, discard behavior, replica count and consumer acknowledgment/retry controls. Connections remain read-only unless started with `NATSUI_ALLOW_WRITES=1` and suitable NATS permissions. Changes have a before/after preview, stale-configuration checks, readback verification and an Activity record. Live edits require NATS 2.11+ in the 2.x series. [Settings and boundaries](SETTINGS_AND_ACCESS.md#implemented-jetstream-editor).
