# Dashboard users, login links and HTTPS

Dashboard identities are independent of NATS identities. NATS permissions remain the upper bound on every broker operation. Natsui roles further restrict access through the dashboard.

## Shorter local login

The authenticated Compose stack initializes a random bootstrap key on its first start. Subsequent starts validate and retain that key. Invalid existing key files stop initialization. The initializer never prints the permanent key.

~~~sh
docker compose -f compose.auth.yaml up -d --wait
docker compose -f compose.auth.yaml exec dashboard natsui login
~~~

The command proves possession of the configured key and prints a one-time link valid for 60 seconds. Opening the link exchanges a random ticket for an eight-hour session. The browser removes the ticket from the address before exchanging it. Tickets are single-use, bounded, and invalidated by dashboard restart. A link grants access until used or expired, so it belongs only in a private terminal. If the host port differs from 4321, the link must use that published port. An SSH tunnel can retain local port 4321.

The native equivalent is natsui login with NATSUI_AUTH_TOKEN_FILE and NATSUI_PORT configured for the running server. Existing access-key login remains supported. The cluster tryout and standalone web simulation remain keyless.

## Named dashboard users

Settings contains Dashboard users when authentication is enabled. A bootstrap-key session is an administrator. Named users have generated 256-bit access keys that are shown only after creation or rotation.

- Viewer can read observations, inspect retained records and browse buckets.
- Operator can also perform enabled NATS configuration and messaging operations.
- Admin can also change dashboard settings, manage dashboard users and access configured deployment controllers.

Role changes, disabling, deletion and key rotation invalidate that user's sessions and outstanding login tickets. Re-enabling a user does not revive old sessions. Revision checks reject updates made from stale user lists. The bootstrap key remains a recovery administrator and cannot be deleted through the user table. Rotating that key requires replacing its protected file and restarting the dashboard, as in the Ansible guide.

Named user records contain key digests, roles and revisions in access.sqlite3 inside NATSUI_DATA_DIR. Permanent user keys are never stored in plaintext. The entire data directory must persist and be included in stopped backups. Access keys are not human passwords or multi-factor authentication. Optional [OIDC sign-in](OIDC.md) maps explicitly configured provider subjects to these same local users. All configured profiles use the same dashboard identity store. Optional [profile membership](PROFILES.md) restricts resource access.

## HTTPS through a trusted reverse proxy

NATSUI_PUBLIC_URL opts into one explicit HTTPS origin. Startup requires dashboard authentication. Host and Origin must match that origin. Forwarded headers do not expand the accepted host set. Cookies gain Secure, and responses include a restrictive content security policy and no-referrer policy.

~~~yaml
services:
  dashboard:
    environment:
      NATSUI_PUBLIC_URL: https://natsui.example.com
      NATSUI_AUTH_TOKEN_FILE: /run/natsui-auth/access.key
    # Keep the existing persistent data and read-only credential mounts.
    # Publish only to loopback for a proxy on the same host.
    ports: ['127.0.0.1:4321:4321']
~~~

A Caddy proxy running on the same host can use:

~~~text
natsui.example.com {
    reverse_proxy 127.0.0.1:4321
}
~~~

The DNS name must resolve to that host and the proxy must obtain a trusted certificate. A proxy running in a container instead connects to dashboard:4321 on a dedicated private Docker network. In that arrangement the dashboard does not need a published port. Only the proxy's HTTPS endpoint should be externally reachable. The proxy must preserve the original Host header. TLS termination does not authorize access by itself.

The dashboard's direct HTTP listener is an internal hop. NATSUI_PUBLIC_URL does not enable TLS on that listener. Loopback health checks and the authenticated login-ticket CLI endpoint remain available for local administration. Other dashboard HTTP requests require the configured public Host and browser sessions use Secure cookies.


## Optional NATS login

[NATS-backed login](NATS_LOGIN.md) accepts existing NATS usernames and passwords. Live requests use the signed-in user credentials. Collector history and HTTP monitoring require separate deployment opt-ins. Dashboard administration retains its own identity boundary.
