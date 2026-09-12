# Fibril port and additions ledger

Last updated: 2026-09-12. This document describes the actual repository state. An implemented row is not a claim of production readiness. Update this ledger in the same change as each feature; AGENTS.md makes that a project convention.

Status vocabulary: **Implemented**, **Adapted**, **Planned**, **Optional integration**, **Not ported**. Planned and optional rows are not available UI features. Verification appears at the end.

## Product decisions

- General-purpose NATS operations, informed by real incidents without company-specific subjects, naming rules or infrastructure assumptions.
- Fibril's design principles and themes are retained. Branding uses a new pixel kitten.
- NATS is authoritative for messaging state. SQLite owns dashboard settings, observations and activity.
- Largest consumer backlog and number of consumers above a configurable threshold replace ambiguous summed backlog totals.
- Reported state, derived interpretation and missing evidence remain distinguishable.
- Standard mode uses an existing NATS deployment. Managing a NATS process is a separate opt-in integration.
- Initial application is a loopback-only Rust server with bundled static assets. No account system is implied by the local operator mode.

## Original Fibril admin aspects

Source baseline: `../thetube/crates/admin` and `../thetube/website/src/content/docs/admin-dashboard.md`.

| ID | Original aspect | Status | Current form or intended replacement | Reason / boundary |
| --- | --- | --- | --- | --- |
| F01 | Dark/light design system and semantic colors | Adapted | Original CSS retained; compact accent-dot picker with named swatches and adjacent light/dark toggle; preferences persist locally; Original swatch matches its salmon accent in dark and light modes | Preserve visual identity and reuse proven presentation. |
| F02 | Mascot, favicon and brand character | Implemented | Pixel kitten favicon; text-only natsui wordmark; large mascot omitted | Explicit product identity choice. Busy traffic does not imply distress. |
| F03 | Named accent flavors | Adapted | All eight Fibril flavors available through a compact accent-dot picker | Preserves the collection without adding a large settings control. |
| F04 | Sidebar and command palette | Adapted | Native NATS navigation and Ctrl/Cmd-K page search | Resource vocabulary changes; navigation principles stay. |
| F05 | Per-broker switcher | Planned | Environment/account/domain context; one configured profile currently | A standalone dashboard should retain context without jumping between broker websites. |
| F06 | Live status and freshness | Adapted | Snapshot time, complete/partial/unavailable status, disconnected browser warning | External collection can fail independently of the broker. |
| F07 | Shared SSE with polling fallback | Planned | Shared backend collection implemented; browser currently polls cached state | Multiple browsers do not multiply NATS scans. SSE is an incremental transport improvement. |
| F08 | Curated attention panel | Adapted | Backlog threshold and 80% acknowledgment-pressure rules with drilldowns | Derive from exposed evidence; do not assert a worker root cause. |
| F09 | Desktop attention notifications | Planned | Optional notification preference | Requires persisted condition transitions and noise control first. |
| F10 | Global backlog and inflight cards | Adapted | Maximum pending-delivery backlog; count above threshold; acknowledgment counts per consumer | Avoid counting overlapping consumer backlogs as unique messages. |
| F11 | Publish/delivery/completion rates | Partially adapted | Native server traffic, qualified stream append estimates and consumer delivery-attempt rates | Attempts include redelivery. Exact completion and acknowledgment throughput are not inferred from sequence floors. |
| F12 | Short broker-memory chart history | Adapted | SQLite summaries, configurable retention; latest 240 samples drawn as SVG with exact time/value/consumer readout, crosshair, keyboard and touch inspection | Retains observations across restarts; partial and missing samples break chart lines. |
| F13 | Per-resource charts and sparklines | Adapted | Stream, node and consumer histories, inspectable list trends and dedicated detail pages | Identity changes and missing samples break histories and rates. |
| F14 | CPU, memory, disk and reconnect overview | Adapted | Optional /varz collector, node resource/rate charts and on-demand connection transport; Docker limits remain separate | Native process metrics need no Docker access. Container and host pressure require optional infrastructure telemetry. See RESOURCE_METRICS_AND_TRENDS.md. |
| F15 | Disk usage breakdown | Adapted | Logical bytes per stream, plus observed aggregate | These values are not filesystem utilization or replica-inclusive disk totals. |
| F16 | Queue list and partition detail | Adapted | Stream inventory, separate consumer inventory, config detail dialogs | Native NATS entities replace Fibril queue partitions. |
| F17 | Search and URL-persistent filters | Implemented | Stream/consumer search and above-threshold filter encoded in URL fragment | Shareable operational context. |
| F18 | Queue hide-inactive toggle | Planned | Relevant consumer filters after inactivity semantics are defined | Durable consumers may intentionally be idle. |
| F19 | Queue creation/deletion | Planned | Native stream/consumer creation, reviewed deletion, optional work-queue preset | Current adapter is read-only. |
| F20 | Queue idle loading/eviction/skip reasons | Not ported | No equivalent field or placeholder | Fibril storage implementation detail unavailable through standard NATS APIs. |
| F21 | Oldest ready-message age | Optional integration | Retained age can be added; exact outstanding age needs adequate evidence | Oldest retained is not necessarily oldest pending for a consumer. |
| F22 | Test publish | Planned | Explicit Core publish versus JetStream storage acknowledgment | Never label publish success as verified business processing. |
| F23 | Follower checkpoint/recovery internals | Not ported | Native replica matrix shows leaders, current/offline followers and lag instead | Fibril internal checkpoint/recovery states have no equivalent public NATS fields. |
| F24 | Plexus stream inventory and durable cursors | Adapted | Stream retention/configuration and consumer pending/ack values | Native JetStream state replaces Plexus semantics. |
| F25 | Plexus durability tiers and live-buffer recovery | Not ported | Native storage/replication fields shown; no mapped durability tier | Avoid pretending implementations have equivalent guarantees. |
| F26 | Retained-message inspector | Adapted | Paged retained-record browsing, subject filters, newest matching record, exact sequence lookup and payload dialog through STREAM.MSG.GET | Does not pull or acknowledge production consumer work. |
| F27 | Optional payload previews and bounded reads | Adapted | On-demand browse capped at 20 records, 8 seconds and 8 MB of processed record JSON; per-response cap 2 MB. List responses omit payloads; the native lookup still reads payloads from NATS. Payload dialog requires explicit inspection; no SQLite payload storage | Debugging should not become automatic message replication. |
| F28 | Exact ready/inflight/delayed/settled/DLQ per-record filters | Not ported | Consumer summary counters; optional event evidence later | Public consumer information is not a complete per-record lifecycle. |
| F29 | Topic/group suggestions in forms | Adapted | Stream selector, capture rules and consumer filter suggestions use observed inventory | No guessed resource names needed for inspection. |
| F30 | Native global/per-queue DLQ policies | Optional integration | Configurable application DLQ conventions and failure collector | MaxDeliver advisories do not move records automatically. |
| F31 | Replay selected to original source | Optional integration | Explicit destination/provenance, header policy and per-item results | Arbitrary messages do not carry sufficient source metadata; copy/delete is not atomic. |
| F32 | Activity diary | Adapted | Searchable incident timeline, selected configuration diffs, threshold changes, observed restarts and received NATS advisories | Persistent SQLite events include source, resource and gaps; missed advisories are not replayed. |
| F33 | Runtime settings with editability flags | Adapted | Editable dashboard settings; clear resource/server ownership boundaries | Only dashboard settings are writable in this build. |
| F34 | Version-checked settings saves | Planned | Application revision checks and native-resource diff/revalidation | A local lock cannot guarantee atomic exclusion of external NATS changes. |
| F35 | Admin users, password rotation and logout | Planned | Dashboard-local users with viewer/operator/admin roles | Shared deployment requires authentication; initial server binds only to loopback. |
| F36 | NATS/broker user administration | Optional integration | Separate NATS identity adapter for config-based or JWT deployments | Dashboard users are not NATS users. Authority depends on deployment. |
| F37 | Served certificate inspection and reload | Optional integration | Dashboard TLS separately; NATS certificates only with deployment access | External dashboard does not own NATS files or certificates. |
| F38 | First-boot broker TLS setup | Not ported | Future connection onboarding, not broker startup gating | Standard mode attaches to existing infrastructure. |
| F39 | Connections table and traffic diagrams | Partially adapted | Searchable cross-node connections, client/SDK details, rates, RTT, pending output and subscriptions | Paged on demand, 100 per endpoint. Animated traffic diagrams remain unimplemented. |
| F40 | Subscriptions and queue-group inspection | Adapted | Subject explorer includes paged native subscriptions, account, queue group and connection identity | Monitoring scope may exceed the JetStream profile. Interest does not prove delivery. |
| F41 | Cohort worker targets and partition assignments | Optional integration | Optional worker registry/telemetry | Broker consumer state does not provide Fibril's assignment model. |
| F42 | Animated topology and placement matrix | Partially adapted | Stream replica placement matrix with leader and follower state | Placement is reported per stream; animation and inter-node route topology remain planned. |
| F43 | One cluster consensus leader | Not ported | Show appropriate metadata/stream/consumer leadership separately | JetStream has multiple consensus groups. |
| F44 | Drain, repartition and voting-member actions | Optional integration | Native, version-aware operational workflows where supported | No Fibril-style handoff guarantee or repartition abstraction is assumed. |
| F45 | Quarantine repair/log truncation | Not ported | Health evidence and recovery guidance only | No equivalent safe public repair operation. |
| F46 | Process health endpoint | Adapted | /healthz reports dashboard liveness; collection status remains separate | Dashboard running does not imply NATS is healthy. |
| F47 | Storage command lanes, append/snapshot/recovery counters | Not ported | Native NATS diagnostics when available | Fibril-specific instrumentation. |
| F48 | Global timer interception during navigation | Not ported | One explicit browser refresh loop and simple page lifecycle | No need to copy a navigation workaround. |
| F49 | Vendored local assets and no external requests | Implemented | CSS, JavaScript and pixel kitten embedded in Rust binary | Portable and independent of external asset services. |

