# Resource metrics and stream trends

Status: native node monitoring, per-stream trends and chart inspection implemented and verified against the live demo on 2026-09-12. Container/host pressure, system-account monitoring and worker instrumentation remain separate future capabilities.

## Server resources

NATS 2.11.8 `/varz` already reports process CPU (`cpu`) and resident memory in bytes (`mem`). These can be collected without Docker integration. The demo exposes one monitoring endpoint per node on loopback ports 18222-18224. Measurements taken during the live workload were:

| Server | Reported process CPU | Resident memory |
| --- | --- | --- |
| demo-1 | 9 | 95.6 MiB |
| demo-2 | 6 | 84.7 MiB |
| demo-3 | 12 | 107.8 MiB |

These are point-in-time readings, not resource guarantees or a synchronized benchmark. CPU normalization must be documented before presenting capacity percentages; neither a reported core count nor GOMAXPROCS establishes a container CPU quota.

JetStream memory/storage counters describe a different resource. All three nodes reported zero JetStream memory-store usage while their processes occupied RAM, because these streams use file storage. JetStream's configured memory-store maximum is not a process or container memory limit.

Docker stats independently reported approximately 84-112 MiB against a 256 MiB limit per demo container. Container accounting and process RSS need not agree. The Docker CLI subtracts some cache from Linux memory usage. Container limits, throttling, OOM events and host pressure belong in an optional infrastructure integration, with explicitly named sources. Standard operation should not require the Docker socket.

Implemented first increment: backend collection of an explicit list of monitoring endpoints, projected into an allowlist of metrics; per-server CPU and RSS mini charts; timestamp and unavailable states; SQLite history keyed by profile, server identity and startup time. Monitoring failure must not hide working JetStream inventory. Optional authenticated system-account telemetry can follow for deployments that do not expose private HTTP monitoring.

