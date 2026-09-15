# Deploy Natsui with Ansible

This example deploys a protected dashboard beside an existing NATS cluster on a Linux Docker host. Deployment is unattended once SSH, privilege escalation and Vault access are configured. Dashboard login still uses the existing access-key file. The standalone authenticated Compose stack can initialize its key automatically. This playbook supplies a managed key explicitly, preserving unattended and repeatable deployment.

## Prerequisites

- A Linux control node, or WSL on Windows, with Python 3.11+ and ansible-core 2.19.x. Native Windows is not an Ansible control node for this recipe.
- A Linux managed host with Python 3, Docker Engine and the Docker Compose plugin. Compose 2.34+ covers the commands in the setup guides.
- SSH access with permission to become root. The example assumes standard rootful Docker without user-namespace remapping.
- An existing private Docker network shared with the NATS servers. The playbook does not install Docker, create the cluster or provision NATS identities.
- One writable data directory per dashboard process. The default /opt/natsui/data is a host bind mount of the complete SQLite directory.

The dashboard is published only on the managed host's loopback address. Public HTTPS uses the separate explicit proxy configuration described in the shared-access guide. Remote access uses an SSH tunnel as shown below. NATS transport TLS is independent of dashboard HTTP.

## 1. Prepare the playbook folder

Copy the [deploy/ansible directory](../deploy/ansible) from the repository or release package to a private working directory. The required files are:

```text
ansible/
  deploy.yml
  requirements.yml
  inventory.example.ini
  settings.example.yml
  templates/
    compose.yaml.j2
```

From that directory, install the pinned collection and create local settings:

```sh
ansible-galaxy collection install -r requirements.yml
cp inventory.example.ini inventory.ini
cp settings.example.yml settings.yml
```

Change inventory.ini to the managed host's address and SSH user. The sample 192.0.2.10 is a documentation address. Replace natsui_network in settings.yml with the network name from docker inspect on that host. Replace natsui_urls and natsui_monitor_urls with addresses reachable inside that network.

The sample image uses latest for initial evaluation. For repeatable deployments, set natsui_image to a published digest such as ghcr.io/axmouth/natsui@sha256:IMAGE_DIGEST, replacing IMAGE_DIGEST with the actual digest. The playbook pulls missing images. Reusing a mutable latest tag does not automatically fetch a newer image on every run.

## 2. Provision the dashboard key with Vault

Natsui accepts a 64-character hexadecimal access key representing 32 random bytes. The following Bash commands create one privately and encrypt the file with Ansible Vault. Run this initialization once:

```sh
umask 077
mkdir -p vault
python3 -c "import secrets; from pathlib import Path; p = Path('vault/dashboard.key'); f = p.open('x'); f.write(secrets.token_hex(32) + chr(10)); f.close()"
ansible-vault encrypt vault/dashboard.key
```

Exclusive file creation refuses to replace an existing key. If Vault encryption fails, the file remains plaintext with the restricted permissions established by umask. Complete encryption before storing it in source control. The encrypted file and its Vault password must be stored separately.

natsui_auth_source already points to vault/dashboard.key. The playbook decrypts it on the controller, validates its format, and copies it to /opt/natsui/secrets/dashboard.key as root:10001 with mode 0640. The container mounts that individual file read-only at /run/natsui-auth/access.key. The secret directory is mode 0750. The data directory belongs to UID/GID 10001 so SQLite can create its WAL and SHM files.

An existing secret manager can supply an equivalently protected source file instead. No generated key is returned as an Ansible fact or printed by the playbook. Secret tasks disable logging and diff output. Vault protects files at rest, while no_log and diff: false protect ordinary Ansible output. Debugging tasks or insecure callback plugins can still expose decrypted values.

## 3. Add NATS credentials and TLS files when required

The dashboard key does not authenticate to NATS. The cluster administrator supplies a restricted observer identity and any required certificates. Leave unused source settings empty. Source paths refer to files on the Ansible controller, relative to the playbook directory:

```yaml
natsui_urls:
  - tls://nats1:4222
  - tls://nats2:4222
  - tls://nats3:4222
natsui_creds_source: vault/observer.creds
natsui_ca_source: certificates/ca.pem
natsui_client_cert_source: certificates/client.pem
natsui_client_key_source: vault/client.key
```

Encrypt private credentials and keys with ansible-vault encrypt before deployment. Public CA and client certificates may remain ordinary PEM files. The client private key must decrypt to an unencrypted PEM key because startup cannot prompt for a passphrase. Do not supply a CA private key or a server private key.

The template adds NATSUI_CREDS and NATSUI_TLS_* only for configured source files. It mounts each file read-only and never mounts the Docker socket in Natsui. A custom CA or client certificate requires TLS. Client certificate and key sources must be configured together. For password/token authentication, place the percent-encoded credentials in a seed URL and protect settings.yml with Vault as well. Do not combine inline URL credentials with natsui_creds_source.

NATSUI_URL chooses the first successful NATS connection from the seed list. NATSUI_MONITOR_URLS collects each node's independent HTTP endpoint. NATS discovery does not provide monitoring ports. Monitoring private CAs, client certificates and HTTP credentials use NATSUI_MONITOR_CONFIG_FILE, with independent settings per endpoint. The [connection recipes](SETUP.md#broker-credentials-and-tls-recipes) cover those boundaries and certificate-name checks.

## 4. Check and deploy

An interactive operator can provide the Vault password when prompted:

```sh
ansible-playbook -i inventory.ini deploy.yml --syntax-check --ask-vault-pass
ansible-playbook -i inventory.ini deploy.yml --check --ask-vault-pass
ansible-playbook -i inventory.ini deploy.yml --ask-vault-pass
```