## Additions not present in the original Fibril admin

| ID | Addition | Status | Form and reason |
| --- | --- | --- | --- |
| N01 | NATS management adapter | Implemented | Native request/reply API; paged stream/consumer scans with time and inventory caps. |
| N02 | SQLite persistence | Implemented | Settings, compact historical summaries and activity; WAL and retention cleanup; no payload persistence. |
| N03 | Evidence and coverage vocabulary | Implemented | Reported/derived tags, coverage page, partial/unavailable states and explicit fixture banner. |
| N04 | Largest consumer backlog and threshold summary | Implemented | Maximum pending deliveries and count strictly above configured threshold. |
| N05 | Configurable threshold, collection cadence and retention | Implemented | Local SQLite settings applied without NATS restart. |
| N06 | Explicit demo mode | Implemented | --demo simulation stays separate from the live Docker demo; the latter uses native JetStream observations and its own SQLite directory. |
| N07 | Native-versus-managed settings distinction | Implemented | Settings page explains ownership; no hidden process controller. |
| N08 | Shared-workspace roles | Planned | Viewer: observations; operator: approved messaging actions; admin: users/profiles/deployment settings. Fibril has users; role separation is the addition. |
| N09 | Durable advisory/incident collector | Implemented | Selected advisory fields persisted to SQLite, with recording caps and connection-gap notices. Routine API audits and individual NAK events are excluded. No broker-side replay stream is created. |
| N10 | Configuration snapshots and incident correlation | Partially implemented | Operational-setting diffs between complete observations, threshold changes and startup/reload identity changes; resource/time drilldowns |
| N11 | Subject relationship explorer | Implemented | Literal subject lookup shows configured capturing streams, matching consumers on those streams and on-demand live subscription interests with account context. No observed message-path claim. |
| N12 | KV and Object Store views | Planned | Dedicated semantics, not generic edits to backing streams. |
| N13 | Optional worker telemetry contract | Optional integration | Generic worker identity and error evidence without a mandatory SDK or company-specific schema. |
| N14 | Managed deployment controller | Optional integration | Validate, diff, write, reload/restart and verify only explicitly managed servers. See SETTINGS_AND_ACCESS.md. |
| N15 | Historical comparison and saved views | Partially implemented | Shared time window, six linked stream/consumer/node charts, pause, incident markers, historical range reads and local JSON export. Named saved views remain planned. |

