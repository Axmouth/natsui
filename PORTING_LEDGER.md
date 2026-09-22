# Fibril port and additions ledger

Last updated: 2026-09-15. This document describes the actual repository state. An implemented row is not a claim of production readiness. Update this ledger in the same change as each feature. AGENTS.md makes that a project convention.

Status vocabulary: **Implemented**, **Adapted**, **Planned**, **Optional integration**, **Not ported**. Planned and optional rows are not available UI features. Verification appears at the end.

## Product decisions

- General-purpose NATS operations, informed by real incidents without company-specific subjects, naming rules or infrastructure assumptions.
- Fibril's design principles and themes are retained. Branding uses a new pixel kitten.
- NATS is authoritative for messaging state. SQLite owns dashboard settings, observations and activity.
- Largest consumer backlog and number of consumers above a configurable threshold replace ambiguous summed backlog totals.
- Reported state, derived interpretation and missing evidence remain distinguishable.
- Standard mode uses an existing NATS deployment. Managing a NATS process is a separate opt-in integration.
- The Rust server embeds local assets and defaults to loopback. Named dashboard users, explicit HTTPS proxy configuration and optional OIDC support shared deployments.

## Original Fibril admin aspects

Source baseline: `../thetube/crates/admin` and `../thetube/website/src/content/docs/admin-dashboard.md`.

