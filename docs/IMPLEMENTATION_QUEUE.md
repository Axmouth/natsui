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
- [ ] Complete an endurance recording and report its actual duration and failures.

Features land in verified increments. External NATS permissions remain authoritative.
Configuration-based NATS users and JWT users require different management adapters.
A completed short smoke run does not qualify as a 24-hour endurance run.

- [x] Independent adversarial maintainability review after the final implementation, followed by concrete fixes.

The 24-hour endurance recording started on 2026-09-14 at 17:04 UTC against a separate local three-node cluster and the verified runtime. Its record is data/soak/2026-09-14-shared-deployment.jsonl. A follow-up checks for completion or interruption. This item remains open until the record has been reviewed.