| N16 | Reusable real-cluster demo | Implemented | Three Docker NATS nodes, replicated streams, shared work-queue workers, independent consumers, Core queue group and scripted slowdown/burst/recovery. PowerShell launcher and lifecycle documentation in demo/README.md. Actual throughput and backlog are observed, not fabricated. |

| N17 | Native metric discovery | Partially implemented | Prioritized CPU/RSS, retention occupancy, replication, connections, traffic and transport pressure. Feasibility and interpretation boundaries in RESOURCE_METRICS_AND_TRENDS.md; Native resource views implemented as documented; route topology, infrastructure pressure and worker outcomes remain separate work. |

## Attribution

Design attribution appears in the README and About page. MIT license credit remains in LICENSE because theme styles are adapted from Fibril. The primary footer links to About without persistent co-branding.

## Verification and next increment

Seventeen Rust tests pass, including monitoring failure isolation, response bounds and live NATS 2.11.8 checks for overlapping filtered consumers, maximum-not-sum backlog and non-consuming message inspection. Browser verification covers filters, drilldowns, inspection, settings changes and theme/layout behavior. Full verification and remaining test gaps are recorded in README.md.

Current priority: local read-only beta hardening and release evidence. Native resource edits require before/after review. Shared user management precedes externally accessible deployment. A deployment supervisor is not implemented or installed by this change.

