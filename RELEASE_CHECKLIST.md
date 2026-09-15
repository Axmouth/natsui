# Local beta release evidence

This document separates implemented checks from validation that still needs elapsed time, external machines or publication access.

## Implemented

- Native binary packaging with embedded assets, documentation and SHA-256 checksums.
- Linux Docker image, non-root runtime, read-only root filesystem and loopback-only Compose publication.
- One-command three-node demo, with real NATS traffic and persistent dashboard data.
- Dedicated subject-permission template, explicit URL authentication, private CA and mutual TLS configuration.
- Connection/profile mismatch protection and explicit legacy-history adoption.
- Per-operation storage-health reporting, retained validated settings and bounded logical history storage.
- Liveness and readiness endpoints, local concurrency check and configurable endurance recorder.
- CI definition for Windows, Linux and macOS artifacts. full Linux container verification target.

## Evidence recorded locally

Verified on 2026-09-12:

- All 22 Rust tests passed on Windows and in the Linux Docker verification target, including live non-consuming inspection, mutual TLS, rejected untrusted/missing certificates, wrong credentials, denied inventory permissions, denied publish/delete actions and recovery after a broker restart.
- A disposable inventory with 301 streams and 2,001 consumers produced the expected partial, capped observation. The Windows bounded scan took about 0.9 seconds in the recorded run.
- SQLite tests cover legacy migration, profile conflicts, simulated read-only writes, failure visibility despite successful reads, recovery and eviction of oldest history under a byte budget.
- Clippy with warnings denied, Rust formatting, JavaScript syntax, trend/subject semantics and static simulation checks passed locally.
- The Linux runtime reported complete inventory for three nodes, three streams and five consumers. Docker inspection confirmed UID 10001, a read-only root filesystem and only loopback port publication.
- Eight simultaneous local readers completed 120 requests without errors. The recorded Linux-container run took 118 ms total, with 20 ms p95 and 44 ms maximum request latency. This is a small local check, not a production capacity benchmark.
- One Linux dashboard resource sample showed 14.63 MiB resident container memory and 0.02% CPU. Endurance evidence is required before drawing conclusions about sustained overhead.
- The packaged Windows release opened the existing real-cluster workspace after explicit legacy adoption. A stopped-process SQLite backup passed quick_check and retained 2,456 samples. Browser inspection confirmed the preserved node view and healthy 3/3 monitoring coverage.
- The short Linux endurance-recorder smoke test completed without readiness failures. A separate 24-hour Windows run started at 2026-09-12 00:43:45 UTC and writes data/soak/beta-24h.jsonl plus its summary. The recording was subsequently interrupted. Its 279 observations include one unavailable collection and long observation gaps. it does not establish a continuous 24-hour pass.

NATS Server 2.11.8 is a compatibility test baseline, not a claim that it is the latest or recommended server patch. The password-based restricted identity and mutual TLS cases were exercised. operator-issued JWT/account policy combinations still need deployment-specific validation.

## Before publication

- Partial endurance evidence was accepted for the current release scope on 2026-09-15. A continuous 24-hour pass remains unverified.
- Verify the exact main commit on the Windows, Linux and macOS CI runners before treating its native packages as available.
- Source repository: https://github.com/Axmouth/natsui. The repository publication workflow produces images and the Pages demo after verification. The current public deployment is identified by the successful main-branch publication run.
- Have fresh users follow the quick start and investigate actual operational questions. Record installation failures and confusing evidence semantics.
- Review broker compatibility beyond the tested baseline, including deployment-specific account/JWT policies.

## Deferred beyond the local beta

Separate payload grants, automatic identity provisioning and coordinated broker rollout remain extensions. Profile membership, explicit OIDC mapping and bounded JWT account administration are implemented. Named access-key identities, explicit HTTPS proxy deployment, reviewed native resource operations and an opt-in controller are implemented. The controller does not perform coordinated cluster rollout or claim quorum safety.

## Publishing

`.github/workflows/publish.yml` runs after successful main-branch verification. Each native Linux architecture builds the dashboard and traffic images and exercises a fresh three-node cluster before pushing. The bundle combines architecture manifests, resolves image digests, and verifies the registry-delivered Compose application. Pages deploys the static landing page, demo and fallback Compose file. First publication requires GitHub Pages Source=GitHub Actions and public visibility for the three GHCR packages. See [publishing operations](docs/PUBLISHING.md).


## Authentication and persistent deployment increment