| ID | Original aspect | Status | Current form or intended replacement | Reason / boundary |
| --- | --- | --- | --- | --- |
| F01 | Dark/light design system and semantic colors | Adapted | Original CSS retained, compact accent-dot picker with named swatches and adjacent light/dark toggle, preferences persist locally, Original swatch matches its salmon accent in dark and light modes | Preserve visual identity and reuse proven presentation. |
| F02 | Mascot, favicon and brand character | Implemented | Pixel kitten favicon, text-only natsui wordmark, large mascot omitted | Explicit product identity choice. Busy traffic does not imply distress. |
| F03 | Named accent flavors | Adapted | All eight Fibril flavors available through a compact accent-dot picker | Preserves the collection without adding a large settings control. |
| F04 | Sidebar and command palette | Adapted | Native NATS navigation and Ctrl/Cmd-K page search | Resource vocabulary changes, navigation principles stay. |
| F05 | Per-broker switcher | Adapted | Per-tab connection profiles with independent credentials/history and optional user membership | Context persists without retargeting another tab. |
| F06 | Live status and freshness | Adapted | Snapshot time, complete/partial/unavailable status, disconnected browser warning | External collection can fail independently of the broker. |
| F07 | Shared SSE with polling fallback | Adapted | Bounded SSE observation notifications with coalesced refresh and polling fallback | Streams reuse collected snapshots. Browsers do not multiply NATS scans. |
| F08 | Curated attention panel | Adapted | Backlog threshold and 80% acknowledgment-pressure rules with drilldowns | Derive from exposed evidence, do not assert a worker root cause. |
| F09 | Desktop attention notifications | Adapted | Opt-in browser notifications for new recorded threshold transitions | Shared-tab deduplication and one-per-minute delivery limit. Requires an open dashboard. |
| F10 | Global backlog and inflight cards | Adapted | Maximum pending-delivery backlog, count above threshold, acknowledgment counts per consumer | Avoid counting overlapping consumer backlogs as unique messages. |
| F11 | Publish/delivery/completion rates | Partially adapted | Native server traffic, qualified stream append estimates and consumer delivery-attempt rates | Attempts include redelivery. Exact completion and acknowledgment throughput are not inferred from sequence floors. |
| F12 | Short broker-memory chart history | Adapted | SQLite summaries, configurable retention, selectable backlog windows aggregate at most 240 observed extrema with exact time/value/consumer inspection. Detailed resource histories retain their latest-sample bounds | Retains observations across restarts, partial and missing samples break chart lines. |
| F13 | Per-resource charts and sparklines | Adapted | Stream, node and consumer histories, inspectable list trends and dedicated detail pages | Identity changes and missing samples break histories and rates. |
| F14 | CPU, memory, disk and reconnect overview | Adapted | Optional /varz collector, node resource/rate charts and on-demand connection transport, Docker limits remain separate | Native process metrics need no Docker access. Container and host pressure require optional infrastructure telemetry. See RESOURCE_METRICS_AND_TRENDS.md. |
| F15 | Disk usage breakdown | Adapted | Logical bytes per stream, plus observed aggregate | These values are not filesystem utilization or replica-inclusive disk totals. |
| F16 | Queue list and partition detail | Adapted | Stream inventory, separate consumer inventory, config detail dialogs | Native NATS entities replace Fibril queue partitions. |
| F17 | Search and URL-persistent filters | Implemented | Stream/consumer search and above-threshold filter encoded in URL fragment | Shareable operational context. |
| F18 | Queue hide-inactive toggle | Planned | Relevant consumer filters after inactivity semantics are defined | Durable consumers may intentionally be idle. |
| F19 | Queue creation/deletion | Adapted | Native stream/consumer creation and reviewed deletion | NATS resources retain their native semantics and permissions. |
| F20 | Queue idle loading/eviction/skip reasons | Not ported | No equivalent field or placeholder | Fibril storage implementation detail unavailable through standard NATS APIs. |
| F21 | Oldest ready-message age | Optional integration | Retained age can be added, exact outstanding age needs adequate evidence | Oldest retained is not necessarily oldest pending for a consumer. |
| F22 | Test publish | Adapted | Explicit Core publish and JetStream storage acknowledgment | Broker acceptance is not verified business processing. |
| F23 | Follower checkpoint/recovery internals | Not ported | Native replica matrix shows leaders, current/offline followers and lag instead | Fibril internal checkpoint/recovery states have no equivalent public NATS fields. |
| F24 | Plexus stream inventory and durable cursors | Adapted | Stream retention/configuration and consumer pending/ack values | Native JetStream state replaces Plexus semantics. |
| F25 | Plexus durability tiers and live-buffer recovery | Not ported | Native storage/replication fields shown, no mapped durability tier | Avoid pretending implementations have equivalent guarantees. |
| F26 | Retained-message inspector | Adapted | Paged retained-record browsing, subject filters, newest matching record, exact sequence lookup and payload dialog through STREAM.MSG.GET | Does not pull or acknowledge production consumer work. |
| F27 | Optional payload previews and bounded reads | Adapted | On-demand browse capped at 20 records, 8 seconds and 8 MB of processed record JSON, per-response cap 2 MB. List responses omit payloads, the native lookup still reads payloads from NATS. Payload dialog requires explicit inspection, no SQLite payload storage | Debugging should not become automatic message replication. |
| F28 | Exact ready/inflight/delayed/settled/DLQ per-record filters | Not ported | Consumer summary counters, optional event evidence later | Public consumer information is not a complete per-record lifecycle. |
| F29 | Topic/group suggestions in forms | Adapted | Stream selector, capture rules and consumer filter suggestions use observed inventory | No guessed resource names needed for inspection. |
| F30 | Native global/per-queue DLQ policies | Optional integration | Configurable application DLQ conventions and failure collector | MaxDeliver advisories do not move records automatically. |
| F31 | Replay selected to original source | Optional integration | Explicit destination/provenance, header policy and per-item results | Arbitrary messages do not carry sufficient source metadata, copy/delete is not atomic. |
| F32 | Activity diary | Adapted | Searchable incident timeline, selected configuration diffs, threshold changes, observed restarts and received NATS advisories | Persistent SQLite events include source, resource and gaps, missed advisories are not replayed. |
| F33 | Runtime settings with editability flags | Adapted | Dashboard settings, reviewed native resource edits and optional process controller | Server files require separate deployment authority. |
| F34 | Version-checked settings saves | Adapted | Revision checks and reviewed resource/controller changes | External NATS writers cannot be atomically excluded by the dashboard. |
| F35 | Admin users, rotation and logout | Adapted | Named access-key users, roles, one-time links and optional OIDC | Dashboard identity stays separate from broker identity. |
| F36 | NATS/broker user administration | Adapted | Optional config process controller and NSC JWT account authority | Explicit signing or process ownership is required. No authority is inferred from observer access. |
| F37 | Served certificate inspection and reload | Optional integration | Dashboard TLS separately, NATS certificates only with deployment access | External dashboard does not own NATS files or certificates. |
| F38 | First-boot broker TLS setup | Not ported | Future connection onboarding, not broker startup gating | Standard mode attaches to existing infrastructure. |
| F39 | Connections table and traffic diagrams | Partially adapted | Searchable cross-node connections, client/SDK details, rates, RTT, pending output and subscriptions | Paged on demand, 100 per endpoint. Animated traffic diagrams remain unimplemented. |
| F40 | Subscriptions and queue-group inspection | Adapted | Subject explorer includes paged native subscriptions, account, queue group and connection identity | Monitoring scope may exceed the JetStream profile. Interest does not prove delivery. |
| F41 | Cohort worker targets and partition assignments | Optional integration | Optional worker registry/telemetry | Broker consumer state does not provide Fibril's assignment model. |
| F42 | Animated topology and placement matrix | Partially adapted | Stream placement and reported route topology with directional counters | Route pools are distinct from peers. Animation is omitted because it would imply unobserved message paths. |
| F43 | One cluster consensus leader | Not ported | Show appropriate metadata/stream/consumer leadership separately | JetStream has multiple consensus groups. |
| F44 | Drain, repartition and voting-member actions | Optional integration | Native, version-aware operational workflows where supported | No Fibril-style handoff guarantee or repartition abstraction is assumed. |
| F45 | Quarantine repair/log truncation | Not ported | Health evidence and recovery guidance only | No equivalent safe public repair operation. |
| F46 | Process health endpoint | Adapted | /healthz reports dashboard liveness, collection status remains separate | Dashboard running does not imply NATS is healthy. |
| F47 | Storage command lanes, append/snapshot/recovery counters | Not ported | Native NATS diagnostics when available | Fibril-specific instrumentation. |
| F48 | Global timer interception during navigation | Not ported | One explicit browser refresh loop and simple page lifecycle | No need to copy a navigation workaround. |
| F49 | Vendored local assets and no external requests | Implemented | CSS, JavaScript and pixel kitten embedded in Rust binary | Portable and independent of external asset services. |

