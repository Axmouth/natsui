# Settings ownership and access design

Status: local dashboard settings and opt-in native JetStream configuration editing are implemented. NATS server supervision and shared user management remain planned.

## Four settings paths

| Owner | Examples | Apply mechanism | Initial status |
| --- | --- | --- | --- |
| Dashboard | Threshold, observation interval, retained history | SQLite update, no restart | Implemented |
| JetStream resources | Editable stream retention and consumer delivery controls | Native NATS management API | Reviewed edits implemented for the allowlisted fields below |
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

The current local operator mode supports optional access-key login through NATSUI_AUTH_TOKEN_FILE, with expiring sessions and sign-out. Individual accounts and roles are not implemented. It binds only to 127.0.0.1 and rejects unexpected Host and cross-origin requests. It is not a multi-user security boundary against other local processes. Shared exposure must wait for individual identities and profile-scoped authorization; simply changing the bind address is not a supported deployment path.

Dashboard users and NATS identities are distinct. Native NATS authorization still restricts what a configured connection can do. A dashboard administrator does not automatically acquire operator signing keys, a system-account identity or authority to change NATS users.



The local beta runtime and credential boundary is documented in [SECURITY.md](SECURITY.md). Container mode requires host-loopback publication; access-key authentication does not implement multi-user authorization. See [deployment and storage setup](docs/SETUP.md).

## Implemented JetStream editor

Settings contains an editor for existing resources on NATS 2.11+ in the 2.x series. NATS remains the final validator for cluster constraints and configuration combinations. The deployment must run a compatible server version throughout the cluster; the dashboard checks the connected server version, not every stream leader.

| Resource | Editable fields |
| --- | --- |
| Stream | max_msgs, max_bytes, max_age, max_msg_size, subjects, discard, duplicate_window, num_replicas |
| Consumer | ack_wait, max_ack_pending, max_deliver, backoff |

The editor is enabled at startup with `NATSUI_ALLOW_WRITES=1` for the configured local profile. The default is `0`. Simulation modes cannot write. Values are submitted as decimal strings and parsed on the backend, preserving 64-bit limits and nanosecond precision; the form displays durations in seconds. Unedited configuration fields stay on the backend and are preserved verbatim as JSON values.

A native INFO read supplies the creation identity and configuration revision. Preview validates the allowlisted patch, includes the effective acknowledgment timeout when backoff changes, and stores one candidate for up to 120 seconds. A new preview supersedes the previous preview for this dashboard instance. Apply requires a one-use token, a same-origin request header and acknowledgment of listed effects. The backend refetches the full configuration and creation identity before sending an update. This rejects stale forms but is not an atomic compare-and-swap against external NATS clients. Deployments with Git or application-owned configuration can overwrite UI changes.

An attempted change with a before/after diff must be persisted to SQLite before any broker write. The resulting outcome is recorded separately with the same operation ID. History uses existing incident retention and size limits; it is not a tamper-proof compliance log. The actor is labeled local operator, not an authenticated personal identity. Failed readback or a request timeout leaves an explicit unconfirmed outcome, with no automatic retry. Failure to save the outcome does not imply that the broker change failed. Retention reductions can remove data irreversibly, so restoring an old configuration is not presented as rollback of stored messages.

Streams use STREAM.UPDATE. Consumers use CONSUMER.CREATE with action=update so a deleted consumer is not silently recreated. No worker fetch, acknowledgment, resource deletion or broker process control is part of the editor. Permission on the consumer CREATE subject can authorize creation through other clients; NATS subject permissions do not restrict the JSON action field.

Sources: [stream editable fields](https://github.com/nats-io/nats.docs/blob/master/nats-concepts/jetstream/streams.md), [consumer editable fields](https://github.com/nats-io/nats.docs/blob/master/nats-concepts/jetstream/consumers.md).
