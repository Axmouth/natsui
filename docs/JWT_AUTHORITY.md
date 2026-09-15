# Optional NATS JWT account authority

The JWT adapter manages users in one existing NSC account store. It is separate from dashboard login and from the [config-based process controller](MANAGED.md). It never starts or restarts NATS. Standard dashboards need no signing authority.

## Authority and persistent storage

The deployment administrator supplies an existing, unmanaged NSC store in a dedicated writable volume. The adapter uses NSC 2.14.0 with a unified `-H /data/store` layout. That directory contains nsc.json, the operator directory with its accounts, and keys. Importing an existing split-directory store requires an explicit offline migration and verification with NSC first. The adapter does not import, create or select operators through the UI.

Account keys can issue user credentials. Changing account JWTs, including user revocations, also requires the appropriate operator signing authority. A supplied operator key can have authority beyond the configured account. The adapter's fixed account limit is an application boundary, not a cryptographic restriction on that mounted key. A narrowly provisioned signing store is preferable to mounting an entire organizational operator store.

Only this adapter may write the store while it runs. An advisory lock rejects a second adapter, but external NSC processes do not honor that lock. Stop the adapter before offline NSC administration, key rotation, backups or retirement of old user entries. Managed NSC stores are rejected because they can publish account changes implicitly. JWT symlinks and oversized stores are rejected.

## Build and configure

```sh
docker build -f controller/Dockerfile.jwt -t natsui-jwt-controller:local controller
```

Example authority.json for local signing and export:

```json
{
  "store": "/data/store",
  "operator": "ExampleOperator",
  "account": "APP",
  "account_public_key": "REPLACE_WITH_EXISTING_ACCOUNT_PUBLIC_KEY"
}
```

Names and the public key must match the existing store. The account public key begins with A and contains 56 characters. `nsc -H /data/store describe account -n APP --json` reports it as sub. The adapter checks that identity before operations. No seed belongs in this JSON file.

Optional resolver publication adds:

```json
{
  "resolver_url": "nats://nats1:4222",
  "system_user": "resolver-publisher",
  "ca_file": "/run/control/nats-ca.pem",
  "client_cert": "/run/control/client.pem",
  "client_key": "/run/control/client.key"
}
```

Merge those fields into authority.json. The system user must already exist in the operator's system account. NSC's push command accepts nats:// with its explicit CA option, which requires and verifies TLS. This adapter requires ca_file for publication, even for a public CA. Client certificate and key are optional together for mutual TLS. HTTP account servers and WebSocket resolvers are outside this adapter.

```yaml
services:
  jwt-authority:
    image: natsui-jwt-controller:local
    restart: unless-stopped
    environment:
      NATSUI_JWT_CONFIG_FILE: /run/control/authority.json
      NATSUI_CONTROL_TOKEN_FILE: /run/control/token
      NATSUI_CONTROL_CERT_FILE: /run/control/server.pem
      NATSUI_CONTROL_KEY_FILE: /run/control/server.key
    volumes:
      - ./nsc-store:/data/store
      - ./jwt-control:/run/control:ro
    tmpfs: [/tmp]
    read_only: true
    cap_drop: [ALL]
    security_opt: ['no-new-privileges:true']
    networks: [brokers]
networks:
  brokers:
    external: true
    name: EXISTING_NATS_NETWORK
```

Prepare directories and permissions before startup. The store must be writable by UID/GID 10001, and control files readable by that identity. The HTTPS certificate must include jwt-authority in its subject alternative names. Generate a separate controller token with `natsui --init-auth token`. No host port or Docker socket is required. Signing material must be included in encrypted, stopped-process backups.

Add an endpoint to the dashboard's NATSUI_MANAGED_CONFIG_FILE:

```json
{
  "id": "app-jwt",
  "url": "https://jwt-authority:9443",
  "ca_file": "/run/controllers/authority-ca.pem",
  "token_file": "/run/controllers/authority-token"
}
```

The endpoint file is an array of these objects. NATSUI_ALLOW_MANAGED=1 and dashboard authentication are required. Mount its token and CA read-only as in the process-controller guide. Only dashboard admins can use Settings to access this authority. Controller access is deployment-wide, independent of the selected observation profile.

## Reviewed operations

- Create user requires explicit publish and subscribe subject lists and an expiry from 1 to 8760 hours. An empty list denies that direction. Names are limited to 48 letters, digits, underscores or hyphens. Commas in permission subjects are unsupported by the NSC argument format. A signed user credential may be usable immediately wherever the account is already trusted.
- Revoke user changes the local account JWT. It does not revoke broker access until that JWT is distributed.
- Export account returns the public account JWT for an existing distribution process.
- Publish account uses the fixed resolver endpoint and system identity. Successful NSC acknowledgement reports resolver acceptance, not convergence of every broker or termination of every existing connection.

Reviews are tied to the session, expire after two minutes and recheck the public JWT revision. There is no automatic retry after a failed or uncertain operation. Busy operations fail immediately without queueing a later mutation. Each command has a five-second deadline, and resolver replies are collected for two seconds. Slow or unreachable deployments can return uncertainty even if some work completed. Status must be reloaded before further action.

User credentials are displayed once after creation and cleared when the result dialog closes. NSC retains their private seeds in the protected store. If a response is lost, stop the adapter and recover the existing credential through `nsc -H /data/store generate creds -a APP -n USER`, or revoke it through the normal distribution process. Do not create a second identity merely because the first response was lost. Shell output containing credentials must remain private.

This version has no user rename, credential regeneration UI, operator/account creation, key rotation or resolver topology management. Revoked and expired users still count toward the 256-user store limit. Offline retirement and replacement names remain explicit administrator operations. User creation without operator authority is supported, while revocation fails if the required account signer is absent.

Reference: [NATS JWT administration](https://docs.nats.io/running-a-nats-service/nats_admin/jwt), [NSC push](https://nats-io.github.io/nsc/nsc_push.html).
