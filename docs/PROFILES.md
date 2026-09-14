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

Dashboard user identities are shared across configured profiles. Viewer, operator and admin roles do not provide per-profile access restrictions. Separate dashboard deployments are appropriate when different teams must not access the same set of profiles.
