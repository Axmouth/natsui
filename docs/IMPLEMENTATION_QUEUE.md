# Pending feature implementation

Approved scope: the complete pending feature list discussed after the deployment guide.
The porting ledger remains the feature and verification record.

- [x] Finish Ansible deployment and rotation verification.
- [x] Idempotent authenticated stack initialization and one-time login links.
- [x] Dashboard identities, viewer/operator/admin permissions and HTTPS proxy deployment.
- [x] Monitoring private CAs, mutual TLS and HTTP authentication.
- [x] Stream and consumer creation and reviewed deletion.
- [x] Explicit Core and JetStream test publishing.
- [x] Dedicated Key Value and Object Store browsing.
- [x] NATS user management with an explicit deployment authority model.
- [x] Profile switching with isolated history and credentials.
- [x] Optional managed configuration validation, reload/restart and verification.
- [x] Review endurance evidence and record its duration and limitations. The partial run was accepted for the current scope on 2026-09-15. A continuous 24-hour pass remains unverified.

Features land in verified increments. External NATS permissions remain authoritative.
Configuration-based NATS users and JWT users require different management adapters.
A completed short smoke run does not qualify as a 24-hour endurance run.

- [x] Independent adversarial maintainability review after the final implementation, followed by concrete fixes.

The 24-hour endurance recording started on 2026-09-14 at 17:04 UTC against a separate local three-node cluster and the verified runtime. Its record is data/soak/2026-09-14-shared-deployment.jsonl. Host sleep interrupted this attempt on 2026-09-14 at 23:41 UTC until 2026-09-15 at 06:20 UTC. The pre-sleep record contains 757 healthy samples spanning 6 hours 36 minutes, followed by one unavailable response across suspension. Sampling recovered after wake. This does not establish a continuous 24-hour pass. The partial evidence was accepted for the current scope on 2026-09-15. The follow-up remains paused. Any future uninterrupted run requires a host that stays awake. See RELEASE_CHECKLIST.md for the measured duration and limitations.

## Approved follow-up, 2026-09-15

- [x] Full-window backlog aggregation with extrema and continuity evidence.
- [x] Named investigation views stored in the browser.
- [x] Opt-in desktop notifications for new attention transitions.
- [x] Reported inter-node route topology.
- [x] SSE update transport with polling fallback.
- [x] Per-profile access restrictions.
- [x] Optional OIDC dashboard login.
- [x] Optional NATS JWT identity administration with explicit authority.
- [x] Reconcile setup and ledger, and wire the follow-up into verification and publication workflows.
- [x] Adversarial review of the follow-up implementation.

The follow-up ships through the existing main-branch verification gate. GitHub Actions records native runner results and image/Pages promotion for each commit. Local evidence is recorded in RELEASE_CHECKLIST.md.