Sources: [NATS 2.11.8 metric definitions and process sampling](https://github.com/nats-io/nats-server/blob/v2.11.8/server/monitor.go), [Docker statistics accounting](https://docs.docker.com/reference/cli/docker/container/stats/), [NATS system telemetry](https://docs.nats.io/learn/monitoring/advisories-and-events).

## Per-stream trends

The existing stream/consumer scans already contain the required values for useful first graphs. No additional application polling or message consumption is needed.

| Trend | Evidence | Meaning |
| --- | --- | --- |
| Retained records | Stream state messages | Current retained inventory; retention policies affect the curve |
| Stored bytes | Stream state bytes | Logical stream storage, not host disk usage |
| Largest consumer backlog | Maximum num_pending within the stream | Worst observed pending-delivery count; never a sum of overlapping consumers |
| Estimated append rate | Delta of last_seq divided by elapsed sample time | Sequence progress estimate for ordinary streams; not an exact count of producer attempts |

List presentation: a compact retained-record sparkline per stream, a clearly named metric, the latest count and a consistent time window across rows. The detail view adds separate backlog, storage and estimated append-rate charts. A flat line at a retention limit must not imply no traffic. In the demo, ORDERS reached 100,000 retained records while its last sequence continued advancing.

Compact per-stream counters and node measurements are stored in SQLite alongside aggregate summaries. The largest consumer backlog is projected per stream; per-consumer pending, acknowledgment, redelivery and delivery-sequence histories are now also recorded. Older aggregate-only samples cannot reconstruct per-stream curves. Match stream identity using profile/account/domain, name and creation timestamp. Recreated streams, sequence discontinuities and collection gaps break rate lines. Estimate rates only when identity and consecutive observations are valid; unusual sequence-changing operations and mirrored streams need explicit handling before enabling the estimate. Missing samples are not zero traffic. Acknowledgment progress is not proof of business completion.

Per-stream charts cannot truthfully show per-stream CPU/RAM from server process metrics. Shared server resources remain in the node view. The same time axis can support investigation without claiming that correlation identifies the cause.

## Additional native metrics worth exposing

The following candidates were identified from NATS 2.11.8 monitoring and current JetStream observations. The implemented subset is listed in the implementation section below; remaining candidates retain their source and interpretation boundaries.

| Priority | Metric | Surface | Useful interpretation and boundary |
| --- | --- | --- | --- |
| First | Per-node CPU and resident memory trends | /varz | Resource use of the NATS process; no per-stream attribution |
| First | Stream retention occupancy | Stream config and state | Retained bytes/records against configured limits; only calculate percentages for positive finite limits |
| First | Replica current/offline status and lag | Stream and consumer cluster information | Replication evidence per resource; leader location and missing peer observations remain explicit |
| First | Server connections and connection churn | /varz | Current count plus delta of lifetime connections; reset on server restart |
| First | Pending outbound bytes and connection RTT | /connz | Identify clients with queued output; transport RTT is not end-to-end application latency |
| First | Traffic rates per server | /varz message/byte counters | Derive from counter deltas and actual elapsed time; includes protocol/application traffic, not an exact business throughput measure |
| Next | Slow-consumer counter changes | /varz | Server-detected transport slow consumers; distinct from a JetStream consumer backlog |
| Next | JetStream storage usage and reservations | /varz or /jsz | Server JetStream budgets and allocations, separate from filesystem free space |
| Next | JetStream API request/error rate | /jsz or /varz JetStream stats | Includes management requests and expected errors; not a count of failed jobs |
| Next | Uptime, version and configuration reload time | /varz | Establish restart and configuration context for trend changes |
| Next | Routes and link RTT/pending output | /routez | Inter-server transport; route-pool count is not the number of unique peer servers |
| Next | Named clients, SDK version and subscription inventory | /connz | Useful connection inventory; collect bounded detail on demand, with account/access boundaries |

Read-only verification found native connection names, language/version, transport RTT and pending bytes on the live cluster, plus route RTT and lifetime traffic counters. Resource state already contains retention limits and replica information. These fields require a capability-aware adapter and honest labels, not application instrumentation.

Host free disk space, CPU throttling, cgroup memory pressure, OOM events and business execution errors remain outside these native summaries. A container or infrastructure adapter can add the first group later. Optional worker instrumentation is needed for application outcomes.

## Chart inspection

The existing aggregate backlog chart now supports a crosshair and exact recorded sample details: timestamp, pending count, leading consumer/stream, awaiting acknowledgments and collection status. Keyboard arrows, Home/End and Escape complement pointer and touch input. Selection is kept by timestamp across polling refreshes. Missing collection intervals are identified as gaps rather than interpolated healthy values. This interaction also applies to the per-stream and node charts.

## Implemented observability views (2026-09-12)

Nodes now lists process CPU, resident memory, connections, subscriptions and cumulative slow-consumer detections. Node drilldowns include inspectable CPU/RSS, inbound/outbound message-rate, new-connection-rate and JetStream API-error-rate charts. JetStream storage budget usage is separate from RSS. Connection detail is fetched on demand, capped at 100, and projects client identity, SDK, transport RTT, pending output and subscriptions without returning credentials or addresses.

Streams now includes retained-record sparklines. Stream drilldowns show retention occupancy, observed replication state, retained records, logical bytes, maximum consumer backlog and qualified append-rate estimates. Detail configuration remains available. Rate estimates require matching creation/start identities and consecutive observations; missing values, counter resets and long gaps break the calculation.

Resource history is stored as compact projections in the existing SQLite sample body. No destructive schema migration is required. Older aggregate samples remain readable but cannot supply per-resource history. Historical charts retain at most 240 samples within a 16 MB response budget in their view; database retention follows the configured days. Resource storage grows with the number of observed streams and nodes.

The HTTP collector is optional and independent of JetStream inventory. Configure `NATSUI_MONITOR_URLS` as up to 32 comma-separated HTTP(S) origins, without paths, query strings or embedded credentials. Requests use three-second timeouts, a two-megabyte response cap and no redirects or environment proxy. Each configured node is sampled once per five seconds by the backend, regardless of browser count. Projected monitoring samples are persisted at the dashboard collection cadence. Endpoint access belongs behind a private access boundary. System-account collection and Docker/container-limit integration remain future work.

Per-node errors show unavailable state without healthy zero replacements. The last known node identity remains available for historical drilldowns. The standalone simulation includes synthetic stream history but does not fabricate node monitoring; its Nodes page reports monitoring as unconfigured.

The investigation workspace links consumer, stream and node charts with a common time range and cursor. Paused snapshots retain their observations while normal collection continues. Incident markers use recorded source timestamps, and historical window requests expose sample truncation explicitly.