### Real-cluster demo verification (2026-09-12)

Three NATS 2.11.8 containers and the traffic container passed health checks. All replicated streams reported current followers. The dashboard recorded real backlog growth to 18,145 followed by recovery to about 200, with threshold crossings in SQLite history. Retained-message inspection returned an actual producer payload. Generator restart preserved resources. Core traffic exists; connection and queue-group views were added in the six investigation additions below. See demo/README.md.


### Resource view verification (2026-09-12)

Live browser checks verified three-node inventory, CPU/RSS histories, stream sparklines, retention and replica details, exact historical readouts and on-demand client RTT/pending-byte data. SQLite resource samples survived dashboard restart. JavaScript tests cover rates, identity changes, counter resets, duplicate monitoring samples and missing observations. Rust tests cover bounded HTTP responses, rejected redirects, independent endpoint failure and existing live NATS inspection. Clippy passes with warnings denied.

### Inspectable lists and retained records (2026-09-12)

Stream and node mini charts support hover, touch and keyboard inspection with timestamp/value popups outside the table clipping boundary. Node-list sparklines cover CPU, resident RAM, connections and subscriptions. Chart hosts remain stable across polling; missing observations remain gaps.

The message browser reads up to 20 actual records through standard STREAM.MSG.GET with seq and next_by_subj. This skips deleted sequence gaps without creating a consumer. A newest-matching shortcut uses last_by_subj. Exact lookup distinguishes the explicit no-message response from upstream failures. Reads are not atomic snapshots: retention or work-queue acknowledgments can remove a listed record before payload inspection. Consumer matches use observed filters, not per-record delivery or acknowledgment evidence. Core messages that were never captured by a stream cannot be inspected here.

The disposable NATS 2.11.8 test verifies sparse sequences, wildcard filtering, newest-record lookup, absent records, and unchanged consumer delivery/ack state after browsing. Fourteen Rust tests, trend/subject/demo JavaScript checks and Clippy pass. The standalone export includes the same browser controls with explicitly simulated records.

Live browser verification covered all four node-list metric trends, persistent selected samples across refreshes, stream popups, real sparse work-queue records, matching and unmatched subject filters, page navigation, an expired record, and the newest producer payload with two matching consumer filters.