Check mode predicts directory and file changes. Container changes and HTTP readiness checks are deliberately skipped, since a first deployment has no rendered files or running service yet. Check mode does not prove Docker, broker credentials or network reachability. The live run verifies that /api/snapshot rejects unauthenticated requests with HTTP 401 and that /readyz reports current collection and storage readiness. Monitoring coverage must still be inspected separately.

For unattended automation, provide SSH and become credentials through the automation platform and use its Vault credential integration. A protected Vault password file is also supported:

```sh
ansible-playbook -i inventory.ini deploy.yml --vault-password-file /secure/ansible/natsui-vault-password
```

Keep that password file outside the repository and restrict it to the automation identity. A configured secret-store password-client script is another supported Ansible mechanism. Deployment does not require a browser, a manual copy from container logs, or a running human session.

If the broker is intentionally unavailable during deployment, set natsui_require_ready: false. The dashboard login check still runs. This changes the deployment gate, not the application's reported broker health.

## 5. Open the remote dashboard

Create an SSH tunnel from the operator's machine to the Docker host:

```sh
ssh -N -L 4321:127.0.0.1:4321 deploy@DASHBOARD_HOST
```

Replace the SSH user and host with inventory values, then open http://127.0.0.1:4321 locally. If local port 4321 is occupied, use -L 4322:127.0.0.1:4321 and open port 4322. The remote command sudo docker compose -f /opt/natsui/compose.yaml exec dashboard natsui login prints a one-time link after proving access to the configured key. The link expires after 60 seconds. Its host port must match the local SSH tunnel. The permanent-key form remains available through the authorized secret-management workflow. For a local Vault-backed deployment, ansible-vault view vault/dashboard.key displays the secret for sign-in. Its output belongs in a password manager and must not be captured in job logs.

## Repeat runs, rotation and upgrades

Copy and template tasks compare content. An unchanged deployment reuses the existing key, data directory and container. Changes to a secret or Compose definition notify one handler that recreates the dashboard before readiness checks. Recreation is necessary because Natsui loads credentials at startup, and an atomic file replacement can leave an individual bind mount attached to the old inode. A plain process restart is insufficient to guarantee that a replaced file is remounted.

Dashboard key rotation is explicit: generate a new random key into a different source file, encrypt it, change natsui_auth_source, and rerun the playbook. The handler recreates the container and invalidates browser sessions. Keep the same natsui_directory and natsui_profile for dashboard-key-only rotation. Removing or corrupting the configured key fails the deployment instead of disabling login.

Changing the NATS seed set, domain, credential-file contents or client certificate also requires a new natsui_profile under the current history identity guard. Reordering the same seeds does not. Retain the data directory and its older history. Changing the image digest or another rendered setting triggers recreation without replacing storage.

The playbook manages only its own Compose project. It does not remove broker containers or delete volumes. Obsolete secret files remain in the protected host directory but are no longer mounted if their source setting is empty. Removing obsolete host secrets is a separate, explicit administrative step.

## Backups

The full /opt/natsui/data directory contains SQLite and any WAL/SHM companions. For a stopped-process backup on the managed host, use the deployed Compose definition:

```sh
sudo docker compose -f /opt/natsui/compose.yaml stop dashboard
sudo tar -czf /secure/backups/natsui-data.tar.gz -C /opt/natsui data
sudo docker compose -f /opt/natsui/compose.yaml start dashboard
```

The backup directory must already exist and be protected. Check archive success and restart the dashboard even if a backup attempt fails. Use distinct archive filenames to retain prior backups. A persistent host bind mount is not a backup. Restore only while the dashboard is stopped, preserving ownership. Store encrypted credential sources and Vault recovery material separately from history backups. The [storage guide](SETUP.md#backup-and-restore) explains the complete-directory requirement.

## Reference

- [Ansible Docker Compose module](https://docs.ansible.com/projects/ansible/latest/collections/community/docker/docker_compose_v2_module.html)
- [Ansible Vault](https://docs.ansible.com/projects/ansible-core/devel/vault_guide/vault.html)
- [Ansible logging and no_log](https://docs.ansible.com/projects/ansible-core/devel/reference_appendices/logging.html)

## Extended deployment files

The provided playbook covers the common observer deployment, dashboard authentication and NATS transport TLS. Profile JSON, HTTPS monitoring endpoint files and controller credentials use the same Vault-backed copy and read-only mount pattern. Their environment variables and volume targets are shown in the linked [profile](PROFILES.md), [monitoring](MONITORING_SECURITY.md) and [controller](MANAGED.md) guides. These optional settings require extending the template and secret file list. The playbook does not infer them from NATS discovery.

For NATSUI_PUBLIC_URL deployments, the unauthenticated API check must send the configured public Host header and still expect 401. The loopback /readyz check remains unchanged. Secret and certificate replacement must continue to notify container recreation.


## Optional SSO and profile policy files

The [OIDC guide](OIDC.md) and [profile policy guide](PROFILES.md) use deployment-owned JSON files. Provision the JSON and client secret with Vault-backed copy tasks, mode '0400', owner '10001', group '10001' and no_log: true. Add read-only mounts and NATSUI_OIDC_CONFIG_FILE or NATSUI_ACCESS_POLICY_FILE to the Compose template. Notify the existing recreation handler when either changes. The bootstrap key remains available for unattended recovery.

The [JWT authority](JWT_AUTHORITY.md) requires its own service, exclusive persistent NSC store, signing keys and independent TLS/token files. The observer playbook does not provision or infer this signing authority. Its store backup and key distribution require explicit deployment tasks.
