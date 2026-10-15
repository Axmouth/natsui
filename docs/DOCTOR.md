# Check a deployment with natsui doctor

Run natsui doctor to find missing or conflicting settings and get a suggested correction. The command is included in the normal binary and container image. It uses the same configuration checks as the dashboard. NATS sign-in with session-only history is the recommended path and the default in compose.nats-login.yaml. Existing runtime defaults remain unchanged.

## Choose a check

| Task | Start here |
| --- | --- |
| Check settings and mounted files | [Container check](#check-inside-the-container), no NATS connection |
| Service fails before the sign-in page appears | [Temporary container check](#check-inside-the-container) |
| Check a NATS username and password | [Password probe](#probe-a-nats-user) |
| Check a user certificate and key | [Certificate probe](#probe-certificate-sign-in) |
| Understand a failure or partial access | [Read the results](#read-the-results) |
| Add a deployment check to Ansible | [JSON and exit codes](#automation-and-exit-codes) |

These examples follow the [main setup guide](SETUP.md) and use its .env file, Docker network and Compose recipe. Commands work in PowerShell, Bash and Zsh. Replace alice, ORDERS, fulfilment and staging with actual configured identities and resource names when used. Probe files and Compose overlays belong in the same deployment folder.

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

Create a private UTF-8 file named local-secrets/probe-password containing only the NATS password, with an optional final newline. Keep it outside source control. Download [compose.doctor-password.yaml](../deploy/compose.doctor-password.yaml) into the deployment folder. It mounts that file read-only:

```yaml
services:
  dashboard:
    volumes:
      - type: bind
        source: ./local-secrets/probe-password
        target: /run/doctor/password
        read_only: true
        bind:
          create_host_path: false
```

The file must be readable by container UID/GID 10001. The [mounted-file permissions guide](SETUP.md#mounted-file-permissions) covers Linux and Docker Desktop. Then run:

```sh
docker compose --env-file .env -f compose.nats-login.yaml -f compose.doctor-password.yaml run --rm --no-deps dashboard doctor --probe --username alice --password-file /run/doctor/password --require-session-only
```

For the private-CA TLS setup from the main guide, use the complete file list:

```sh
docker compose --env-file .env -f compose.nats-login.yaml -f compose.tls.yaml -f compose.doctor-password.yaml run --rm --no-deps dashboard doctor --probe --username alice --password-file /run/doctor/password --require-session-only
```

The CA file is local-secrets/ca.pem and NATSUI_URL uses tls:// addresses. Passwords are not accepted directly as command-line arguments. The report omits passwords, private keys, connection URLs, mounted paths and raw broker responses. Resource names explicitly supplied to the command can appear in suggested API subjects.

For a specific stream and consumer, using the same TLS setup:

```sh
docker compose --env-file .env -f compose.nats-login.yaml -f compose.tls.yaml -f compose.doctor-password.yaml run --rm --no-deps dashboard doctor --probe --username alice --password-file /run/doctor/password --stream ORDERS --consumer fulfilment --require-session-only
```

The probe checks authentication, stream listing and system monitoring. For resource-specific checks, add --stream ORDERS and optionally --consumer fulfilment. These request stream information, consumer listing and consumer information without pulling or acknowledging messages. Add --profile staging to select an additional configured profile. The default profile is otherwise selected. All profiles receive local validation, but only the selected profile is probed.

System monitoring may be unavailable to a working application user. A timeout does not establish a permission denial. Guidance names the requested API subject and reply-inbox subscription, with reminders to check the account, JetStream domain and system-service imports. Access to one API does not establish access to every resource or operation.

## Probe certificate sign-in

Enable NATSUI_NATS_LOGIN_CERTIFICATES=1 and configure NATS certificate identity mapping as described in the [certificate sign-in guide](NATS_LOGIN.md#client-certificate-sign-in). Server trust remains deployment-owned. Download [compose.doctor-certificate.yaml](../deploy/compose.doctor-certificate.yaml) into the deployment folder. It mounts the probe's user certificate and key:

```yaml
services:
  dashboard:
    volumes:
      - type: bind
        source: ./local-secrets/client.crt
        target: /run/doctor/client.crt
        read_only: true
        bind:
          create_host_path: false
      - type: bind
        source: ./local-secrets/client.key
        target: /run/doctor/client.key
        read_only: true
        bind:
          create_host_path: false
```

```sh
docker compose --env-file .env -f compose.nats-login.yaml -f compose.tls.yaml -f compose.doctor-certificate.yaml run --rm --no-deps dashboard doctor --probe --certificate-file /run/doctor/client.crt --key-file /run/doctor/client.key --require-session-only
```

Certificates accept PEM, PEM bundles or a single DER certificate, including .crt files. Private keys must be matching, unencrypted PEM. These probe mounts must not be assigned to NATSUI_TLS_CERT or NATSUI_TLS_KEY in session-only mode. The probe cannot inspect server configuration to prove verify_and_map is enabled. Successful authentication alone does not establish individual certificate mapping or sufficient API permissions.

## Check native HTTPS

The [native HTTPS recipe](HTTPS.md) serves the dashboard without a proxy. doctor checks paired dashboard certificate/key files, the public HTTPS origin and CA material. After startup, natsui healthcheck additionally verifies the running listener using the configured hostname and trust. Broker TLS uses separate NATSUI_TLS_* settings.

## Read the results

A local check can report these lines together:

```text
[PASS] login.session_only: Session-only mode is configured. History is private to each login and transient.
[UNKNOWN] probe.identity: No network connection was attempted. Broker authentication, API access and certificate mapping remain untested.
```

That is expected. The configuration is suitable for session-only mode, but no user has been tested yet. Run an explicit password or certificate probe to check access.

| Result | Meaning and next step |
| --- | --- |
| login.policy fails with a shared-history conflict | Keep NATSUI_NATS_LOGIN=1 and NATSUI_NATS_LOGIN_SESSION_ONLY=1. Set both NATSUI_NATS_LOGIN_SHARED_HISTORY and NATSUI_NATS_LOGIN_SHARED_MONITORING to 0, then recreate the service. |
| profile.default.connection names an unreadable NATSUI_TLS_CA | Check that local-secrets/ca.pem exists, include compose.tls.yaml and make the mounted file readable by UID/GID 10001. |
| probe.authentication passes but probe.stream_list is unknown | The login worked. Check the account, JetStream availability/domain, API publish permission and reply-inbox subscription. A timeout alone does not identify the cause. |
| probe.stream_list passes but probe.system_monitoring is unknown | Stream access works. System visibility is separate and often intentionally unavailable to application users. Check both account routing and user grants as explained in [JetStream versus node access](NATS_LOGIN.md#why-jetstream-access-does-not-imply-node-access). Grant system access only when node monitoring is intended. |
| profile.default.operations is disabled | The dashboard's write switch is off. For reviewed edits, set NATSUI_ALLOW_WRITES=1 and separately grant only the required NATS operations. Doctor does not test writes. |
| storage.persistence is unknown | The command did not test volume durability or SQLite writes. Follow the setup guide's storage and readiness checks. |

The full report includes guidance below each relevant check. For a corrected running service, apply the same Compose file list used at startup:

```sh
docker compose --env-file .env -f compose.nats-login.yaml up -d --wait
docker compose --env-file .env -f compose.nats-login.yaml exec -T dashboard natsui doctor --require-session-only
```

Add -f compose.tls.yaml to both commands for that deployment. Recreating the dashboard ends current login sessions. Probe-only overlays are used for temporary checks and are not needed on the running service. An older image without the doctor command needs an image update through the normal deployment process.

## Automation and exit codes

For a machine-readable local report:

```sh
docker compose --env-file .env -f compose.nats-login.yaml exec -T dashboard natsui doctor --json --require-session-only
```

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