## Additions not present in the original Fibril admin

| ID | Addition | Status | Form and reason |
| --- | --- | --- | --- |
| N01 | NATS management adapter | Implemented | Native request/reply API, paged stream/consumer scans with time and inventory caps. |
| N02 | SQLite persistence | Implemented | Settings, compact historical summaries and activity, WAL and retention cleanup, no payload persistence. |
| N03 | Evidence and coverage vocabulary | Implemented | Reported/derived tags, coverage page, partial/unavailable states and explicit fixture banner. |
| N04 | Largest consumer backlog and threshold summary | Implemented | Maximum pending deliveries and count strictly above configured threshold. |
| N05 | Configurable threshold, collection cadence and retention | Implemented | Local SQLite settings applied without NATS restart. |
| N06 | Explicit demo mode | Implemented | --demo simulation stays separate from the live Docker demo, the latter uses native JetStream observations and its own SQLite directory. |
| N07 | Native-versus-managed settings distinction | Implemented | Settings page explains ownership, no hidden process controller. |
| N08 | Shared-workspace roles | Implemented | Viewer/operator/admin roles, optional profile membership and explicit OIDC mappings. Global administrators remain trusted across the deployment. |
| N09 | Durable advisory/incident collector | Implemented | Selected advisory fields persisted to SQLite, with recording caps and connection-gap notices. Routine API audits and individual NAK events are excluded. No broker-side replay stream is created. |
| N10 | Configuration snapshots and incident correlation | Partially implemented | Operational-setting diffs between complete observations, threshold changes and startup/reload identity changes, resource/time drilldowns |
| N11 | Subject relationship explorer | Implemented | Literal subject lookup shows configured capturing streams, matching consumers on those streams and on-demand live subscription interests with account context. No observed message-path claim. |
| N12 | KV and Object Store views | Implemented | Dedicated bounded browsing uses native bucket semantics. |
| N13 | Optional worker telemetry contract | Optional integration | Generic worker identity and error evidence without a mandatory SDK or company-specific schema. |
| N14 | Managed deployment controller | Implemented, optional | Config validation and owned-process reload/restart. Separate NSC authority issues users and reviews local revocation or resolver publication. |
| N15 | Historical comparison and saved views | Implemented | Linked historical charts and export, plus up to 12 named browser-local views per profile. Saved selections contain no observations or payloads. |

| N16 | Reusable real-cluster demo | Implemented | Three Docker NATS nodes, replicated streams, shared work-queue workers, independent consumers, Core queue group and scripted slowdown/burst/recovery. PowerShell launcher and lifecycle documentation in demo/README.md. Actual throughput and backlog are observed, not fabricated. |

| N17 | Native metric discovery | Partially implemented | Prioritized CPU/RSS, retention occupancy, replication, connections, traffic and transport pressure. Feasibility and interpretation boundaries in RESOURCE_METRICS_AND_TRENDS.md, Native resource views implemented as documented, route topology is available. Infrastructure pressure and worker outcomes remain separate work. |

## Attribution

Design attribution appears in the README and About page. MIT license credit remains in LICENSE because theme styles are adapted from Fibril. The primary footer links to About without persistent co-branding.

## Verification and next increment

Seventeen Rust tests pass, including monitoring failure isolation, response bounds and live NATS 2.11.8 checks for overlapping filtered consumers, maximum-not-sum backlog and non-consuming message inspection. Browser verification covers filters, drilldowns, inspection, settings changes and theme/layout behavior. Full verification and remaining test gaps are recorded in README.md.

Current priority: local operator beta hardening and release evidence. Native configuration edits include before/after review and remain opt-in. Shared user management precedes externally accessible deployment. A deployment supervisor is not implemented or installed by this change.

### Real-cluster demo verification (2026-09-12)

Three NATS 2.11.8 containers and the traffic container passed health checks. All replicated streams reported current followers. The dashboard recorded real backlog growth to 18,145 followed by recovery to about 200, with threshold crossings in SQLite history. Retained-message inspection returned an actual producer payload. Generator restart preserved resources. Core traffic exists, connection and queue-group views were added in the six investigation additions below. See demo/README.md.


