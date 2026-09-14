# Optional managed NATS deployment

Standard Natsui attaches to unmodified NATS and does not own server files. The optional controller is a separate image that starts one NATS process and owns its JSON configuration. It has no Docker socket and cannot restart sibling containers or modify unrelated servers.

This adapter supports config-based authorization.users. JWT operators, resolvers and config-based accounts blocks remain outside this adapter. Existing identity tooling remains authoritative for those deployments.

## Build the controller

The initial controller is built from the repository. It is not included in the ordinary dashboard image or the one-command demo.

```sh
docker build -t natsui-controller:local controller
```

The image uses NATS 2.11.8 and Python with bcrypt. It runs as UID/GID 10001. The persistent controller directory contains nats.json and previous.json. Generated user passwords are persisted only as bcrypt hashes. The previous file supports manual configuration recovery, not recovery of removed messages or durable cursors.

## Prepare separate controller credentials

The controller requires its own 64-character hexadecimal token, an HTTPS server certificate and its private key. The dashboard requires the token and the public CA certificate that verifies that server. The server certificate must name the controller hostname used in its URL. A CA private key never belongs in either container.

The existing initializer can create a controller token in a protected directory:

```sh
natsui --init-auth control-token
```

Ansible can provision the token and certificate files with Vault-backed copy tasks and no_log: true. Files must be readable by UID/GID 10001. Controller certificates and tokens are distinct from dashboard login keys and NATS client credentials.

## Example controller service

The initial base.json is a valid NATS JSON configuration with server_name, a persistent JetStream store directory and at least one username/password identity. Existing operational configuration should be translated and validated before adoption. The controller only copies this base on its first start. Subsequent starts retain the managed nats.json.

```json
{
  "server_name": "managed-nats1",
  "listen": "0.0.0.0:4222",
  "http": "0.0.0.0:8222",
  "jetstream": {"store_dir": "/data/jetstream"},
  "authorization": {
    "users": [{"user": "observer", "password": "REPLACE_WITH_BCRYPT_HASH"}]
  }
}
```

REPLACE_WITH_BCRYPT_HASH is a placeholder. A password hash can be generated with the controller image. This interactive command reads the password without echoing it and prints only its bcrypt hash:

~~~sh
docker run --rm -it --entrypoint python3 natsui-controller:local -c "import bcrypt,getpass; print(bcrypt.hashpw(getpass.getpass('NATS password: ').encode(),bcrypt.gensalt(rounds=12)).decode())"
~~~

The NATS client needs the original password. The JSON configuration receives the printed hash. Store both configuration and credentials with permissions appropriate to their deployment roles. Files mounted under /run/control must be readable by UID/GID 10001. The controller data directory must be writable by that identity.

 The initial user needs permissions appropriate to the existing deployment. A broad bootstrap user is administrative authority and should be replaced with deliberately scoped client users.

```yaml
services:
  managed-nats1:
    image: natsui-controller:local
    restart: unless-stopped
    environment:
      NATSUI_CONTROL_BASE_FILE: /run/control/base.json
      NATSUI_CONTROL_TOKEN_FILE: /run/control/token
      NATSUI_CONTROL_CERT_FILE: /run/control/server.pem
      NATSUI_CONTROL_KEY_FILE: /run/control/server.key
    volumes:
      - managed-data:/data
      - ./control:/run/control:ro
    read_only: true
    cap_drop: [ALL]
    security_opt: ['no-new-privileges:true']
    networks: [brokers]
volumes:
  managed-data:
networks:
  brokers:
    external: true
    name: EXISTING_NATS_NETWORK
```

The control port is 9443. No host port is published in this example. The dashboard reaches it through the existing private network. The NATS client and monitoring ports remain separate. An unexpected NATS exit terminates the controller container so its restart policy can recover the process. An intentional reviewed restart remains within the controller.

## Connect Natsui to the controller

```json
[
  {
    "id": "nats1",
    "url": "https://managed-nats1:9443",
    "ca_file": "/run/controllers/ca.pem",
    "token_file": "/run/controllers/token"
  }
]
```

Mount the endpoint JSON, public CA and controller token read-only, then set NATSUI_MANAGED_CONFIG_FILE to the JSON path and NATSUI_ALLOW_MANAGED=1. Dashboard authentication is mandatory. Only dashboard admins can access the controller UI. Up to 32 explicitly configured controllers are supported.

## Review and apply

Settings contains a managed-node selector, reported process state and operation templates. The first supported server settings are max_connections, max_payload and max_subscriptions. Changing max_subscriptions requires a restart. User creation, password rotation, permission changes and deletion use a live reload. The last NATS user cannot be deleted.

Every proposal is validated with nats-server -t before review. Application revalidates the file revision, writes the candidate atomically, and requests reload or restart. The result distinguishes a saved configuration from a reload/restart observed through local NATS monitoring. Missing verification is not successful application evidence. No automatic retry is sent.

Generated NATS passwords are shown once after application. An optional 64-character hexadecimal password can provision the same credential on another node. User changes are node-local. A cluster with config-based authentication must receive matching usernames, passwords and permissions on every node that accepts those clients. Apply node changes deliberately and verify connectivity before moving to the next node. This controller does not perform a coordinated cluster rollout or assert quorum safety.

The normal NATSUI_ALLOW_WRITES setting remains separate from controller authority. A read-only NATS observer connection does not grant access to server files. The explicitly supplied controller credential grants that separate authority.
