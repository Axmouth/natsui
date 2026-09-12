# Try a real cluster

This Compose project is the basis for the local "try a cluster" experience. Traffic follows a scripted scenario, but streams, replication, delivery, acknowledgments, retries, retained records and dashboard history are real.

## Start

Docker Desktop must be running with Linux containers. Rust is required for the local dashboard. The first run downloads the pinned NATS image and builds the small Python traffic image.

From the repository root in PowerShell:

```powershell
.\scripts\try-cluster.ps1
```

Open http://127.0.0.1:4321/#overview. An existing dashboard on that port must be stopped first, or use `-Port 4322`. The launcher starts the dashboard without `--demo` and configures all three private monitoring endpoints. Its profile is `Demo cluster / real NATS` and its SQLite data is separate under `data/live-demo`.

For other shells, start the infrastructure with `docker compose -f demo/compose.yaml up --build -d --wait`, then run natsui with `NATSUI_URL=nats://127.0.0.1:14222`, `NATSUI_PROFILE=Demo cluster / real NATS`, and `NATSUI_DATA_DIR=data/live-demo`. Leave `NATSUI_CREDS` and `NATSUI_DOMAIN` unset for this isolated cluster.

## Workloads

| Resource | Behavior |
| --- | --- |
| ORDERS | File-backed stream, three replicas; independent billing and fulfillment consumers |
| PAYMENTS | File-backed stream, three replicas; settlement and receipt consumers |
| JOBS | Three-replica work-queue stream; two workers share one durable consumer |
| demo.live.events | Core NATS subject with two subscribers in a queue group |

The 120-second cycle targets the following rates. Actual throughput depends on scheduling, acknowledgments and the host.

| Seconds | Phase | Orders published/s | Billing delivery budget/s |
| --- | --- | --- | --- |
| 0-19 | Steady traffic | 200 | 260 |
| 20-39 | Slow billing | 200 | 20 |
| 40-59 | Producer burst | 800 | 40 |
| 60-119 | Recovery | 100 | 1000 |

Payment and job workloads also change rates. Selected first deliveries are negatively acknowledged with a delay, producing real retries. Producers wait for JetStream publish acknowledgments. Consumer workers hold batches briefly before acknowledging. Counts are never written into dashboard fixtures.

The generator logs its phase and observed stream/billing counts every five seconds:

```powershell
docker compose -f demo/compose.yaml logs -f traffic
```

The current dashboard shows JetStream resources and backlog history. Nodes now shows process resources and server traffic history, with on-demand client connection details. Core queue groups, per-producer rates and the full cluster topology remain future UI work. The scenario phase is currently available in generator logs. The badge reads `Live / read-only`; the dashboard itself does not consume application work.

## Lifecycle and isolation

Only demo containers and volumes are managed by this Compose project. No existing NATS deployment is configured. Host ports bind to loopback: clients 14222-14224, monitoring 18222-18224. Authentication is disabled for the isolated local demo; this is not a production deployment template.

Each stream retains at most 100,000 records, 64 MiB or 10 minutes. Job acknowledgments remove work-queue records sooner. Server storage, container memory and Docker logs are bounded. Named volumes preserve data across container restarts. The traffic generator restarts its cycle when restarted and reuses its resources without purging data.

Ctrl+C stops the local dashboard. Containers continue running until explicitly stopped:

```powershell
docker compose -f demo/compose.yaml stop
```

Remove the demo containers and network while retaining broker volumes:

```powershell
docker compose -f demo/compose.yaml down
```

A clean reset uses `docker compose -f demo/compose.yaml down -v`, which deletes only this project's broker volumes. Dashboard history remains in `data/live-demo`. The browser-only simulation and `--demo` do not require these containers.

## Implementation references

The generator uses the official [NATS Python client's JetStream API](https://github.com/nats-io/nats.py/tree/v2.11.0). The cluster uses standard [JetStream clustering configuration](https://docs.nats.io/running-a-nats-service/configuration/clustering/jetstream_clustering).

## Verified locally

On 2026-09-12, all three NATS containers and the traffic heartbeat passed health checks. Native stream information reported three replicas with both follower replicas current. The first observed billing backlog rose to 18,145 and later drained to about 200; SQLite history captured four samples above the 10,000 threshold. Stream GET returned a real generator payload. Rebuilding/restarting the generator reused the same streams and durable consumers successfully. Browser verification showed the live profile, consumer state and backlog history.


## Complete container demo

The dashboard can run alongside the broker and generator without a local Rust toolchain:

```sh
docker compose -f demo/compose.yaml -f demo/dashboard.yaml up --build -d --wait
```

The dashboard is published only on 127.0.0.1:4321 and uses its own dashboard-data volume. A native dashboard already using that port must be stopped first. The matching down command uses both Compose files and preserves volumes unless --volumes is explicitly supplied.