### Resource view verification (2026-09-12)

Live browser checks verified three-node inventory, CPU/RSS histories, stream sparklines, retention and replica details, exact historical readouts and on-demand client RTT/pending-byte data. SQLite resource samples survived dashboard restart. JavaScript tests cover rates, identity changes, counter resets, duplicate monitoring samples and missing observations. Rust tests cover bounded HTTP responses, rejected redirects, independent endpoint failure and existing live NATS inspection. Clippy passes with warnings denied.

### Inspectable lists and retained records (2026-09-12)

Stream and node mini charts support hover, touch and keyboard inspection with timestamp/value popups outside the table clipping boundary. Node-list sparklines cover CPU, resident RAM, connections and subscriptions. Chart hosts remain stable across polling, missing observations remain gaps.

The message browser reads up to 20 actual records through standard STREAM.MSG.GET with seq and next_by_subj. This skips deleted sequence gaps without creating a consumer. A newest-matching shortcut uses last_by_subj. Exact lookup distinguishes the explicit no-message response from upstream failures. Reads are not atomic snapshots: retention or work-queue acknowledgments can remove a listed record before payload inspection. Consumer matches use observed filters, not per-record delivery or acknowledgment evidence. Core messages that were never captured by a stream cannot be inspected here.

The disposable NATS 2.11.8 test verifies sparse sequences, wildcard filtering, newest-record lookup, absent records, and unchanged consumer delivery/ack state after browsing. Fourteen Rust tests, trend/subject/demo JavaScript checks and Clippy pass. The standalone export includes the same browser controls with explicitly simulated records.

Live browser verification covered all four node-list metric trends, persistent selected samples across refreshes, stream popups, real sparse work-queue records, matching and unmatched subject filters, page navigation, an expired record, and the newest producer payload with two matching consumer filters.

Node monitoring coverage uses a compact dot and reporting count instead of an alert banner. Green indicates complete fresh coverage, amber partial coverage or initial connection, red no fresh reports or dashboard refresh failure, and gray unconfigured monitoring. The tooltip retains endpoint coverage and process-versus-container context. Reports older than 15 seconds do not count as reporting.

### Six investigation additions

Consumer histories and dedicated detail pages are implemented with identity-aware delivery-attempt rates, acknowledgment pressure and tracked-redelivery gauges. Incident recording now includes observed resource/configuration changes, threshold transitions, node startup/reload changes and an allowlisted NATS advisory subscription. Incidents persist in the additive SQLite v2 schema. Advisory recording is bounded to 120 accepted events per minute and 64 KiB per advisory, skipped events create a gap record. Chart markers and the searchable timeline link to resource context. Live verification continues as the remaining investigation views are integrated.

The six requested investigation additions are implemented. Historical requests accept windows up to six hours and return the latest 240 samples in that window, explicitly reporting truncation, the response is capped at 16 MB. Incident windows return at most 500 relevant records. Pausing freezes the investigation view while collection continues. Exported JSON includes selected identities, samples, incident evidence and limits, without retained payloads or full resource configuration.

Consumer backlog direction uses a contiguous recent window of at most 60 seconds. Connection rates require matching node/connection startup identities and two reads within 30 seconds. Native connection and subscription inventory is paged at 100 records per endpoint, filters apply to the loaded page. The standalone demo models consumer histories, phase-derived incidents and replica placement, node monitoring remains explicitly unavailable there.

The default history response is additionally bounded to 16 MB, retaining the newest fitting samples. Browser checks verified a persisted backlog transition opening the correct stream and consumer in a four-minute historical window, linked chart readouts, pause, native clients/subscriptions, replica placement, and the standalone demo. A snapshot download was requested from the browser, the in-app browser did not expose a saved-file path for disk verification.

Export serialization was verified with real observations: a 190,942-byte JSON artifact containing 27 samples and six events was written under data/live-demo/exports and parsed successfully. The browser download request and filesystem serialization were checked separately.


### Local beta hardening

These additions have no direct Fibril UI equivalent. They make the adapted observability interface usable as a local standalone tool.

| Addition | Implemented form | Reason and boundary |
| --- | --- | --- |
| Distribution | Embedded native binary archives with checksums, non-root Docker image, complete Compose demo | Removes a Rust toolchain requirement for runtime use, public artifact hosting is still pending |
| Restricted connections | NATS subject-permission template, explicit URL authentication, custom CA and mutual TLS | Broker permissions enforce read-only access, dashboard login remains a separate capability |
| Profile history guard | Additive SQLite v3 bindings, mismatch refusal and explicit legacy adoption | Prevents accidental mixing after configured identity changes, an unchanged endpoint/user is not broker-attested account identity |
| Dashboard self-health | Per-operation storage status, timestamps, visible failure notice and last validated settings | Successful inventory reads must not hide failed history writes |
| Storage budgets | Newest history within a configurable serialized-byte budget, separate incident budget and bounded activity | Time retention alone can consume excessive disk on large inventories, physical files include overhead and reusable pages |
| Repeatable release checks | Disposable TLS/permission/reconnect tests, oversized inventory test, local reader load check and endurance recorder | Validation records actual broker behavior and elapsed evidence, macOS and public CI remain unverified until run |

