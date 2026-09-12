# NATS admin dashboard assessment

Historical assessment. Current implementation status and subsequent decisions are maintained in [PORTING_LEDGER.md](PORTING_LEDGER.md), including mascot retention and SQLite persistence. This document records the original research rather than the current feature set.

Assessment date: 2026-09-11. Scope: reuse Fibril's admin UI patterns for a standalone NATS and JetStream dashboard. No application implementation is included.

## Recommendation

Proceed with a standalone Rust application that carries over Fibril's design system and operational workflows, with a new NATS-specific data layer. The presentation is highly reusable; the broker integration is largely a replacement. Treat Core NATS and JetStream as separate, connected areas of the product.

This is a source assessment, not a rendered visual review or a live NATS compatibility test. The destination directory was empty. No target NATS deployment or minimum supported server version has been selected. Online reference branches can contain newer features than a deployed release; implementation must pin and test a supported version range.

## Existing Fibril foundation

Reviewed source under `../thetube/crates/admin`:

| Source | Reuse judgment |
| --- | --- |
| `admin-ui/css/admin.css` | Strong candidate for direct extraction: dark/light palettes, accent flavors, semantic colors, compact typography, cards, tables, controls, focus styling. |
| `templates/layout.html` | Carry over the sidebar, command palette, context header, live status and theme controls. Replace branding, navigation and broker switching semantics. |
| `admin-ui/js/admin.js` | Reuse formatting, SVG charts, sparklines, hover readouts, URL filters and notification patterns after separating domain-specific helpers. |
| `admin-ui/js/api.js` | Small HTTP wrapper worth retaining conceptually, with structured error and capability handling. |
| `admin-ui/js/tendrils.js` | Drawing primitives can support NATS connection and topology diagrams. Replace mascot-specific assets and entity composition as appropriate. |
| `templates/pages/*.html` | Useful page structures, but page scripts and labels encode Fibril response schemas and queue semantics. Adapt individually. |
| `src/events.rs`, `history.rs`, `attention.rs`, `audit.rs` | Reuse architectural patterns: shared snapshots, short history, actionable conditions, activity records. Replace their data producers and rules. |
| `src/routes.rs`, `server.rs` | Substantial rewrite: handlers currently access Fibril broker, storage, partitions and runtime settings directly. |

Fibril already uses Axum, Askama, embedded assets and plain JavaScript. A frontend framework migration is not necessary for this project. Explicit page initialization and disposal would be preferable to carrying over the layout's global timer tracking unchanged.

The most valuable design principle is the connection between a symptom and its drilldown: an attention item opens the affected resource, charts explain a trend, and the resource page contains the relevant action.

## Resource model

