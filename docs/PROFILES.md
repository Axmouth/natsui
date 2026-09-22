# Multiple connection profiles

NATSUI_PROFILES_FILE defines up to seven additional profiles alongside the default environment-configured connection. Every profile keeps its own connection, monitoring clients, settings and historical database. All configured profiles are collected in the background, so adding profiles increases collection work even when no browser is viewing them.

```json
{
  "profiles": [
    {
      "id": "staging",
      "name": "Staging",
      "urls": ["tls://stage1:4222", "tls://stage2:4222"],
      "credentials": "/run/staging/observer.creds",
      "ca": "/run/staging/ca.pem",
      "certificate": "/run/staging/client.pem",
      "key": "/run/staging/client.key",
      "monitor_urls": ["http://stage1:8222", "http://stage2:8222"],
      "allow_writes": false
    }
  ]
}
```

Omit unused credential and TLS fields. A profile can set domain for a JetStream domain and monitor_config_file for independent HTTPS monitoring credentials. monitor_config_file and monitor_urls are mutually exclusive. All paths refer to files inside the dashboard container.

```yaml
services:
  dashboard:
    environment:
      NATSUI_PROFILES_FILE: /run/profiles/profiles.json
    volumes:
      - ./profiles.json:/run/profiles/profiles.json:ro
      - ./staging:/run/staging:ro
```

Profile identities are deployment configuration. Adding or changing them requires container recreation. The primary profile uses the existing NATSUI_URL and related environment settings. Additional SQLite databases live under /data/profiles/PROFILE_ID. Backups must include the complete /data directory. Changing a profile's NATS identity requires a new profile ID and name rather than relabeling existing history.

The top-bar selector changes the active profile for one browser tab. API requests carry that tab's explicit profile ID. Switching another tab cannot retarget an existing review or publish form. A removed profile produces an unavailable-context error while the shared application shell remains available for selecting another profile.

Dashboard user identities are shared across profiles. Optional NATSUI_ACCESS_POLICY_FILE restricts profile membership for named users. Roles remain deployment-wide. With no policy file, every authenticated user can access all configured profiles.

## Profile access policy

Example profile-access.json:

```json
{"profiles":{"default":["operator-alex"],"staging":["operator-alex","reader-sam"]}}
```

Mount the file read-only and set NATSUI_ACCESS_POLICY_FILE to its container path. Dashboard authentication is mandatory. Once a policy is configured, omitted profiles and users are denied. Restrictions apply to profile listing, selection, direct API requests and live update streams. The bootstrap administrator retains every profile for recovery. User IDs must match the named dashboard identities, including users mapped through OIDC.

Admin can manage global users and controllers, including rotating another user's key. Profile membership therefore does not isolate mutually untrusted administrators. Separate dashboard deployments and signing authorities are appropriate for that boundary. Usernames in this policy are deployment-owned membership labels, so recreating a username preserves its membership. OIDC additionally pins the identity revision to prevent a stale provider mapping attaching to that replacement.

Policy changes require container recreation. Existing browser tabs then reconnect and recheck access. A forbidden selected profile leaves the profile selector available for choosing an allowed one.


## Optional NATS login

[NATS-backed login](NATS_LOGIN.md) accepts existing NATS usernames and passwords. Live requests use the signed-in user credentials. Collector history and HTTP monitoring require separate deployment opt-ins. Dashboard administration retains its own identity boundary.
