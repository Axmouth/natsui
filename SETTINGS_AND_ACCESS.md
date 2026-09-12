# Settings ownership and access design

Status: design plus implemented local dashboard settings. No NATS write actions, NATS supervisor or shared user management are implemented in the local beta.

## Four settings paths

| Owner | Examples | Apply mechanism | Initial status |
| --- | --- | --- | --- |
| Dashboard | Threshold, observation interval, retained history | SQLite update, no restart | Implemented |
| JetStream resources | Editable stream retention and consumer delivery controls | Native NATS management API | Read-only inspection implemented; edits planned |
| Reloadable server configuration | Supported authentication, logging and policy changes | Validate file, write managed configuration, request reload | Optional deployment adapter |
| Restart-only deployment configuration | Bind addresses, storage location and environment changes | Reviewed process restart or container recreation | Optional deployment adapter |

The exact reloadable/editable field set depends on the target server version. The UI must not derive editability solely from a JSON field being present. Immutable resource settings need explicit replacement/migration workflows rather than silent delete/recreate.

NATS supports configuration validation and reload for supported changes. Reload should be preferred over restarting when it is applicable. See [NATS configuration management](https://docs.nats.io/learn/deployment/config-management) and [JetStream management API](https://raw.githubusercontent.com/nats-io/nats.docs/master/using-nats/jetstream/nats_api_reference.md).

## Optional managed deployment

A supervisor is a legitimate option for a packaged deployment that explicitly grants ownership of the server lifecycle. It must not become a requirement for inspecting an existing NATS installation.

Prefer a generated configuration file or managed include for mutable settings. A process cannot have its existing environment variables changed externally. A supervisor can spawn a new child with a new environment; changing a container's configured environment ordinarily requires recreating that container, not merely restarting the NATS child.

Controller contract:

1. Identify the managed target and current applied configuration revision.
2. Show a diff and classify each field as live resource update, reload, restart, or unsupported migration.
3. Validate against the actual server binary/version and deployment constraints.
4. Save a candidate and previous configuration with restricted permissions; atomically replace only owned files.
5. Apply with bounded timeouts and a recorded operation identity.
6. Verify observed state, connectivity and relevant health conditions. A successful signal is not proof of a successful change.
7. Offer recovery from the prior configuration where safe. Storage movement or data loss cannot be undone by restoring config text.

Single-node restarts interrupt service. Cluster changes require separate rolling procedures and quorum checks. Kubernetes, systemd and a bundled local supervisor should be separate adapters. Avoid granting the dashboard unrestricted Docker socket or shell access merely to edit settings. Credentials and signing keys are not ordinary JSON config fields to echo into diffs or audit records.

## Dashboard users

Shared deployments should have user management. Proposed roles:

- Viewer: metadata and diagnostics, with payload access separately grantable.
- Operator: permitted publish, resource edit and replay actions within assigned profiles.
- Administrator: dashboard users, profiles, retention and explicitly configured deployment control.

Authorization must be enforced by the backend on every operation, including payload reads. Mutations must record the authenticated actor. Local passwords need a modern password hash, revocable sessions and a first-run setup with no default password. OIDC can be an optional community integration.

The current local operator mode has no login. It binds only to 127.0.0.1 and rejects unexpected Host and cross-origin requests. It is not a multi-user security boundary against other local processes. Shared exposure must wait for authentication and profile-scoped authorization; simply changing the bind address is not a supported deployment path.

Dashboard users and NATS identities are distinct. Native NATS authorization still restricts what a configured connection can do. A dashboard administrator does not automatically acquire operator signing keys, a system-account identity or authority to change NATS users.



The local beta runtime and credential boundary is documented in [SECURITY.md](SECURITY.md). Container mode requires host-loopback publication and does not implement shared access.
