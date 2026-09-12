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
- CI definition for Windows, Linux and macOS artifacts; full Linux container verification target.

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
- The short Linux endurance-recorder smoke test completed without readiness failures. A separate 24-hour Windows run started at 2026-09-12 00:43:45 UTC and writes data/soak/beta-24h.jsonl plus its summary. The recording was subsequently interrupted. Its 279 observations include one unavailable collection and long observation gaps; it does not establish a continuous 24-hour pass.

NATS Server 2.11.8 is a compatibility test baseline, not a claim that it is the latest or recommended server patch. The password-based restricted identity and mutual TLS cases were exercised; operator-issued JWT/account policy combinations still need deployment-specific validation.

## Before publication

- Complete and review a 24-hour endurance report; a started or short run is not a completed soak.
- Run the prepared CI workflow on a chosen GitHub repository. macOS and additional native architectures require successful runner results before support claims.
- Source repository: https://github.com/Axmouth/natsui. Release-binary distribution and public demo hosting remain separate publication steps.
- Have fresh users follow the quick start and investigate actual operational questions. Record installation failures and confusing evidence semantics.
- Review broker compatibility beyond the tested baseline, including deployment-specific account/JWT policies.

## Deferred beyond the local beta

Shared login, roles, public HTTP exposure, resource creation/deletion, server configuration editing and restart supervision remain separate capabilities. Native stream and consumer configuration editing is opt-in for local operator use; default connections remain read-only.