Node monitoring coverage uses a compact dot and reporting count instead of an alert banner. Green indicates complete fresh coverage, amber partial coverage or initial connection, red no fresh reports or dashboard refresh failure, and gray unconfigured monitoring. The tooltip retains endpoint coverage and process-versus-container context. Reports older than 15 seconds do not count as reporting.

### Six investigation additions

Consumer histories and dedicated detail pages are implemented with identity-aware delivery-attempt rates, acknowledgment pressure and tracked-redelivery gauges. Incident recording now includes observed resource/configuration changes, threshold transitions, node startup/reload changes and an allowlisted NATS advisory subscription. Incidents persist in the additive SQLite v2 schema. Advisory recording is bounded to 120 accepted events per minute and 64 KiB per advisory; skipped events create a gap record. Chart markers and the searchable timeline link to resource context. Live verification continues as the remaining investigation views are integrated.

The six requested investigation additions are implemented. Historical requests accept windows up to six hours and return the latest 240 samples in that window, explicitly reporting truncation; the response is capped at 16 MB. Incident windows return at most 500 relevant records. Pausing freezes the investigation view while collection continues. Exported JSON includes selected identities, samples, incident evidence and limits, without retained payloads or full resource configuration.

Consumer backlog direction uses a contiguous recent window of at most 60 seconds. Connection rates require matching node/connection startup identities and two reads within 30 seconds. Native connection and subscription inventory is paged at 100 records per endpoint; filters apply to the loaded page. The standalone demo models consumer histories, phase-derived incidents and replica placement; node monitoring remains explicitly unavailable there.

The default history response is additionally bounded to 16 MB, retaining the newest fitting samples. Browser checks verified a persisted backlog transition opening the correct stream and consumer in a four-minute historical window, linked chart readouts, pause, native clients/subscriptions, replica placement, and the standalone demo. A snapshot download was requested from the browser; the in-app browser did not expose a saved-file path for disk verification.

Export serialization was verified with real observations: a 190,942-byte JSON artifact containing 27 samples and six events was written under data/live-demo/exports and parsed successfully. The browser download request and filesystem serialization were checked separately.


### Local beta hardening

These additions have no direct Fibril UI equivalent. They make the adapted observability interface usable as a local standalone tool.

| Addition | Implemented form | Reason and boundary |
| --- | --- | --- |
| Distribution | Embedded native binary archives with checksums; non-root Docker image; complete Compose demo | Removes a Rust toolchain requirement for runtime use; public artifact hosting is still pending |
| Restricted connections | NATS subject-permission template, explicit URL authentication, custom CA and mutual TLS | Broker permissions enforce read-only access; dashboard login remains a separate capability |
| Profile history guard | Additive SQLite v3 bindings, mismatch refusal and explicit legacy adoption | Prevents accidental mixing after configured identity changes; an unchanged endpoint/user is not broker-attested account identity |
| Dashboard self-health | Per-operation storage status, timestamps, visible failure notice and last validated settings | Successful inventory reads must not hide failed history writes |
| Storage budgets | Newest history within a configurable serialized-byte budget, separate incident budget and bounded activity | Time retention alone can consume excessive disk on large inventories; physical files include overhead and reusable pages |
| Repeatable release checks | Disposable TLS/permission/reconnect tests, oversized inventory test, local reader load check and endurance recorder | Validation records actual broker behavior and elapsed evidence; macOS and public CI remain unverified until run |

Documentation now describes current implemented behavior without the superseded initial-slice claims. Shared identity management and native mutations remain deferred beyond the local read-only beta.

Verification for the local beta increment: 22 Rust tests passed on Windows and Linux, including private CA/mutual TLS, denied operations, reconnects, large inventories, profile migration and storage failure recovery. The live Docker package and Windows release both observed the three-node demo. A 24-hour recorder was started; completion and external CI remain pending. Detailed evidence is in RELEASE_CHECKLIST.md.

### Public repository presentation

The README presents the operational questions addressed by the dashboard, a clone-and-run cluster demo and screenshots of actual demo broker observations. Detailed setup and limits are retained in docs/OPERATIONS.md. Screenshots include consumer diagnostics and a light-themed node view. The interrupted endurance recording is labeled incomplete.
