# NATS-backed login

NATS-backed login is an optional username/password sign-in method. NATS validates the credentials. Every live stream, consumer, message, bucket and operation request uses those credentials. The background collector continues to use the deployment's configured NATS identity.

NATS permissions remain authoritative. Stream listing returns whatever the NATS API permits, without inferred ownership or application-subject filtering. Failed live requests never retry with collector credentials. Unavailable data is not an empty inventory.

## Enable the login option

Add these environment settings to the dashboard service in an existing Compose deployment:

```yaml
environment:
  NATSUI_URL: nats://observer:REPLACE_WITH_COLLECTOR_PASSWORD@nats1:4222,nats://nats2:4222
  NATSUI_NATS_LOGIN: '1'
  NATSUI_NATS_LOGIN_SHARED_HISTORY: '0'
  NATSUI_NATS_LOGIN_SHARED_MONITORING: '0'
  NATSUI_ALLOW_WRITES: '0'
```

NATSUI_URL supplies fixed server addresses and the background collector's credentials. NATSUI_CREDS can supply the collector identity instead. Submitted login credentials replace collector authentication for live requests. Hosts cannot be supplied through the login form.

The environment block extends the existing deployment. Its persistent /data volume and private network remain required. Apply the deployment with docker compose up -d, then open the dashboard and select the cluster, NATS username and NATS password. Real NATS authentication must be enabled. The simulated demo cannot use NATS login.

NATSUI_ALLOW_WRITES=1 enables the existing reviewed edit workflow. NATS still decides whether the signed-in user can perform the requested operation. A successful login alone grants no NATS API permissions.

NATSUI_AUTH_TOKEN_FILE is optional in a deployment that only needs NATS login. Keeping a mounted dashboard access key provides access to dashboard administration and the existing one-time login CLI. NATS sessions cannot change dashboard users, settings, profiles or controller configuration and cannot access controller APIs or the administrative activity log.

## TLS and profiles

NATSUI_TLS_CA and NATSUI_TLS_REQUIRED apply to the login connection as well as collection. The [setup guide](SETUP.md) includes sample CA mounts and HTTPS deployment instructions. Dashboard HTTPS and NATS TLS are separate connections.

The initial NATS login mode supports username/password authentication with optional server TLS. It rejects profiles containing NATSUI_TLS_CERT/NATSUI_TLS_KEY or profile client certificates. A collector certificate could otherwise select an identity different from the submitted user. NATS JWT credentials, NKeys and mutual TLS login require separate credential handling and are not implemented by this mode.

Configured profiles appear on the login form by name. Profile IDs and display names are public when this option is enabled. Each NATS session belongs to exactly one profile. Switching clusters requires signing out and signing in to the other profile. Dashboard-user profile allowlists apply to dashboard identities, not NATS identities.

## Shared data

Both sharing options default to 0:

| Setting | Access granted to NATS sessions |
| --- | --- |
| NATSUI_NATS_LOGIN_SHARED_HISTORY=1 | Stored stream and consumer trends, backlog history and recorded incidents from the collector's selected profile |
| NATSUI_NATS_LOGIN_SHARED_MONITORING=1 | HTTP node metrics, connections, subscriptions and routes for the selected profile |

Sharing is profile-wide. Every user that NATS accepts on that profile receives the enabled shared data, including users from another NATS account. It does not inherit the signed-in user's subject permissions. These options suit a trusted operations team and remain disabled for individually restricted deployments.

Historical node observations are removed unless monitoring sharing is also enabled. Live data always uses the session credentials, regardless of either setting. Shared responses require fresh NATS authentication too, so they are unavailable during a broker authentication or connectivity failure.

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

NATS permission denials can surface as request timeouts. The dashboard reports unavailable data without guessing that every timeout is a permission error. Core publishes report acceptance as unconfirmed because a flush is not a publish acknowledgment. JetStream publish acknowledgments can verify storage.

## Session lifetime and limits

Passwords remain in process memory for an eight-hour session and are not stored in SQLite, browser storage or logs. Logout and session replacement discard them. Expired session records are swept within one minute. Process restart signs users out.

Each HTTP data request opens a fresh NATS connection with session credentials. This applies password changes and NATS revocation at the next authentication attempt without maintaining a second permission cache. An already-running request may finish. Enforcement across nodes depends on the cluster's authentication configuration being consistent.

The initial implementation allows 32 dashboard sessions, four concurrent NATS login attempts and 32 concurrent NATS-session data requests. Login attempts share the dashboard's 30-per-minute limit. Extra connections increase NATS connection counters and authentication work. NATS sessions use polling instead of collector-driven SSE notifications.

The [Ansible guide](ANSIBLE.md) exposes the same options as deployment variables. No interactive provisioning is required.