Documentation now describes current implemented behavior without the superseded initial-slice claims. Shared identity management and native mutations remain deferred beyond the local read-only beta.

Verification for the local beta increment: 22 Rust tests passed on Windows and Linux, including private CA/mutual TLS, denied operations, reconnects, large inventories, profile migration and storage failure recovery. The live Docker package and Windows release both observed the three-node demo. A 24-hour recorder was started, completion and external CI remain pending. Detailed evidence is in RELEASE_CHECKLIST.md.

### Public repository presentation

The README presents the operational questions addressed by the dashboard, a clone-and-run cluster demo and screenshots of actual demo broker observations. Detailed setup and limits are retained in docs/OPERATIONS.md. Screenshots include consumer diagnostics and a light-themed node view. The interrupted endurance recording is labeled incomplete.

## Native configuration editing increment

| Aspect | Form | Reason and boundary |
| --- | --- | --- |
| Fibril administrative configuration | Adapted into Settings with reviewed native JetStream updates | Stream capacity/capture/replication and consumer acknowledgment/retry settings have native equivalents, server file and lifecycle control remain separate. |
| Natsui addition: reviewed change workflow | Implemented | Allowlisted fields, exact numeric values, preview expiry, one-use apply, fresh configuration/creation identity check, before/after incidents and readback. External writers are not atomically excluded. |
| Natsui addition: write capability | Implemented | Explicit per-instance/profile startup opt-in with native permission enforcement, read-only and simulated modes remain supported. Shared identities are not implied. |
| Windows release packaging | Fixed | Tar runs in the output directory with a relative archive filename, avoiding GNU tar remote-host parsing of drive-letter paths. |

Creation/deletion, consumer policy migration, server config files and supervision remain unimplemented. Earlier read-only beta entries describe the previous release slice, native editing supersedes the mutation deferral for the fields listed in SETTINGS_AND_ACCESS.md only.

Verification: 25 Rust tests passed on Windows and Linux, including real NATS stream/consumer edits, immutable-field rejection, read-only/simulation gating, stale revisions, one-use previews and unchanged consumer delivery state. Browser verification applied and restored a stream limit against the three-node demo. Windows packaging passed with Git Bash GNU tar, and archive contents/checksum were checked.

## Published tryout and browser demo

| Aspect | Form | Reason and boundary |
| --- | --- | --- |
| Fibril one-command cluster tryout | Adapted as a published Docker Compose OCI bundle | One cross-platform Docker command starts three unmodified NATS servers, a dashboard and traffic, without a clone or compiler. Inline broker configuration removes local-file dependencies. |
| Fibril public demo entry point | Adapted as a GitHub Pages landing page with the fully simulated dashboard | An immediate browser preview complements the real cluster. Demo assets are self-contained and connection attempts are blocked by Content Security Policy. |
| Natsui addition: standalone image | Published through GitHub Actions after successful verification | Dashboard and traffic images target Linux AMD64 and ARM64. The standalone dashboard works alongside existing NATS, with no broker plugin or Docker socket. |
| Natsui addition: digest-pinned demo bundle | Published after native-architecture real-cluster smoke checks | Image digests keep a bundle's components consistent. latest/main track verified builds, full commit tags identify individual builds. |

The published demo uses its own Compose project and named volumes. Only the dashboard is exposed on loopback, stopping retains data and explicit down -v deletes it. Source-build demos remain available. Package visibility and Pages enablement are repository-owner settings, public availability requires verification after the initial publication.

Local verification: fresh-volume Compose startup reported three replicated streams, five consumers and all monitoring endpoints. Billing pending ranged from 200 to 18,045 across the workload phases, history and real retained-record inspection passed. A temporary local registry round trip verified OCI publication, inline configuration, the custom-port variable and readyz before removing its test project. The static demo rendered in the browser with synthetic history and no backend. Public package visibility and hosted deployment are verified separately after publication.


## Browser demo monitoring increment

| Aspect | Form | Reason and boundary |
| --- | --- | --- |
| Fibril inspectable node histories | Adapted into the browser simulation | Three modeled nodes include CPU, resident memory, connections and subscription trends, plus detailed traffic and counter histories. No processes or containers are measured. |
| Natsui addition: synthetic client inventory | Implemented in the browser adapter | Producers, workers, temporary burst clients, RTT, pending output and subscription interest make connection and subject inspection usable without a backend. Node counts agree with client inventories. |
| Natsui addition: repeatable monitoring workload | Implemented | Lifetime counters remain monotonic across cycles, temporary clients receive new identities. Connection rates retain the same two-observation requirement as real monitoring. |

