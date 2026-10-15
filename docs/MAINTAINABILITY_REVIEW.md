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


## NATS login review, 2026-09-22

An independent read-only reviewer inspected the implementation and a corrective pass. Four findings were resolved:

- A session-removal race could select the collector context. Authentication now carries the NATS session in request extensions and missing sessions fail closed. A barrier-based regression replaces the session while a request is paused and verifies that the collector route is never invoked.
- URL conversion could reinterpret password punctuation. Credentials are encoded before insertion and decoded once. The real-broker tests exercise literal commas, percent escapes and Unicode.
- Slow or denied consumer inventory requests could discard permitted streams. Consumer collection now has its own deadline and retains a partial snapshot.
- A Core protocol flush was treated as server acceptance. Core publishing now reports an unconfirmed outcome, including authorization, while JetStream storage acknowledgments remain distinct.

The follow-up review found no additional blocking credential-isolation or shared-data issue. New NATS-session endpoints require an explicit route classification. This is a dashboard data-source boundary, not a parallel NATS permission model.

Fresh connections simplify credential revocation but add authentication and connection traffic. The implementation reconstructs the route service with a request-specific App, reusing handlers without threading credentials through each endpoint. These tradeoffs are acceptable at the documented session limits. Pooling would require explicit identity binding, permission-change handling and isolation tests before replacing this approach.

## Session investigation review, 2026-10-05

An independent read-only review covered session routing, native monitoring, failure watches, certificate metadata, pause/resume and frontend diagnosis. No new credential-isolation or authorization bypass was identified. Findings corrected during the review were:

- History can lag a snapshot. Diagnosis now requires matching current counters and configuration identity, and excludes paused historical observations.
- Repeated searches through every historical consumer caused quadratic lookup cost. The new diagnostic view indexes history once and renders only on its active page.
- Monitoring sampled before a slow JetStream inventory could already be stale on arrival. Native monitoring is sampled after inventory collection.
- A successful resume omits the false paused field. Verification now checks the validated consumer response and cleared pause time.
- Subscription streams can remain open after connection loss. The bounded watch checks connection state and records interrupted coverage explicitly.

Native tools reuse sanitized monitoring projections, the existing review-token store and private session storage. New feature behavior remains separate from connection and authentication policy. Existing frontend globals and full-page rendering remain architectural constraints. The diagnostic changes do not establish large-inventory performance for every older view.

Real-broker and browser regressions cover the corrected paths. The reviewer inspected source without independently running those tests. This review does not qualify container deployment or long-duration operation.

## Deployment diagnostic review, 2026-10-05

An independent read-only review inspected the new CLI, shared validators, profile extraction and real-broker test script. Corrected findings included omitted connection validation for additional profiles, dependent TLS parsing after a failed file-size check, unvalidated collector HTTP monitoring conflicts and overly permissive response-shape checks.

Startup and diagnostics share login policy, connection validation, profile definitions, monitoring construction and deployment-setting parsers. Diagnostic reporting and explicit read probes remain separate from normal collection. Controlled validation errors can identify the failing setting, while external errors and response payloads remain excluded from reports. The reviewer did not run tests independently. Local regression evidence is recorded in VERIFICATION.md.

## Native HTTPS and monitoring review, 2026-10-15

An independent read-only review covered native TLS termination, bounded concurrent handshakes, local CLI destinations, certificate verification, origin/cookie handling and session monitoring controls. Corrections addressed bracketed IPv6 public names and stale snapshots overwriting confirmed monitoring choices. The follow-up review found no remaining blockers in that scope.

The HTTPS listener uses the existing Rustls stack and Axum listener interface. Health and login commands share a bounded HTTP client that connects only to IPv4 loopback while verifying the configured public DNS name or IP certificate identity. Public DNS and redirects cannot redirect CLI credentials. TLS certificate reload and issuance remain deployment responsibilities.

Monitoring choices use a short-held mutex and revision counter. Snapshots retain the revision captured before collection, so delayed results cannot reverse a newer choice. The browser regression delays a real snapshot across Enable and verifies the confirmed state. Review was source-only. Executed tests are recorded in VERIFICATION.md.
