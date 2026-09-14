# Local beta access boundaries

The native dashboard listens on IPv4 loopback. Docker mode listens on the container network and requires host-loopback port publication. Host and Origin checks reject unexpected browser origins and DNS rebinding; these checks are not authentication. Without NATSUI_AUTH_TOKEN_FILE, any process or person with access to the allowed HTTP endpoint can inspect permitted payloads and change local dashboard settings. With it configured, protected routes require a valid operator session. Public reverse-proxy deployment and shared users remain outside this beta's access model.

## Dashboard authentication

NATSUI_AUTH_TOKEN_FILE enables a generated-access-key login independent of NATS credentials. Sessions use random tokens, HttpOnly/SameSite=Strict cookies, an eight-hour server-enforced lifetime and server-side sign-out revocation. Restart invalidates all sessions. The key is loaded at startup; missing, unreadable or malformed configured files fail closed. Sign-in attempts are bounded to 30 per minute across the instance, with a 1 KiB body limit and at most 32 active sessions.

Authentication protects dashboard and API access, including payload inspection, history, settings and reviewed writes. Login assets and health/readiness endpoints remain public. The shared key grants one configured operator identity, without per-user roles or attribution. Access-key and session digests remain in memory; the key is not written to SQLite or browser storage. Session cookies are sent only to the configured browser host; cookie scope does not isolate different ports on the same host.

The existing localhost HTTP access model remains. Cookies omit Secure for this loopback-only mode. This is not approval to expose HTTP or forward untrusted requests through a proxy; public HTTPS/origin configuration and team authorization remain unimplemented. Host and Docker administrators can access or replace the secret and remain trusted. Authentication does not reduce NATS permissions or isolate message payload access by user.