Verification: simulation checks cover three cycles, counter continuity, client turnover, subscription totals, pagination and API errors. Trend tests and static-site network isolation checks passed. Browser inspection confirmed populated node sparklines and detail graphs, connection rates after refresh, client inspection and subscription queue groups. Synthetic monitoring is specific to the browser adapter, backend simulation and live monitoring retain their existing behavior.

Public publication verification: the landing page and browser demo returned HTTP 200. All three GHCR packages allowed anonymous manifest reads, with AMD64 and ARM64 image platforms. The published latest Compose bundle started successfully with an empty Docker credential configuration and reported three real NATS nodes, three streams and five consumers. Its disposable smoke-test project and volumes were removed afterward.


## Browser demo subject discovery

| Aspect | Form | Reason and boundary |
| --- | --- | --- |
| Natsui addition: discoverable demo subjects | Implemented as six clickable examples and automatically loaded synthetic subscriptions | Stream-backed subjects, Core NATS queue-group and wildcard interest, and an unmatched subject make the existing subject tracer discoverable. Example subjects are explicitly synthetic and do not imply an exhaustive NATS subject catalog. |

The browser demo labels subscription inventory as simulated. Real monitoring retains on-demand subscription loading. Clearing the subject query restores the full loaded subscription inventory. Verification covers example capture and interest relationships, static export and browser navigation through matching and unmatched examples.


## Persistent workspace context

| Aspect | Form | Reason and boundary |
| --- | --- | --- |
| Fibril persistent header | Adapted as a sticky Natsui top bar | Workspace identity, connection mode, navigation search and appearance controls remain accessible while scrolling long views. An opaque theme background separates content below, the existing compact mobile layout is retained. Scroll padding leaves clearance for focused controls. |

The shared stylesheet applies to live dashboards and the browser demo. Page titles and scenario controls remain in normal document flow.

Verification: the static build passed and browser inspection confirmed the header remains visible over a long subject inventory in the narrow layout, with the theme menu layered above content. Theme-menu focus preserves the scroll position.


## Dashboard authentication and deployment guide

| Aspect | Form | Reason and boundary |
| --- | --- | --- |
| Natsui addition: dashboard authentication | Optional generated-access-key login, expiring sessions and sign-out | Protects dashboard/API access independently of NATS credentials without another service. All key holders share one operator identity, individual users, roles and public deployment remain unimplemented. |
| Natsui addition: bounded authentication | Implemented | 256-bit generated keys and sessions, constant-time key-digest comparison, eight-hour server expiry, explicit revocation, restart invalidation, session cap and sign-in rate/body limits. Invalid configured key files fail startup. Existing Host/Origin and mutation-header controls remain enforced. |
| Fibril-style accessible setup documentation | Adapted as a Pages setup guide | The guide connects the disposable cluster tryout to authenticated existing-broker deployment, with native and Docker startup paths and an authenticated Compose download. |
| Natsui addition: explicit SQLite persistence contract | Documented and verified | The entire /data directory belongs on writable local persistent storage, including WAL/SHM companions. Secret mounts remain separate. Volume ownership, lifecycle, logical versus physical budgets, stopped backups and fresh-volume restores are covered. |
| Natsui addition: repeatable auth/storage deployment checks | Added to CI | A real container drill verifies access-key initialization, protected requests, sign-out, session revocation after replacement, retained settings/history and backup restoration without logging credentials. |

Verification: all 30 Rust tests passed on Windows and Linux, including real NATS TLS, permissions and reviewed edits. The Linux verification image also packaged the binary and deployment documentation. Clippy, trend/simulation tests and static-site link/isolation checks passed. Browser verification exercised invalid login, successful login and sign-out, and rendered the Pages setup guide. The Docker auth/storage drill passed with UID 10001, read-only container roots, a separate read-only secret volume, replacement of the container and restoration into a fresh volume.

Authentication remains opt-in for trusted local deployment through NATSUI_AUTH_TOKEN_FILE. The one-command cluster demo and static browser demo retain their documented unauthenticated behavior. Prior no-login entries describe earlier increments, this entry supersedes them for the single-operator mode only.

## Cluster connection setup recipes

The repository and Pages setup guides now include shared Docker network discovery, multiple monitoring hosts and ports, one-seed discovery limits, URL and JWT authentication, private-CA TLS, mutual TLS, file permissions, persistent mounts and connection troubleshooting. Downloadable Compose overlays mount only the required credential/certificate files read-only and retain dashboard login and SQLite storage. The guide distinguishes broker TLS from the independently configured HTTP monitoring client. Broker user management and monitoring custom CA/client authentication remain unimplemented. Multiple initial server URLs are implemented as described below.

