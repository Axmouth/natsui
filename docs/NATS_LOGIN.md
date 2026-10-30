# NATS-backed login

NATS-backed login supports username/password sign-in and an optional client-certificate form. NATS validates the credentials. Every live stream, consumer, message, bucket and operation request uses those credentials. Session-only deployments need fixed server addresses and trust configuration, with no collector account. The optional background collector uses a separate deployment identity.

NATS permissions remain authoritative. Stream listing returns whatever the NATS API permits, without inferred ownership or application-subject filtering. Failed live requests never retry with collector credentials. Unavailable data is not an empty inventory.

For a new deployment, start with the [step-by-step setup guide](SETUP.md). It downloads the NATS login Compose file, configures the server addresses, starts the dashboard and signs in with an existing NATS user. The following settings apply when adding this option to an existing deployment.

The [deployment checker](DOCTOR.md) validates this configuration inside the container and can probe a supplied NATS identity without changing deployment settings.

The dashboard can [serve HTTPS directly](HTTPS.md) with a mounted server certificate and key. Browser HTTPS and broker TLS have independent settings.

## Session-only mode

This mode starts without connecting to NATS. Browser sign-in authenticates against the configured cluster, and each live data request uses that session's credentials. No observer account, HTTP monitoring collector or background advisory subscription is required.

```yaml
environment:
  NATSUI_URL: nats://nats1:4222,nats://nats2:4222
  NATSUI_NATS_LOGIN: '1'
  NATSUI_NATS_LOGIN_SESSION_ONLY: '1'
  NATSUI_ALLOW_WRITES: '0'
```

Server addresses must contain no credentials. NATSUI_CREDS, NATSUI_TLS_CERT and NATSUI_TLS_KEY must be unset, including in additional profiles. NATSUI_TLS_CA and NATSUI_TLS_REQUIRED still configure server trust. Shared history and shared monitoring flags must remain 0. Startup rejects conflicting sharing options or collector credentials.

Each login creates a private in-memory SQLite database. The existing history, backlog, resource trend and observed-incident queries use that database. Snapshot polling records observations while a dashboard tab is open. The first snapshot establishes a baseline, so incidents do not imply changes before sign-in. Multiple tabs using the same cookie share a session. Separate logins remain isolated even when they use the same NATS username.

Session history starts empty and disappears on logout, session replacement, eviction, expiry or process restart. Completed requests already in flight may finish. There is no collection while tabs are closed, no recovery of earlier session history, and no HTTP node monitoring. Data access still requires fresh NATS authentication, including historical reads. Broker outages therefore make session history unavailable until authentication succeeds again.

History and incident payloads each have a 4 MiB rolling budget per session, with at most 32 authenticated dashboard sessions. Older records are pruned. These are serialized payload limits, not process RAM limits. The previous observation also has a 4 MiB serialized cache limit. Larger observations remain live but cannot establish an incident-comparison baseline. Samples within two seconds share one observation. Session operations also use the private activity store, so this mode does not provide a durable operation audit. Persistent /data remains useful for dashboard-owned settings and identities but contains no session observations or uploaded keys. It is optional in this mode: when the data directory is not writable, the dashboard starts with in-memory storage, logs that choice and reports `persistence: memory` in storage health. Settings and dashboard users then reset on restart. Dashboard identities cannot fall back to collector data in this mode.

The /readyz endpoint reports collection as on_sign_in and checks dashboard storage. A successful readiness response means the sign-in service is ready, not that any broker or user permissions were tested.

## Enable the login option

The alternative background-collection mode retains observations between logins. Add these environment settings to the dashboard service in an existing Compose deployment:

```yaml
environment:
  NATSUI_URL: nats://observer:REPLACE_WITH_COLLECTOR_PASSWORD@nats1:4222,nats://nats2:4222
  NATSUI_NATS_LOGIN: '1'
  NATSUI_NATS_LOGIN_SESSION_ONLY: '0'
  NATSUI_NATS_LOGIN_SHARED_HISTORY: '0'
  NATSUI_NATS_LOGIN_SHARED_MONITORING: '0'
  NATSUI_ALLOW_WRITES: '0'
```

