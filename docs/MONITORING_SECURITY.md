# Monitoring endpoint security

NATS client connections and HTTP monitoring use separate transports, addresses and credentials. NATSUI_URL seeds and discovery do not discover monitoring ports. Simple private monitoring deployments continue to use comma-separated NATSUI_MONITOR_URLS.

## Per-endpoint HTTPS configuration

NATSUI_MONITOR_CONFIG_FILE selects a JSON file instead of NATSUI_MONITOR_URLS. Each entry owns its CA, client identity and HTTP credential. Credentials never fall through to another endpoint and redirects are rejected. Hostname verification remains enabled.

~~~json
[
  {
    "url": "https://nats1-monitor:8443",
    "ca_file": "/run/monitor/ca.pem",
    "cert_file": "/run/monitor/client.pem",
    "key_file": "/run/monitor/client.key"
  },
  {
    "url": "https://nats2-monitor:9443",
    "ca_file": "/run/monitor/ca.pem",
    "username": "observer",
    "password_file": "/run/monitor/password"
  },
  {
    "url": "https://nats3-monitor:8443",
    "ca_file": "/run/monitor/ca.pem",
    "bearer_file": "/run/monitor/token"
  }
]
~~~

HTTP Basic or Bearer authentication generally belongs to a monitoring reverse proxy. Configuring these fields does not add an HTTP user database to NATS itself. The optional client certificate and key must appear together. Basic and Bearer credentials are mutually exclusive. TLS settings or credentials require an HTTPS endpoint. Configuration errors fail dashboard startup rather than silently removing protection.

~~~yaml
services:
  dashboard:
    environment:
      NATSUI_MONITOR_CONFIG_FILE: /run/monitor/endpoints.json
      NATSUI_MONITOR_URLS: ''
    volumes:
      - ./monitor:/run/monitor:ro
~~~

The directory contains only the required endpoint JSON, public certificates and protected client credentials. Files must be readable by container UID/GID 10001. No CA private key or server private key belongs in that mount. The JSON file contains file paths, not inline passwords or tokens. Private keys must be unencrypted PEM files because startup is noninteractive.

An Ansible deployment can provision the same files with Vault-backed copy tasks, no_log: true and diff: false. A change requires container recreation because startup loads the HTTP client configuration once. Monitoring collects every configured endpoint independently. A rejected certificate or unavailable endpoint appears as missing coverage, without zero-valued replacement metrics.