Core NATS subjects are routing names, not durable queue objects. Queue groups distribute live deliveries among matching subscribers. They should appear under subscriptions and connections, without invented stored-message depth or replay controls. Core delivery is at most once. [NATS consumer model](https://raw.githubusercontent.com/nats-io/nats.docs/master/nats-concepts/jetstream/consumers.md)

JetStream streams store messages; consumers track delivery and acknowledgments. Present them as separate resources. A queue-oriented preset can configure a work-queue stream and shared pull consumer, but keep the resulting stream and consumer visible. Work-queue consumer filters cannot overlap; limits and interest retention serve different needs. Retention limits can still remove unprocessed data. [Stream policies](https://raw.githubusercontent.com/nats-io/nats.docs/master/nats-concepts/jetstream/streams.md)

Use environment, account and JetStream domain as explicit context. Resource identity must include this context, not just a stream name. System-level observability and application-account access are separate permissions, not interchangeable credentials.

## Page-by-page feasibility

| Fibril experience | NATS dashboard adaptation | Effort and boundary |
| --- | --- | --- |
| Overview | Server traffic, connections, storage, stream/consumer summaries, attention conditions | Medium. Rebuild metric definitions and scope labels. |
| Queues and queue detail | Streams with nested consumers; optional work-queue preset | Medium/high. Reuse tables and drilldowns, replace partitions and settlement vocabulary. |
| Plexus streams | JetStream stream configuration, retention, storage, consumer links | Medium. Different durability and replication model. |
| Message inspector | Retained stream records, sequence, subject, timestamp, headers, payload preview | Medium. Replace global status filters; payload reads are explicit and bounded. |
| Test publish | Subject publisher with optional headers and JetStream acknowledgment display | Low/medium. Core publish cannot report durable storage confirmation. |
| Connections | Per-server connection table and traffic diagram | Medium. Handle reconnect identity, pagination and bidirectional clients. |
| Subscriptions/cohorts | Subject subscriptions and queue groups; separate JetStream consumers | Medium/high. Fibril cohort membership/partition ownership does not transfer. |
| Topology | Routes, gateways, leaf links, stream replica placement | High. Reuse rendering, replace graph construction. |
| Activity | Collected advisories plus dashboard actions | Medium. Requires explicit retention and coverage indicators. |
| Dead letters/replay | Failure inbox plus configured DLQ integrations | High. An optional application subsystem. |
| Diagnostics | Slow consumers, traffic pressure, replica health, API errors | Medium/high. Fibril storage internals have no direct equivalents. |
| Settings/security | Connection profiles, dashboard access, supported resource settings | High for full administration. Server configuration and identity management require separate integrations. |

NATS monitoring supplies server, connection, route and JetStream information through endpoints including `/varz`, `/connz`, `/routez` and `/jsz`. Fetch scoped detail rather than repeatedly walking every account and consumer. The monitoring listener must remain behind an appropriate access boundary. [Monitoring endpoints](https://docs.nats.io/learn/monitoring/monitoring-endpoints)

JetStream management APIs support stream and consumer discovery, configuration, message retrieval, deletion, purge, backup and restore. Domain-aware API prefixes are supported. These are enough to build a substantial admin board without modifying nats-server. [JetStream API reference](https://raw.githubusercontent.com/nats-io/nats.docs/master/using-nats/jetstream/nats_api_reference.md)

## Semantics that must change

- **Message state is consumer-relative.** Consumer summary counts are useful, but do not reproduce Fibril's exact ready/inflight/delayed/settled classification for every stored record. Acknowledgment floors alone cannot classify all messages when acknowledgments occur out of order.
- **Browsing must not consume production work.** Use stored-message GET/direct reads. Pulling from the application's consumer changes delivery state. Creating an inspection consumer can also affect retention semantics. Sequence gaps and expired records are normal.
- **Backlog needs a named scope.** Prefer reported consumer pending and acknowledgment-pending values. Do not label stream sequence subtraction as an exact backlog, or sum overlapping consumers as unique messages.
- **Acknowledgment does not prove business completion.** Preserve the distinction in labels. NATS traffic counters also include messaging overhead and fanout; they are not automatically Fibril's published/completed counters.
- **A subject browser is an observed inventory.** Subscription interest, stream filters and optional traffic sampling provide different evidence. None alone is a permanent catalogue of every possible subject or publisher-to-subscriber relationship.
- **Worker identity is additional telemetry.** Shared pull consumer state does not provide a complete Fibril-style roster of active workers and their assignments. A worker registry or instrumentation can supply that later.
- **Unknown is not zero.** Every snapshot should carry source, scope and observation time. Missing permissions, unavailable servers and unsupported fields must remain distinguishable from empty results.

## DLQ and replay assessment

MaxDeliver emits an advisory and leaves the message in the stream; it does not automatically move it to a DLQ. BackOff and delayed negative acknowledgments provide retry controls, but not a complete operator failure workflow. [Consumer configuration](https://raw.githubusercontent.com/nats-io/nats.docs/master/nats-concepts/jetstream/consumers.md)

A useful optional failure service would persist advisories, index failures by account/domain/stream/consumer/sequence, retrieve the original when available, and capture provenance. Advisory collection can have gaps, and the original may expire before retrieval. Show these conditions explicitly. Advisories can be captured for later investigation. [Advisories and events](https://docs.nats.io/learn/monitoring/advisories-and-events)

Replay should distinguish reading history through a consumer from republishing a copy. A republish action needs an explicit destination, controlled header handling, deduplication policy, rate limit and per-item result. Copying and removing the source are separate operations and must not be represented as an atomic move. An application's existing DLQ convention should be configurable rather than replaced automatically.

## Valuable additions beyond Fibril

1. **Consumer diagnosis.** Explain a growing backlog using acknowledgment pressure, pauses, delivery configuration and observed progress. Show the supporting values rather than asserting a worker failure from one stale sample.
2. **Subject explorer.** Match wildcard subscriptions to stream capture rules and consumer filters. Label configured matches separately from observed traffic.
3. **Configuration assistance.** Presets for work queues and retained event history, with a before/after diff and explanations of retention consequences. Preserve unknown fields and identify immutable settings.
4. **Failure inbox.** Durable incident records, related advisories and replay provenance. This is the closest extension of Fibril's DLQ experience.
5. **History and comparison.** Short local history first; optional durable metrics integration, saved views and configuration snapshots later.
6. **Storage tools.** Separate KV and Object Store browsers, using their dedicated APIs rather than treating their backing streams as ordinary work queues. These are native NATS capabilities exposed by the Rust client. [async-nats](https://docs.rs/async-nats/latest/async_nats/)
7. **Version-aware publishing tools.** Per-message TTL and scheduled messages are potential additions. Documentation lists TTL support from 2.11 and scheduling from 2.12; gate by server support and stream configuration. [Stream configuration](https://raw.githubusercontent.com/nats-io/nats.docs/master/nats-concepts/jetstream/streams.md)

## Suggested architecture

Browser with extracted Fibril styling and components, served by an Axum/Askama application. An `async-nats` adapter handles account-scoped messaging and JetStream operations. A separate monitoring adapter gathers permitted server information through system services or configured private HTTP endpoints. Backend credentials stay outside browser storage.

The backend exposes its own stable view models, rather than imitating Fibril's broker API. Shared collection feeds SSE to open pages, with polling fallback. Expensive collection is demand-driven; always-on history or advisory capture is an explicit service with its own cost and retention.

Maintain a capability map per connection profile: observed server version, reachable monitoring surfaces, JetStream domain, account access and allowed operations. Timeouts should not be diagnosed as authorization failures without evidence. Authorization is enforced again at action execution; hiding a button is not enforcement.

Keep dashboard-owned users and action audit separate from NATS identities. Full NATS account/user provisioning, server reload, drain and certificate rotation require deployment-specific authority. Do not port Fibril's security or cluster-action buttons without that integration.

## Initial delivery scope

1. Extract the visual shell and reusable components. Establish one local NATS/JetStream connection profile, capability reporting and fixture data for loading, denied, stale and empty states.
2. Implement read-only overview, streams, consumers, retained-message browsing, and connections where monitoring access permits. Add lightweight history and attention conditions.
3. Add stream/consumer creation and supported edits, explicit test publish, and destructive actions with target/scope review and action records. Add pause/resume only where supported.
4. Add topology, subject exploration, KV/Object Store and durable incident collection in separately reviewable increments.
5. Add DLQ policies and replay after the application's failure conventions are defined.

This ordering delivers a useful NATS-native dashboard before taking on a job-processing framework or infrastructure control plane.

## Required validation before implementation commitments

- Choose and pin a baseline server release and Rust client version, then exercise the supported APIs against that release.
- Verify a restricted application account, an optional monitoring identity and a deployment with no monitoring access.
- Verify stream browsing leaves production consumer state unchanged across the supported retention policies.
- Exercise filtered consumers, out-of-order acknowledgments, expiry, deletion gaps and counter resets.
- Verify partial cluster responses do not appear as complete totals; avoid counting replicas as independent logical messages.
- Check the extracted UI in a visible browser for dense tables, responsive navigation, keyboard use and reduced motion.

No implementation estimate in lines or percentage is reliable from this assessment alone. The defensible conclusion is high presentation reuse, substantial page adaptation, and low direct reuse of Fibril's broker handlers.
