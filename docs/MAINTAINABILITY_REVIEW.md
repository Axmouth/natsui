# Maintainability review

Reviewed working tree: shared identities, profiles, native operations, bucket browsing and optional deployment control. Review date: 2026-09-14. An independent read-only reviewer inspected the original implementation and a corrective pass.

## Findings and resolution

| Finding | Resolution | Evidence |
| --- | --- | --- |
| Deleted usernames could revive earlier grants | Grants bind to key digest and revision | Session and one-time ticket recreation regression |
| Stale user forms could affect a recreated account | Persistent revision allocator shares the user mutation transaction | Stale deletion conflicts after recreation |
| Browser-wide profile cookie could retarget another tab | Every API request carries the tab's selected profile | Explicit dispatch and two-tab browser checks |
| Removed profile could block the shared page | Shared assets use the default router, invalid selection keeps recovery visible | API conflict, shell recovery and single-remaining-profile browser checks |
| Duplicate blocking identity lookups | One middleware boundary, bounded identity cache, blocking task for SQLite mutations | Role/revocation tests and source review |
| One review slot across users | Shared bounded session-owned review storage | Independent owners, confirmation, expiry, capacity and one-use tests |
| TLS handshake could block controller acceptance | Deferred handshake in bounded workers with socket timeouts | Silent TCP peer plus concurrent authenticated HTTPS status |
| Controller stayed alive after broker crash | Watchdog exits on unexpected child termination | Real child kill, intentional restart and process-exit tests |
| Operations page used a missing endpoint | Correct capabilities route | Literal frontend/backend route contract |
| Dense new feature scripts | Format into ordinary blocks and functions | JavaScript syntax and browser checks |

## Assessment

The final independent pass confirmed both follow-up findings resolved and found no new significant issues in those corrections. The verdict was maintainable for the current scope, with no unresolved blockers from the review. The reviewer inspected source and tests without independently rerunning the suites.

The domain modules remain small enough to follow. Shared production and demo markup, styling and feature scripts avoid maintaining two dashboards. The review store and middleware consolidation remove specific duplication without adding a framework.

Frontend state still relies on shared browser globals and DOM event handlers. A route contract catches literal endpoint drift but cannot replace page-level interaction tests. New backend routes should include response and permission checks plus at least one relevant browser flow. The synthetic API must continue to label approximations and preserve NATS semantics.

The optional controller adds a separate Python component and deployment authority. Its scope should remain narrow. JWT provisioning, clustered rollout, arbitrary configuration editing and unrelated process control require distinct designs. No generic shell or Docker socket is part of this adapter.

The existing Activity store is an operational record, not a tamper-proof compliance log. Access roles are global across profiles. Large-scale deployment and long-duration performance claims require separate evidence.
