# Check a deployment with natsui doctor

The natsui binary includes a deployment checker. It shares the dashboard's login-policy, profile, connection, TLS and startup-setting validators. No source checkout or separate diagnostic image is required. NATS sign-in with session-only history is the recommended path and the default in compose.nats-login.yaml. Existing runtime defaults remain unchanged.

## Check inside the container

From the deployment folder, run:

```sh
docker compose --env-file .env -f compose.nats-login.yaml exec -T dashboard natsui doctor --require-session-only
```

Include the same Compose overlays used for deployment, such as -f compose.tls.yaml. The command uses the container's environment, mounted files and runtime identity. It does not load .env itself or inspect the running dashboard process. After deployment-file edits, recreate the service before checking its new environment.

A service that cannot start can still be checked with a temporary container:

```sh
docker compose --env-file .env -f compose.nats-login.yaml run --rm --no-deps dashboard doctor --require-session-only
```

Compose may initialize deployment resources, including volumes, for the temporary container. The doctor process itself does not create or open SQLite, change configuration or contact NATS unless --probe is supplied. Native deployments use natsui doctor in the same environment and under the same OS identity as the service.

Each result has a check ID, status, explanation and correction when needed. Examples include conflicting shared-history flags, collector credentials in a session-only profile, unreadable trust files and disabled writes. A missing NATSUI_URL produces a reminder that the default loopback address refers to the dashboard container itself.

## Probe a NATS user

Local validation cannot establish broker permissions. An explicit probe authenticates with the supplied NATS identity through the same connection code used by sign-in. Collector credentials are never used as a fallback.

Create a private UTF-8 file named local-secrets/probe-password containing only the NATS password, with an optional final newline. Keep it outside source control. Mount it read-only through a file named compose.doctor.yaml:

```yaml
services:
  dashboard:
    volumes:
      - ./local-secrets/probe-password:/run/doctor/password:ro
```

The file must be readable by container UID/GID 10001. The [mounted-file permissions guide](SETUP.md#mounted-file-permissions) covers Linux and Docker Desktop. Then run:

```sh
docker compose --env-file .env -f compose.nats-login.yaml -f compose.doctor.yaml run --rm --no-deps dashboard doctor --probe --username alice --password-file /run/doctor/password --require-session-only
```

Include the TLS overlay when needed. Passwords are not accepted directly as command-line arguments. The report omits passwords, private keys, connection URLs, mounted paths and raw broker responses. Resource names explicitly supplied to the command can appear in suggested API subjects.

The probe checks authentication, stream listing and system monitoring. For resource-specific checks, add --stream ORDERS and optionally --consumer fulfilment. These request stream information, consumer listing and consumer information without pulling or acknowledging messages. Add --profile staging to select an additional configured profile. The default profile is otherwise selected. All profiles receive local validation, but only the selected profile is probed.

System monitoring may be unavailable to a working application user. A timeout does not establish a permission denial. Guidance names the requested API subject and reply-inbox subscription, with reminders to check the account, JetStream domain and system-service imports. Access to one API does not establish access to every resource or operation.

## Probe certificate sign-in

Enable NATSUI_NATS_LOGIN_CERTIFICATES=1 and configure NATS certificate identity mapping as described in the [certificate sign-in guide](NATS_LOGIN.md#client-certificate-sign-in). Server trust remains deployment-owned. Mount the probe's user certificate and private key through compose.doctor.yaml:

```yaml
services:
  dashboard:
    volumes:
      - ./local-secrets/client.crt:/run/doctor/client.crt:ro
      - ./local-secrets/client.key:/run/doctor/client.key:ro
```

```sh
docker compose --env-file .env -f compose.nats-login.yaml -f compose.tls.yaml -f compose.doctor.yaml run --rm --no-deps dashboard doctor --probe --certificate-file /run/doctor/client.crt --key-file /run/doctor/client.key --require-session-only
```

Certificates accept PEM, PEM bundles or a single DER certificate, including .crt files. Private keys must be matching, unencrypted PEM. These probe mounts must not be assigned to NATSUI_TLS_CERT or NATSUI_TLS_KEY in session-only mode. The probe cannot inspect server configuration to prove verify_and_map is enabled. Successful authentication alone does not establish individual certificate mapping or sufficient API permissions.

## Automation and exit codes

Add --json for a report with format natsui-doctor-v1, version, mode and checks. Each check contains id, status, detail and guidance. Check IDs identify capabilities independently of display wording.

| Status | Meaning |
| --- | --- |
| pass | The specific check succeeded. Its stated scope still applies. |
| fail | Invalid local configuration, failed authentication or an explicit service authorization rejection was observed. |
| warning | A recommendation or compatibility concern needs attention. |
| unknown | Available evidence does not establish this capability. |
| disabled | The deployment has not enabled this capability. |

Exit 0 means no failed checks, not full readiness. Exit 1 means at least one failed check. Exit 2 means invalid command arguments. --require-session-only turns a different configured login mode into a failed check. Automation requiring a specific live capability must also inspect that check's status.

An Ansible task for a deployed session-only stack:

```yaml
- name: Validate the NATS session-only deployment
  ansible.builtin.command:
    argv:
      - docker
      - compose
      - --env-file
      - .env
      - -f
      - compose.nats-login.yaml
      - exec
      - -T
      - dashboard
      - natsui
      - doctor
      - --json
      - --require-session-only
    chdir: /opt/natsui
  register: natsui_doctor
  changed_when: false
  failed_when: natsui_doctor.rc != 0
```

The directory and file list must match the deployed stack. The existing [dashboard-key Ansible example](ANSIBLE.md) remains an alternative deployment mode and should omit --require-session-only unless converted to NATS session-only sign-in.

## Boundaries

The checker does not restart services, rewrite deployment files, provision identities, publish application messages or test writes. Read probes can produce normal broker connection, authorization and API-usage telemetry. It does not validate reverse-proxy routing, browser HTTPS, SQLite integrity, volume durability or every optional OIDC/controller setting. It does not inspect the Docker socket or NATS configuration files.

The connection uses the first successful configured seed. A successful connection does not establish that all seeds work. One system reply does not establish full cluster coverage. Reported unknowns remain unknown rather than becoming healthy empty inventories. This command complements the readiness endpoint and browser sign-in checks.
