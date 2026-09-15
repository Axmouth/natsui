# Optional OpenID Connect sign-in

The default authenticated stack supports one-time links from `natsui login`. OIDC adds a Sign in with SSO button for a configured identity provider. Local access keys remain available for recovery. The keyless local tryout and static demo are unchanged.

## Deployment requirements

1. Configure dashboard authentication and an HTTPS reverse proxy as described in [shared access](SHARED_ACCESS.md).
2. Register an OIDC authorization-code client at the provider. Its exact redirect URI is `https://natsui.example.com/api/auth/oidc/callback`. Enable PKCE with S256 and the openid scope. Confidential clients use client_secret_basic authentication. A public client can omit the secret file if the provider allows it.
3. Sign in as the bootstrap administrator. Create named dashboard users in Settings with the intended roles. Record each user's Identity revision from that table.
4. Obtain each user's stable subject identifier from the configured provider. Map that subject explicitly in the JSON file below. Email addresses and display names are not identity keys.
5. Mount the configuration and optional client secret read-only. Recreate the dashboard to load changes, then verify SSO and a recovery login separately.

Example oidc.json:

```json
{
  "issuer": "https://identity.example.com/realms/operations",
  "client_id": "natsui",
  "client_secret_file": "/run/oidc/client-secret",
  "subjects": {
    "stable-provider-subject": {"user": "operator-alex", "identity_revision": 1}
  }
}
```

The issuer must exactly match discovery metadata. The subject and identity revision above are placeholders. An optional ca_file names a mounted private CA bundle for provider HTTPS. Standard public trust is used otherwise. Provider requests are bounded, verify certificates, and do not follow redirects.

```yaml
services:
  dashboard:
    environment:
      NATSUI_AUTH_TOKEN_FILE: /run/natsui-auth/access.key
      NATSUI_PUBLIC_URL: https://natsui.example.com
      NATSUI_OIDC_CONFIG_FILE: /run/oidc/oidc.json
      NATSUI_ACCESS_POLICY_FILE: /run/oidc/profile-access.json
    volumes:
      - natsui-data:/data
      - natsui-auth:/run/natsui-auth:ro
      - ./oidc:/run/oidc:ro
```

This is an override for the authenticated deployment, not a complete stack. Profile policy is optional and described in [connection profiles](PROFILES.md). Files must be readable by UID 10001. Neither a CA private key nor the provider's signing key belongs in the dashboard.

## Access and revocation

The library validates token signature, issuer, audience, expiry and nonce. The flow also binds a single-use state to the initiating browser and verifies PKCE. No dashboard user is created automatically. Unmapped subjects and disabled users are rejected. A deleted and recreated username has a new identity revision, so an old provider mapping cannot attach to its replacement.

SSO creates the same eight-hour dashboard session as an access-key login. Role changes, key rotation, disabling and deletion revoke active local sessions. The bootstrap administrator bypasses profile restrictions for recovery. Ordinary roles and profile memberships apply equally to SSO and key sessions. Admin is a deployment-wide role with authority to manage other dashboard users and configured controllers. Profile membership does not isolate one admin from another team.

Provider logout, provider account suspension and provider group changes do not revoke an existing dashboard session automatically. Immediate dashboard revocation requires disabling the mapped local user or restarting the dashboard. The adapter does not implement group synchronization, automatic provisioning, refresh tokens, back-channel logout or local MFA. Provider MFA can protect new sign-ins, while recovery access remains controlled by the mounted bootstrap key.

Ansible can provision these files through Vault-backed copy tasks with no_log: true and a container-recreation handler. No browser interaction is required to deploy or recover the stack. User creation and mapping are explicit administrative provisioning steps.

Reference: [OpenID Connect Core](https://openid.net/specs/openid-connect-core-1_0.html).