- Generated access-key files fail closed when configured incorrectly. generation refuses overwrite.
- Session expiry/revocation, login body and rate limits, and protection of read/write APIs are covered by Rust tests.
- The Pages setup guide and authenticated Compose download are included in static-site link checks.
- Docker auth/storage smoke verification passed: key initialization as UID 10001, separate secret mount, protected API reads, logout, replacement session invalidation, retained settings/history and fresh-volume restore.
- Windows and Linux: all 30 Rust tests passed, including disposable real-broker TLS/permission/editing checks. Windows Clippy and Linux release packaging passed.
- Browser: invalid key, successful sign-in and corrected sign-out redirect were verified. Public publication follows the verification workflow. these local checks do not claim that the new commit is already deployed.

## Shared deployment verification, 2026-09-14

- Ansible initial deployment, unchanged second run, key rotation, container recreation and preserved history were verified against disposable Docker services.
- Windows unit suite: 39 passed and six real-broker cases skipped locally. The full Linux suite passed all 45 tests with disposable NATS brokers. Windows and Linux release archives were generated successfully.
- Controller Docker tests passed against its real NATS child, including HTTPS token access, stalled-handshake isolation, reload/restart, credential rotation, owned reviews and unexpected process exit.
- Native monitoring integration passed for private CA, mTLS, Basic/Bearer isolation, missing credentials, untrusted certificates and redirect rejection.
- The UI route contract checks literal calls against registered backend routes. It complements browser checks and does not verify response shapes or browser behavior.
- The durable browser regression passed against the final runtime and a fresh authenticated cluster. It covers one-time login, per-tab profiles, isolated settings, create/publish/delete operations, named viewer restrictions and recovery when only the default profile remains. CI includes this workflow. A completed short test does not qualify as a 24-hour run.

## Endurance interruption, 2026-09-15

The shared-deployment recorder captured 757 healthy samples from 2026-09-14 17:04:39 UTC through 23:40:55 UTC, spanning 6 hours 36 minutes 17 seconds. Readiness and monitoring were complete throughout these samples. Maximum recorded request latency was 47 ms and maximum dashboard container memory was 111.9 MiB. These observations do not establish a memory plateau or a 24-hour pass.

Windows Power-Troubleshooter event 1 confirms host sleep from 2026-09-14 23:41:20 UTC until 2026-09-15 06:20:00 UTC. One in-flight sample recorded an unavailable response and 23,911,837 ms elapsed latency across the suspension. That value includes host suspension and is not an active request-service latency measurement. Sampling resumed with healthy readiness and complete monitoring after wake.

Evidence remains in data/soak/2026-09-14-shared-deployment.jsonl and its summary. The recorder's elapsed time and eventual completion flag use wall-clock time, so they cannot certify uninterrupted operation across this sleep interval. The partial evidence was accepted for the current scope on 2026-09-15. A future uninterrupted 24-hour recording requires a host that stays awake. The follow-up remains paused.


## Investigation and identity follow-up, 2026-09-15

- Rust 1.94 strict Clippy passed on Windows. All 43 locally applicable tests passed, with six real-broker cases covered by the full 49-test Linux container suite.
- Backlog tests cover retained peaks, omitted failures, historical cadence changes, legacy rows, full-range selection, profile separation and index-only projection without parsing resource inventories.
- OIDC tests use a disposable HTTPS provider and signed RSA tokens. Wrong issuer/audience/nonce/expiry/authorized-party values, bad signatures, replay, wrong browser cookie, unmapped subjects, disabled users and replacement identities are rejected. A real browser traverses two HTTPS sites and reaches the dashboard with a Secure, Strict session.
- Workspace tests exercise saved-view persistence, superseded range requests, coalesced refresh, notification delivery across tabs, the 128-stream SSE limit and permit release after disconnect.
- Both optional controllers pass Docker integration tests. The NSC authority test proves that local revocation preserves broker access until a TLS resolver accepts the updated JWT, after which a new connection is rejected. Missing operator authority prevents revocation while account-only user creation still works. Contended operations reject immediately without a deferred mutation.
- Static demo and Pages link/network-isolation checks pass with the new shared browser module and identity setup guides.
- An independent adversarial review identified historical continuity, cross-tab timing, identity-generation and controller authority/deadline issues. Corrections and durable regression coverage are included. The review found no need for an architectural rewrite.

Remaining limits are deliberate: notifications need an open browser, OIDC does not synchronize groups or provider logout, JWT resolver acceptance does not prove cluster-wide convergence, and managed process changes do not coordinate a cluster rollout. The accepted partial endurance evidence is unchanged.

Final local release checks also passed the Windows archive, release-build OIDC/workspace browser suites and the existing authenticated three-node cluster browser workflow. The latter waits for asynchronously populated profile options after navigation.