Verification: the downloadable network/TLS/mutual-TLS Compose files were exercised against three disposable NATS 2.11.8 containers with separate client and monitoring ports, private-CA server certificates, client certificates and a restricted password identity. Dashboard login, complete JetStream collection, three reporting monitoring endpoints and read-only secret mounts passed. The JWT overlay was checked through Compose rendering, live JWT authentication was not part of this documentation smoke check.

## Concurrent NATS seed connections

NATSUI_URL accepts up to 32 comma-separated seed addresses. Initial connections run concurrently and the first successful authenticated handshake wins. Every candidate retains all configured seeds for later reconnects through the NATS client. URL credentials are shared across the set, conflicting inline credentials are rejected, and any TLS requirement applies to every peer. Profile identity is independent of seed order and the winner, while changing the seed set requires a new profile. Existing single-seed bindings remain compatible.

HTTP monitoring stays separate because each endpoint reports one process and NATS discovery does not provide its HTTP address or port. The setup guide explains both protocols, the separate collection behavior, seed examples and connection-file mounts. Setup prose uses sentences without semicolons.

Verification: all 32 Rust tests passed on Windows and Linux, including a stalled seed, an offline seed, mutual TLS, certificate rejection, reconnects without advertised peers and subscription recovery. A disposable three-node TLS/password cluster also passed cold start with the first seed offline, recovery after stopping the connected node and independent 2/3 monitoring coverage. Losing startup connections were confirmed closed. Pages links, demo checks, Clippy and Linux packaging passed.

## Ansible deployment example

A standalone playbook and repository/Pages guide provision a protected dashboard on an existing Linux Docker host. Vault-backed source files, UID 10001 storage ownership, individual read-only secret mounts, change-triggered recreation, optional broker readiness checks and loopback access through SSH are covered. The playbook uses the existing key-file interface. The playbook keeps explicit secret provisioning. The standalone Compose initializer and one-time login command are implemented separately.

## Shared access and native operations increment

| Aspect | Current form | Reason and boundary |
| --- | --- | --- |
| Easier protected login | Implemented idempotent Compose initialization and one-time 60-second login tickets | Preserves file-based bootstrap authority and Ansible provisioning. Existing access keys remain supported. |
| F35 and N08 dashboard users and roles | Implemented named key identities, viewer/operator/admin enforcement, rotation, disabling and revision checks | Key digests persist in access.sqlite3. This is not OIDC or password authentication. |
| Public dashboard HTTPS | Implemented explicit NATSUI_PUBLIC_URL and Secure cookies behind a trusted proxy | The internal listener remains HTTP. Forwarded headers never expand the accepted origin. |
| Monitoring transport security | Implemented per-endpoint CA, client certificates, Basic and Bearer credential files | Separate from NATS transport credentials. Credentials require HTTPS and are never forwarded across redirects. |
| F19 resource lifecycle | Implemented reviewed native stream and durable pull-consumer creation/deletion | Deletion revalidates identity and configuration. Native NATS has no atomic revision-checked deletion against external replacement. |
| F22 test publish | Implemented explicit Core or expected-stream JetStream publishing, bounded payload and no automatic retry | Core acceptance is not storage or processing evidence. JetStream acknowledgment identifies stored stream and sequence. |
| N12 KV and Object Store | Implemented retained-key listing, explicit latest KV revision/tombstone inspection and object metadata | Uses information and message-get APIs without creating or pulling application consumers. Object chunks are not downloaded. |

Ansible verification passed with ansible-core 2.19.13 and Docker Compose 5.5.1: syntax/check mode, Vault decryption, unchanged second deployment, credential rotation, session invalidation and retained settings. Access tests pass on Windows and Linux, including roles, stale revisions, key rotation, ticket expiry/reuse and exact public-origin checks. Native lifecycle and bucket integration checks are being completed separately.

## Shared deployment review corrections, 2026-09-14

Named dashboard identities, explicit HTTPS origin, per-endpoint monitoring credentials, independent connection profiles, native lifecycle/publish operations and read-only bucket browsing are implemented. The optional controller handles config-based NATS users and selected server limits only when deployment authority is supplied. These are NATS-specific additions, rather than Fibril job-state analogues.

The independent maintainability review led to credential-bound sessions, persistent user revisions, one authentication middleware boundary, per-tab profile routing and bounded session-owned review maps. Controller acceptance and process supervision now have real-process regressions. [Review findings](docs/MAINTAINABILITY_REVIEW.md) records reasons and remaining limits. [Implementation queue](docs/IMPLEMENTATION_QUEUE.md) distinguishes completion from endurance evidence still needing elapsed time.