NATSUI_URL supplies fixed server addresses and the background collector's credentials. NATSUI_CREDS can supply the collector identity instead. Submitted login credentials replace collector authentication for live requests. Hosts cannot be supplied through the login form.

The environment block extends the existing deployment. Its persistent /data volume and private network remain required. Apply the deployment with docker compose up -d, then open the dashboard and select the cluster, NATS username and NATS password. Real NATS authentication must be enabled. The simulated demo cannot use NATS login.

NATSUI_ALLOW_WRITES=1 enables the existing reviewed edit workflow. NATS still decides whether the signed-in user can perform the requested operation. A successful login alone grants no NATS API permissions.

NATSUI_AUTH_TOKEN_FILE is optional in a deployment that only needs NATS login. Keeping a mounted dashboard access key provides access to dashboard administration and the existing one-time login CLI. NATS sessions cannot change dashboard users, settings, profiles or controller configuration and cannot access controller APIs or the administrative activity log. Session-only Activity contains only actions and changes recorded by that login.

## TLS and profiles

NATSUI_TLS_CA and NATSUI_TLS_REQUIRED apply to the login connection as well as collection. The [setup guide](SETUP.md) includes sample CA mounts and HTTPS deployment instructions. Dashboard HTTPS and NATS TLS are separate connections.

Password login supports server TLS and never inherits a collector client certificate. A profile containing a collector certificate requires certificate sign-in or a dashboard identity instead. NATS JWT credentials and NKeys are not supported by the NATS sign-in form.

### Client-certificate sign-in

