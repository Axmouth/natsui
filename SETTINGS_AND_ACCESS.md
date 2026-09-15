# Settings ownership and access design

Status: dashboard settings, named dashboard identities, opt-in native JetStream editing and resource operations are implemented. An optional deployment controller supports a limited set of server settings and config-based NATS users. The standard dashboard works with unmodified NATS.

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

Prefer a generated configuration file or managed include for mutable settings. A process cannot have its existing environment variables changed externally. A supervisor can spawn a new child with a new environment. changing a container's configured environment ordinarily requires recreating that container, not merely restarting the NATS child.

Controller contract:

1. Identify the managed target and current applied configuration revision.
2. Show a diff and classify each field as live resource update, reload, restart, or unsupported migration.
3. Validate against the actual server binary/version and deployment constraints.
4. Save a candidate and previous configuration with restricted permissions. atomically replace only owned files.
5. Apply with bounded timeouts and a recorded operation identity.
6. Verify observed state, connectivity and relevant health conditions. A successful signal is not proof of a successful change.
7. Offer recovery from the prior configuration where safe. Storage movement or data loss cannot be undone by restoring config text.

Single-node restarts interrupt service. Cluster changes require separate rolling procedures and quorum checks. Kubernetes, systemd and a bundled local supervisor should be separate adapters. Avoid granting the dashboard unrestricted Docker socket or shell access merely to edit settings. Credentials and signing keys are not ordinary JSON config fields to echo into diffs or audit records.

## Dashboard users

Settings supports generated access-key identities with viewer, operator and admin roles. Viewer can inspect observations and retained payloads. Operator can also perform enabled native NATS mutations. Admin can manage dashboard identities, settings and explicitly configured controllers. Optional profile membership restricts named users. Explicit OIDC mappings enable provider sign-in with the same local roles. Separate payload grants and local password/MFA accounts remain outside this implementation. Global administrators retain user and controller management authority.

Authorization is enforced on backend routes. SQLite user changes run outside async workers. Identity checks use a bounded cache refreshed by successful changes. Persistent monotonically increasing revisions reject stale user-management forms across account recreation. Sessions and one-time tickets also bind to the credential digest.

Dashboard users and NATS identities are separate domains. A dashboard administrator does not acquire NATS signing keys or controller authority unless the deployment supplies those credentials. See [shared access](docs/SHARED_ACCESS.md), [profiles](docs/PROFILES.md), [controller setup](docs/MANAGED.md) and [security boundaries](SECURITY.md).

## Implemented JetStream editor

Settings contains an editor for existing resources on NATS 2.11+ in the 2.x series. NATS remains the final validator for cluster constraints and configuration combinations. The deployment must run a compatible server version throughout the cluster. the dashboard checks the connected server version, not every stream leader.

| Resource | Editable fields |
| --- | --- |
| Stream | max_msgs, max_bytes, max_age, max_msg_size, subjects, discard, duplicate_window, num_replicas |
| Consumer | ack_wait, max_ack_pending, max_deliver, backoff |

The editor is enabled at startup with `NATSUI_ALLOW_WRITES=1` for the configured local profile. The default is `0`. Simulation modes cannot write. Values are submitted as decimal strings and parsed on the backend, preserving 64-bit limits and nanosecond precision. the form displays durations in seconds. Unedited configuration fields stay on the backend and are preserved verbatim as JSON values.

A native INFO read supplies the creation identity and configuration revision. Preview validates the allowlisted patch, includes the effective acknowledgment timeout when backoff changes, and stores a session-owned candidate for up to 120 seconds. A bounded map retains up to 32 independent reviews per profile. Expired reviews are removed when the map is accessed. Applying a mutation remains serialized separately. Apply requires a one-use token, a same-origin request header and acknowledgment of listed effects. The backend refetches the full configuration and creation identity before sending an update. This rejects stale forms but is not an atomic compare-and-swap against external NATS clients. Deployments with Git or application-owned configuration can overwrite UI changes.

An attempted change with a before/after diff must be persisted to SQLite before any broker write. The resulting outcome is recorded separately with the same operation ID. History uses existing incident retention and size limits. it is not a tamper-proof compliance log. Records contain the named actor, or trusted-local when authentication is disabled. Failed readback or a request timeout leaves an explicit unconfirmed outcome, with no automatic retry. Failure to save the outcome does not imply that the broker change failed. Retention reductions can remove data irreversibly, so restoring an old configuration is not presented as rollback of stored messages.

Streams use STREAM.UPDATE. Consumers use CONSUMER.CREATE with action=update so a deleted consumer is not silently recreated. No worker fetch or acknowledgment is part of inspection or editing. Resource deletion belongs to the separate Operations page. Process control requires the optional controller. Permission on the consumer CREATE subject can authorize creation through other clients. NATS subject permissions do not restrict the JSON action field.

Sources: [stream editable fields](https://github.com/nats-io/nats.docs/blob/master/nats-concepts/jetstream/streams.md), [consumer editable fields](https://github.com/nats-io/nats.docs/blob/master/nats-concepts/jetstream/consumers.md).

## Optional controller implementation

The initial adapter owns a JSON file and one NATS child. It supports max_connections and max_payload by reload, max_subscriptions by restart, and config-based user creation, rotation, permission changes and deletion. It validates candidates with the actual NATS binary, records the previous config, and reports observed reload/restart separately from file persistence. It does not coordinate cluster rollouts, move storage or manage arbitrary environment settings. [Controller deployment](docs/MANAGED.md) documents the opt-in authority and recovery boundaries.