Pages deployment guides are generated from the Markdown source to avoid parallel documentation copies. Monitoring TLS/auth tests and literal UI route checks are included in the repository and CI. The normal dashboard and one-command demo still run without the optional controller.

Final local verification passed: all 45 Linux Rust tests with real brokers, 39 Windows unit tests, Linux and Windows packaging, controller process/TLS/user tests, monitoring security integration, Pages/demo checks and the authenticated cluster browser regression. The independent final review found no remaining blockers in the reviewed changes. A 24-hour recording is running separately and is not yet qualified.

## Backlog time windows, 2026-09-15

The overview backlog chart adapts Fibril history inspection with rolling 5-minute, 15-minute, 1-hour and 6-hour windows. The default is 15 minutes and the browser remembers the choice. It uses the existing bounded history API in both production and the static demo. The time axis retains the requested range, missing observations remain gaps, and truncated responses disclose the latest-240-sample limit. Longer windows do not imply complete coverage. Partial endurance evidence was accepted for the current scope, without claiming an uninterrupted 24-hour pass.

Verification passed for production assets backed by the local NATS cluster and the standalone static demo in Edge: preset bounds, missing-interval inspection, keyboard navigation, persisted choice, empty history and mobile layout. Pages link checks, demo invariants, trend checks and UI route checks passed.


## Follow-up features, 2026-09-15

| Aspect | Form | Reason and limits |
| --- | --- | --- |
| Backlog window | 5m, 15m, 1h or 6h with at most 240 observed points | First, last, minimum and maximum samples per bucket preserve range and spikes. Historical cadence and incomplete samples break continuity. Legacy samples without cadence remain discrete. |
| History reads | SQLite covering expression index for compact backlog projections | Avoids parsing retained resource inventories during each chart refresh. The index builds once at startup and adds disk usage. Detailed investigation windows retain their 240-sample cap. |
| Notifications | Browser opt-in for new backlog/ack-pressure transitions | At most one per minute per browser/profile, with shared-tab coordination. No payload content, background delivery service, email or webhook credentials. |
| Topology | Bounded routez graph and per-direction route table | Reported route pools do not imply message paths or prove unseen links are down. |
| Login | Existing one-time links plus optional OIDC | Explicit issuer/subject mappings pin local identity generations. No automatic group sync or provider-side logout propagation. |
| JWT administration | Optional pinned NSC adapter with mounted authority | User issuance can take effect immediately. Revocations are local until distributed. Resolver acceptance is distinct from all-broker convergence. |

Deployment recipes are published from OIDC.md, JWT_AUTHORITY.md, PROFILES.md and SHARED_ACCESS.md. The static demo reuses the same browser assets. Authentication, controllers and live transport remain backend capabilities.


## NATS-backed login, 2026-09-22

| Aspect | Form | Reason and limits |
| --- | --- | --- |
| Existing NATS users | Optional username/password login, new to natsui | Live requests authenticate with the session credentials and NATS enforces its API permissions. No local copy of NATS permissions or resource ownership is inferred. |
| Collector history and monitoring | Separate opt-ins for NATS sessions | These data sources do not perform the user's NATS authorization checks. Sharing exposes profile-wide collector observations, potentially across accounts. Embedded node history is removed when monitoring sharing is disabled. |
| Dashboard administration | Separate dashboard login retained | NATS credentials confer no authority over dashboard users, profiles, settings or deployment controllers. |
| Long-lived credentials | Memory-only sessions with fresh request authentication | Logout, expiry and restart release session credentials. Fresh connections add connection traffic and avoid stale reconnect identities. NATS JWT, NKey and client-certificate login remain deferred. |
| Partial inventory | Preserve streams when consumer requests exhaust the time budget | Restricted users can retain permitted stream observations without implying empty consumer inventories. |
| Core publishing | Sent with acceptance unconfirmed | A protocol flush cannot establish success when NATS reports authorization errors asynchronously. |

Verification: 54 Rust tests, strict Clippy, real NATS permission isolation and TLS, concurrent session replacement, desktop/mobile login, existing OIDC/SSE, demo and Pages checks passed locally. Details and limitations are recorded in docs/VERIFICATION.md.


## Setup guide polish, 2026-09-22

NATS username/password sign-in is the main documented setup path for new deployments. A standalone Compose recipe provides persistent SQLite storage, fixed collector credentials and separate shared-data flags that default to disabled. Dashboard-key, SSO, JWT and mutual-TLS recipes remain explicit alternatives. Runtime authentication defaults are unchanged.

The guides add a login comparison, symptom-based NATS sign-in troubleshooting, an in-page contents list and section permalinks. Copy controls use format-neutral feedback and restore their original labels. These are documentation and navigation conveniences, new to natsui rather than ports from Fibril.

Verification: Pages link checks, Compose defaults and TLS-overlay rendering, and desktop/mobile browser checks passed. Runtime deployments were not modified.