NATSUI_NATS_LOGIN_CERTIFICATES=1 enables the certificate choice. It works with session-only or background-collection mode. The NATS servers must use TLS verify_and_map to authenticate certificates as configured NATS users. Merely requiring a CA-signed certificate does not establish an individual NATS identity. [NATS mutual TLS documentation](https://docs.nats.io/learn/security/encryption) describes server-side mapping. Enable this option only for profiles whose servers enforce certificate identity mapping.

```yaml
environment:
  NATSUI_URL: tls://nats1:4222,tls://nats2:4222
  NATSUI_NATS_LOGIN: '1'
  NATSUI_NATS_LOGIN_SESSION_ONLY: '1'
  NATSUI_NATS_LOGIN_CERTIFICATES: '1'
  NATSUI_TLS_CA: /run/nats-tls/ca.crt
volumes:
  - ./local-secrets/ca.crt:/run/nats-tls/ca.crt:ro
```

The environment and volume entries extend the existing dashboard service. Publicly trusted server certificates can use system trust without the CA mount. Mounted CA and client-certificate files accept PEM or DER, independent of .crt or .pem names. PEM bundles may contain multiple certificates, while DER files contain one certificate. The same format detection applies to login uploads. PEM preambles, whitespace and a UTF-8 BOM are accepted. The server certificate must match the configured hostname. Login cannot change destinations or trust roots.

On the login page, select Client certificate, then select the user certificate and its unencrypted PEM private key. Certificate uploads accept PEM chains or a single DER certificate, including .crt, .pem, .cer and .der files. Limits are 96 KiB for the certificate and 32 KiB for the key. Encrypted keys and PKCS#12/PFX bundles are not supported.

The browser sends these files to the dashboard backend. Shared deployments require HTTPS through the configured trusted reverse proxy. Local loopback is supported for local use. The backend parses the key in memory and keeps its TLS signing material only for the authenticated session. It never writes uploaded material to disk or uses it as the collector identity. File selections are cleared after each submission. Activity identifies certificate sessions by the leaf certificate's SHA-256 fingerprint, not an inferred NATS username. NATS enforces permissions using the mapped identity.

Configured profiles appear on the login form by name. Profile IDs and display names are public when this option is enabled. Each NATS session belongs to exactly one profile. Switching clusters requires signing out and signing in to the other profile. Dashboard-user profile allowlists apply to dashboard identities, not NATS identities.

## Shared data

Both sharing options default to 0 and are incompatible with session-only mode:

| Setting | Access granted to NATS sessions |
| --- | --- |
| NATSUI_NATS_LOGIN_SHARED_HISTORY=1 | Stored stream and consumer trends, backlog history and recorded incidents from the collector's selected profile |
| NATSUI_NATS_LOGIN_SHARED_MONITORING=1 | HTTP node metrics, connections, subscriptions and routes for the selected profile |

Sharing is profile-wide. Every user that NATS accepts on that profile receives the enabled shared data, including users from another NATS account. It does not inherit the signed-in user's subject permissions. These options suit a trusted operations team and remain disabled for individually restricted deployments.

Historical node observations are removed unless monitoring sharing is also enabled. Live data always uses the session credentials, regardless of either setting. Shared responses require fresh NATS authentication too, so they are unavailable during a broker authentication or connectivity failure.

## Troubleshooting

Start with [natsui doctor](DOCTOR.md#check-inside-the-container) for local configuration. The [password probe](DOCTOR.md#probe-a-nats-user) and [certificate probe](DOCTOR.md#probe-certificate-sign-in) check a supplied identity. [Result examples](DOCTOR.md#read-the-results) explain partial access and suggested corrections.

| Symptom | Check or next step |
| --- | --- |
| The sign-in page only offers a dashboard key | NATSUI_NATS_LOGIN=1 must reach the dashboard container, not just exist in an unused .env entry. The main Compose recipe passes it explicitly. Recreate the service after changing startup configuration. |
| NATS login fails | Check the original username/password, reachable client addresses and TLS trust. The server must require authentication. Collector readiness does not prove that this user's login works. |
| Login succeeds but streams or consumers are unavailable | Check this user's JetStream API publish grants, reply-inbox subscriptions, account and domain. Permission denials can appear as timeouts. The permissions example below lists read-only requests. |
| Streams work but a message cannot be inspected | Message-get permission is separate from inventory permission. The record may also have expired or been deleted. Only an explicit no-record reply establishes absence. |
| Live data works but historical graphs are not shared | Session-only graphs start at sign-in. In background mode, NATSUI_NATS_LOGIN_SHARED_HISTORY defaults to 0. Enable it only when sharing all collector history for that profile is intended, then recreate the service. |
| Nodes or connections are unavailable | In session-only mode, enable Session node monitoring on Nodes. The NATS identity needs system service access as described below. In background mode, HTTP monitoring sharing is a separate deployment opt-in. |
| Streams work, monitoring is enabled, but Nodes stays empty after refresh | A valid JetStream user may lack access to the system account services. Check both the account export/import and the user permissions. [Why this happens](#why-jetstream-access-does-not-imply-node-access) explains the separate requirements. A missing reply can also reflect routing or connectivity. |
| A permitted operation is disabled | The profile also needs NATSUI_ALLOW_WRITES=1. If NATS rejects an enabled operation, check the signed-in user's API permissions. |
| Dashboard settings or user management are unavailable | These require a separate dashboard administrator login. A NATS administrator username does not grant dashboard authority. |
| Certificate login fails | Check that certificate sign-in is enabled, the key matches the certificate, the certificate is valid and trusted by NATS, and verify_and_map resolves it to an allowed user. The browser must send an unencrypted PEM key. |

## Permissions

An existing NATS user needs appropriate JetStream API publish permissions and reply-inbox subscribe permissions for dashboard reads. For example, a read-only inspection identity may use this NATS permissions block:

```text
permissions: {
  publish: { allow: [
    "$JS.API.STREAM.LIST",
    "$JS.API.STREAM.INFO.*",
    "$JS.API.CONSUMER.LIST.*",
    "$JS.API.CONSUMER.INFO.*.*",
    "$JS.API.STREAM.MSG.GET.*"
  ] }
  subscribe: { allow: ["_INBOX.>"] }
}
```

The message-get grant permits payload inspection. Omit it when payload access is not intended. Domain deployments use the configured $JS.DOMAIN.API prefix instead of $JS.API. Exact stream grants can replace wildcards. Permission to publish application messages is separate from permission to list or inspect streams.

### Scoped access in a shared account

Several identities often share one account, each limited to its own streams. Granting `$SYS.REQ.USER.INFO` lets the dashboard read the signed-in user's grants at sign-in instead of probing. Collection then requests only permitted subjects. Streams outside the user's STREAM.INFO grants are left out without warnings, and streams without a CONSUMER.LIST grant are marked on the stream rather than marking the view partial. The session note names the identity and the number of streams it can access.

STREAM.LIST returns every stream in the account with its configuration and state, so identities that should not learn about other streams can deny it. With USER.INFO granted, the dashboard requests each stream named in a STREAM.INFO grant instead:

```text
permissions: {
  publish: {
    allow: [
      "$SYS.REQ.USER.INFO",
      "$JS.API.STREAM.INFO.ORDERS",
      "$JS.API.CONSUMER.LIST.ORDERS",
      "$JS.API.CONSUMER.INFO.ORDERS.>",
      "$JS.API.STREAM.MSG.GET.ORDERS"
    ]
    deny: ["$JS.API.STREAM.LIST", "$JS.API.STREAM.NAMES"]
  }
  subscribe: { allow: ["_INBOX.>"] }
}
```

Grants for requests without a trailing token must name the stream exactly. `$JS.API.CONSUMER.LIST.ORDERS.>` does not match `$JS.API.CONSUMER.LIST.ORDERS`, because `>` requires at least one more token. Wildcard STREAM.INFO grants still need STREAM.LIST to enumerate streams. Grants are read once per sign-in: NATS enforces changed permissions immediately, while the scoped view follows them after signing in again. Without a USER.INFO reply, the dashboard falls back to listing and reports unavailable consumer inventories once, with a marker on each affected stream.

NATS permission denials can surface as request timeouts. The dashboard reports unavailable data without guessing that every timeout is a permission error. Core publishes report acceptance as unconfirmed because a flush is not a publish acknowledgment. JetStream publish acknowledgments can verify storage.

## Session investigation tools

These tools work with session-only mode and use the signed-in NATS identity. No collector account or HTTP monitoring endpoint is required.

| Location | Capability | Evidence and limits |
| --- | --- | --- |
| Consumers | Delivery progress, sustained backlog growth, paused state and acknowledgment pressure | Progress diagnoses require at least 30 seconds of consecutive complete observations with matching resource identities and configuration. Gaps, resets and current counters that contradict history suppress the diagnosis. No worker heartbeat or processing completion is inferred. |
| Streams | Record and byte headroom, oldest retained age, age limits and discard policy | Retained age is not unprocessed-message age. Missing limits remain unavailable. Stream bytes are logical storage, not disk capacity. Per-subject limits and retention policies can remove records sooner. |
| Activity | Session action diary and observed resource changes | Review results include accepted and verified outcomes. Each login has its own memory store. This is not a durable security audit. |
| Activity | Explicit failure-advisory watch for one stream | Watches max-delivery and terminated-message events in eight-second windows. Retained-record inspection uses message-get and never consumes or acknowledges application deliveries. |
| Operations | Reviewed consumer pause and resume | Requires NATS 2.11+, NATSUI_ALLOW_WRITES=1, consumer-info permission and the native pause API grant. Review tokens are single-use, expire after 60 seconds and belong to the login. |
| Settings | Uploaded certificate details | Subject, issuer, validity and SHA-256 fingerprint. An attention notice appears within 30 days of expiry. Neither the private key nor uploaded certificate body is returned. |
| Nodes, Connections, Subjects, Topology | Optional monitoring through NATS system requests | Only permitted replies are displayed. Cluster size comes only from a reported JetStream meta group. Shared HTTP monitoring is never a fallback. |

### Capabilities and management

Data coverage lists every gated capability for the signed-in login: cluster monitoring, stream listing and creation, and per stream and consumer actions. Each row shows whether the login's grants permit it, the exact publish subjects that enable it, and what granting it exposes. Write capabilities also need NATSUI_ALLOW_WRITES=1. Without a USER.INFO reply the same rows show their requirements with an unknown status.

The same evaluation decides which actions are offered. Stream pages offer publish, purge, consumer creation and stream deletion. Consumer pages offer pause, resume and deletion. A retained record on Messages offers republish and delete. Operations only offers actions and streams the login can use. Every action keeps the reviewed preview, confirmation and audit flow, and NATS still decides each request.

| Action | Publish grants | Effect |
| --- | --- | --- |
| Purge stream | `$JS.API.STREAM.PURGE.<stream>` | Removes all records, or those on one subject. Not reversible. |
| Delete record | `$JS.API.STREAM.MSG.DELETE.<stream>` | Removes one record. Verified by a follow-up read. |
| Republish record | `$JS.API.STREAM.MSG.GET.<stream>` and the stream's subjects | Stores a copy in the same stream as a new sequence, with original headers except message ID and expectation headers, plus `Natsui-Republished-From`. |
| Pause or resume consumer | `$JS.API.CONSUMER.INFO.<stream>.<consumer>`, `$JS.API.CONSUMER.PAUSE.<stream>.<consumer>` | Changes future delivery. |

Grant evaluation treats a wildcard requirement as permitted only when a single grant covers it and no denial overlaps it. Coverage spread across several grants is reported as not granted even when NATS would accept each concrete request.

When an identity has VARZ but not ROUTEZ, Replicas draws configured route peers from each server's node metrics. Configured addresses that do not match a server name are counted but not shown.

### Several accounts in one view

NATSUI_ACCOUNTS lists account labels reached at `$JS.<label>.API`, and a profile can set `accounts` to the same list. An administrator account typically imports each account's JetStream API under that prefix, for example `{ service: { account: SYSTEM1, subject: "$JS.API.>" }, to: "$JS.system1.API.>" }`. Collection then merges the signed-in account with every listed account and labels each stream and consumer with its account. The browser addresses such resources as `account/stream`, and requests about one resource carry an `X-Natsui-Account` header so reads and operations use that account's prefix. Accounts that answer with no JetStream are skipped, so identities without the imports keep seeing only their own account. An identity can see a single stream shared from another account when its grants name `$JS.<label>.API.STREAM.INFO.<stream>` and the owning account exports that stream's read requests. Write-only access, such as an imported publish subject captured by another account's stream, does not appear in the writer's view, because the writer cannot read that stream.

Subjects lists stored subjects per stream with record counts, read through STREAM.INFO with a subject filter, so it needs only the stream information grant. Monitoring inventories are offered only after a server answers them at sign-in. Connection lists requested from an application account are scoped by NATS to that account.

### Failure watch

Activity contains a stream selector and Start failure watch. The watch stops starting new windows when the page changes, the tab becomes hidden or Stop watch is selected. An in-flight window can finish within eight seconds. There is a short gap between windows, and events outside active windows cannot be recovered. Silence does not prove subscription permission or absence of failures. Advisory subjects can also receive application publications, so events are signals for investigation, not authoritative audit records or a dead-letter queue.

Each window accepts at most 120 messages, 64 KiB per message and 1 MB in total. Recording limits and early subscription closure mark coverage incomplete. There is one concurrent watch per session and 16 across the process. The incident store retains coverage records alongside accepted events. These subscribe grants enable the two event types for ORDERS:

```text
subscribe: { allow: [
  "_INBOX.>",
  "$JS.EVENT.ADVISORY.CONSUMER.MAX_DELIVERIES.ORDERS.*",
  "$JS.EVENT.ADVISORY.CONSUMER.MSG_TERMINATED.ORDERS.*"
] }
```

Retained-record inspection additionally needs publish permission for $JS.API.STREAM.MSG.GET.ORDERS. A referenced record may have expired or been removed. MaxDeliver does not automatically move a message into another stream.

### Consumer pause and resume

Operations offers Pause consumer and Resume consumer. Pause requires an explicit end time, at most seven days ahead. Resume clears the pause. The preview includes the exact resource and proposed configuration. Apply rechecks the resource revision and then reads the resulting consumer state. Already delivered work remains in flight, and application processes continue running. NATS does not provide an atomic compare-and-pause operation, so an external change between revalidation and application cannot be excluded.

For ORDERS and durable worker, add these publish permissions to the existing read grants:

```text
"$JS.API.CONSUMER.INFO.ORDERS.worker",
"$JS.API.CONSUMER.PAUSE.ORDERS.worker"
```

The pause endpoint handles both pause and resume. Domain deployments replace $JS.API with their configured API prefix.

### Monitoring through NATS

In session-only mode, monitoring starts on at sign-in when a NATS server answers a node request and the identity's known grants permit it, or when grants are unknown. With known grants, each inventory is offered only with its own grant: Nodes needs VARZ, Connections and node connection lists need CONNZ, live subscriptions on Subjects need SUBSZ and route topology on Replicas needs ROUTEZ. VARZ reports server-wide process metrics and totals, while CONNZ and SUBSZ list other identities' clients and interest, so restricted identities can receive VARZ alone. Replicas still shows where an identity's own streams are placed without any monitoring grant. Otherwise Nodes offers Enable monitoring through NATS. This is a per-login choice and starts no background collector. Snapshot requests then ask for VARZ. Connections, Subjects and Topology request CONNZ, SUBSZ and ROUTEZ on demand. Responses use the existing sanitized projections and session trend store. CPU and RAM describe NATS processes.

NATS serves these requests in its system account. A dedicated system-account identity can use the following permissions. An application-account identity needs explicitly configured system-service exports/imports as well. Merely adding publish permissions in an application account does not make system services available. System monitoring can expose information across accounts and should be granted only to identities intended to have that operational visibility.

For a single application account, the system account can export the node requests and the application account can import them. Users without a permissions block then receive node data, while restricted users still need publish grants for these subjects:

```text
accounts {
  SYS: {
    exports: [
      { service: "$SYS.REQ.SERVER.PING.VARZ", response: stream }
      { service: "$SYS.REQ.SERVER.PING.CONNZ", response: stream }
      { service: "$SYS.REQ.SERVER.PING.SUBSZ", response: stream }
      { service: "$SYS.REQ.SERVER.PING.ROUTEZ", response: stream }
      { service: "$SYS.REQ.SERVER.*.CONNZ", response: stream }
    ]
  }
  APP: {
    imports: [
      { service: { account: SYS, subject: "$SYS.REQ.SERVER.PING.VARZ" } }
      { service: { account: SYS, subject: "$SYS.REQ.SERVER.PING.CONNZ" } }
      { service: { account: SYS, subject: "$SYS.REQ.SERVER.PING.SUBSZ" } }
      { service: { account: SYS, subject: "$SYS.REQ.SERVER.PING.ROUTEZ" } }
      { service: { account: SYS, subject: "$SYS.REQ.SERVER.*.CONNZ" } }
    ]
  }
}
```

```text
permissions: {
  publish: { allow: [
    "$SYS.REQ.SERVER.PING.VARZ",
    "$SYS.REQ.SERVER.PING.CONNZ",
    "$SYS.REQ.SERVER.PING.SUBSZ",
    "$SYS.REQ.SERVER.PING.ROUTEZ",
    "$SYS.REQ.SERVER.*.CONNZ"
  ] }
  subscribe: { allow: ["_INBOX.>"] }
}
```

Each request waits up to 1.2 seconds and accepts up to 32 replies and 4 MB total. Missing replies may reflect permissions, network conditions or unavailable servers. When the replies include a JetStream meta group, the coverage indicator compares responders with its reported size and names members that did not answer. Servers without JetStream are not counted. Without a meta group it shows only the number of responders. Inventories request pages of 100 entries where the server supports pagination. ROUTEZ is a bounded projection of its response. Disabling monitoring stops future monitoring requests and keeps earlier observations in the same session history.

### Why JetStream access does not imply node access

A NATS account is an isolated messaging context containing users, subjects and JetStream resources. It is not a username. A user named natsadmin in an application account does not automatically belong to the configured system account.

For example, observer can list streams in OPS while node metrics are served in SYS. Publishing the same subject in both accounts does not connect them. Even broad publish permission inside OPS does not bypass account isolation.

| Requirement | Purpose |
| --- | --- |
| SYS exports a monitoring service to OPS | The service owner permits that account to access it. |
| OPS imports that service from SYS | Requests in OPS can reach the exported service. |
| observer has the required publish and reply-subscription grants | The particular user can send the request and receive its reply. |

All three are needed for this cross-account path. An import exposes the selected service, not the entire system account. It neither copies streams nor changes the user's identity. A user already in SYS needs no cross-account import, but still needs the applicable user permissions.

Natsui's Enable monitoring button requests observations with the signed-in identity. It does not create imports, change broker permissions or substitute an administrator identity. The following tested example supplies a narrow VARZ import for an application account. [NATS account and service configuration](https://github.com/nats-io/nats.docs/blob/master/running-a-nats-service/configuration/securing_nats/accounts.md) describes the underlying boundaries.

### An application account with node metrics

An enabled toggle with no node rows means no usable system reply was observed. It does not mean the cluster is empty. The page distinguishes this from monitoring being off. Reloading retains the session choice. Stream API access alone is not system-service access.

For configuration-file accounts, this example exposes only VARZ node metrics from SYS to an application account named OPS. Merge the relevant entries into the existing broker account configuration, preserving current users and settings. Apply account changes consistently through the deployment's existing NATS configuration process. This is an administrator change, not a dashboard setting.

```text
system_account: SYS
accounts: {
  SYS: {
    exports: [
      { service: "$SYS.REQ.SERVER.PING.VARZ", accounts: [OPS], response_type: stream }
    ]
  }
  OPS: {
    jetstream: enabled
    imports: [
      { service: { account: SYS, subject: "$SYS.REQ.SERVER.PING.VARZ" } }
    ]
    users: [
      {
        user: observer
        password: REPLACE_WITH_A_STRONG_PASSWORD
        permissions: {
          publish: { allow: ["$JS.API.STREAM.LIST", "$SYS.REQ.SERVER.PING.VARZ"] }
          subscribe: { allow: ["_INBOX.>"] }
        }
      }
    ]
  }
}
```

The response_type permits multiple server replies. This example allows stream listing and node metrics only. Consumer details, message bodies, connections, subscriptions and routes need their separate grants. For connection inventories, an administrator can explicitly export/import the required system subjects listed above and grant them to the intended user. System data can reveal activity across accounts, so account access should reflect the intended operational visibility.

JWT/operator deployments configure equivalent exports/imports and permissions through their account authority. The [NATS account documentation](https://github.com/nats-io/nats.docs/blob/master/running-a-nats-service/configuration/securing_nats/accounts.md) describes the distinction between service imports and ordinary publish grants. Natsui does not infer or broaden those grants.

After applying the intended access, sign in and enable Session node monitoring. The [doctor identity probe](DOCTOR.md#probe-a-nats-user) can check the same VARZ request independently. A working result confirms responding nodes, not complete cluster coverage. Network failures and missing reply permissions remain other possible reasons for no data.

## Session lifetime and limits

Passwords remain in process memory for an eight-hour session and are not stored in SQLite, browser storage or logs. Logout and session replacement discard them. Expired session records are swept within one minute. Process restart signs users out.

Each HTTP data request opens a fresh NATS connection with session credentials. This applies password changes and NATS revocation at the next authentication attempt without maintaining a second permission cache. An already-running request may finish. Enforcement across nodes depends on the cluster's authentication configuration being consistent.

The initial implementation allows 32 dashboard sessions, four concurrent NATS login attempts and 32 concurrent NATS-session data requests. Login attempts share the dashboard's 30-per-minute limit. Extra connections increase NATS connection counters and authentication work. NATS sessions use polling instead of collector-driven SSE notifications.

The [Ansible guide](ANSIBLE.md) exposes the same options as deployment variables. No interactive provisioning is required.