[Setup, secret rotation and persistent volumes](docs/SETUP.md) contains deployment commands. Design references: [OWASP session management](https://cheatsheetseries.owasp.org/cheatsheets/Session_Management_Cheat_Sheet.html) and [REST transport/access controls](https://cheatsheetseries.owasp.org/cheatsheets/REST_Security_Cheat_Sheet.html).

## NATS identity

The dashboard's default read-only behavior does not reduce the permissions of its NATS credentials. Native configuration writes require the explicit NATSUI_ALLOW_WRITES=1 startup option; broker authorization remains authoritative. A dedicated, restricted identity belongs in the application account being inspected. A system account is not required for JetStream inventory.

`deploy/permissions.conf` is a permissions object for a NATS user. It allows only:

- STREAM.LIST, STREAM.INFO.*, CONSUMER.LIST.* and STREAM.MSG.GET.* requests under $JS.API.
- Response inbox subscriptions and $JS.EVENT.ADVISORY.> subscriptions.

All other publish and subscribe subjects are excluded by the allowlists. Application publish subjects, consumer pull requests, acknowledgments and resource mutations are absent. Message payload access can be removed by deleting STREAM.MSG.GET.* from the template; record browsing and inspection will then fail with an upstream error while inventory remains available. Advisory access is optional and can be removed independently.

For a JetStream domain, replace the $JS.API prefix with $JS.<domain>.API. Scoped identities can replace stream wildcards with explicit names; STREAM.LIST and CONSUMER.LIST still reveal the inventory permitted by the broker API. This template is for subject permissions, not complete account/JWT provisioning. Account exports/imports and operator policies remain deployment-specific.

NATSUI_URL accepts up to 32 comma-separated seeds for one cluster/account. Startup races authenticated connections and keeps the first success. Every attempt retains the complete seed list for reconnects. URL authentication is shared across seeds, and conflicting inline credentials are rejected. A tls:// seed or explicit TLS configuration requires encryption on all peers. NATSUI_MONITOR_URLS remains an independently configured HTTP(S) list that is collected per endpoint.

Native permission and TLS references: [NATS authorization](https://docs.nats.io/running-a-nats-service/configuration/securing_nats/authorization), [JetStream API](https://docs.nats.io/reference/reference-protocols/nats_api_reference), [NATS TLS](https://docs.nats.io/running-a-nats-service/configuration/securing_nats/tls).

## Credentials and TLS

JWT credentials stay server-side through NATSUI_CREDS. A URL can instead supply username/password or a token, with reserved characters percent-encoded. URL authentication is explicitly applied to the client. Connection diagnostics expose failure categories rather than raw addresses, authentication material or nested TLS errors.

A private CA uses NATSUI_TLS_CA. Mutual TLS additionally uses NATSUI_TLS_CERT and NATSUI_TLS_KEY. Configured TLS files require TLS; certificate verification cannot be disabled through dashboard configuration. TLS settings apply to the NATS protocol connection. The optional HTTP monitoring client uses its own standard HTTPS trust and does not inherit NATS client certificates or private CA settings.

Container configuration example, supplied as a local Compose override:

```yaml
services:
  dashboard:
    environment:
      NATSUI_URL: tls://broker.internal:4222
      NATSUI_CREDS: /run/nats/observer.creds
      NATSUI_TLS_CA: /run/nats/ca.pem
    volumes:
      - ./local-secrets:/run/nats:ro
```

The container UID 10001 must be able to read mounted files. Secret directories and actual credentials must remain outside source control. Monitoring endpoints should remain on a private network; many NATS monitoring deployments have no HTTP authentication. Native monitoring can reveal connections and subjects from accounts outside the JetStream identity.

## History identity and migration

Profile bindings store SHA-256 digests, not raw connection credentials. Different configured seed sets, users, domains or credential-file contents require distinct profiles. Seed ordering and duplicate addresses do not change the binding. Existing single-seed profiles retain their previous binding, but expanding to multiple seeds requires a new profile name. A server restart, the seed that wins the startup race, or a cluster-discovered peer does not change the configured binding. Password rotation for an unchanged URL username is supported. Replacing an account behind an unchanged endpoint/username cannot be detected automatically by this guard.

For existing unbound history, NATSUI_ADOPT_LEGACY_PROFILE=1 permits initial binding after confirming its origin. This option never overrides an existing conflicting binding. A new profile or data directory is the safe default when origin is uncertain. The additive schema upgrade preserves existing observations but older application versions cannot open the newer schema. A stopped-process backup before upgrading preserves rollback options.

## Data handling

Message payloads and headers are fetched on demand and are not stored by Natsui. The browser can display them and the API transmits them locally. Operational history and exports contain resource names, counters and allowlisted incident context. Filesystem access to those files remains sensitive. Raw stream/consumer configuration dialogs reflect broker-provided metadata and can include application-supplied descriptions or metadata.

## Remaining deployment gate

Team access still requires individual identities, payload-access authorization, settings roles and an explicit public-origin policy. Native broker mutations additionally require authorization and reviewable before/after changes. A NATS configuration supervisor is a separate deployment capability, not part of this application.

## Optional local configuration editing

`NATSUI_ALLOW_WRITES=1` enables reviewed edits under Settings for the configured profile. It does not grant NATS permissions. Existing restricted connections can continue to use `deploy/permissions.conf`, which includes consumer INFO reads but no mutation subjects.

For a selected stream and consumer, additional publish permissions can be restricted to:

```text
$JS.API.STREAM.UPDATE.ORDERS
$JS.API.CONSUMER.CREATE.ORDERS.worker
```

Names are examples. JetStream domains require the corresponding `$JS.<domain>.API` prefix. The consumer endpoint authorizes creation as well as updates at the NATS permission layer; this dashboard sends only update requests. Avoid blanket management permissions merely to edit one resource.

The HTTP listener remains local operator access with optional access-key login. Other local processes are inside this trust boundary. Enabling writes is not suitable for exposing the dashboard publicly. Preview tokens, request headers and Host/Origin checks protect the reviewed local workflow; they do not replace authentication. See [settings ownership and edit guarantees](SETTINGS_AND_ACCESS.md#implemented-jetstream-editor).
