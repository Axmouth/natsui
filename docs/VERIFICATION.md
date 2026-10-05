# Local verification

Core checks use cargo fmt --check, cargo clippy --locked --all-targets -- -D warnings and cargo test --locked. The Linux Docker verification target supplies a disposable NATS binary and certificate fixtures so all integration cases run.

~~~sh
docker build --target verification -t natsui:verified .
docker build --target verification -t natsui-controller:verified controller
node scripts/test-site.mjs
~~~

## Browser regression

The browser check provisions its own authenticated three-node cluster and removes that test project's containers and volumes afterward. It creates a uniquely named stream and dashboard user, verifies their behavior, then deletes them. Port 14337 must be available. Docker and Playwright with an installed Chromium browser are required.

~~~sh
npm install --no-save --package-lock=false playwright@1.62.1
npx playwright install chromium
docker build -t natsui:browser .
docker build -t natsui-traffic:browser demo
node scripts/smoke-browser-stack.mjs natsui:browser natsui-traffic:browser
~~~

CI uses the same script and installs Chromium system dependencies. Screenshots are written under data/browser-check. NATSUI_BROWSER_EXECUTABLE can select an existing compatible browser. NATSUI_PLAYWRIGHT can select a locally provided Playwright module. The default module is playwright.

The regression covers one-time CLI login, independent tab profiles, per-profile settings, native resource creation, JetStream storage acknowledgment, reviewed deletion, named viewer restrictions and recovery from a removed profile. The separate literal route contract catches missing backend routes. Neither replaces semantic backend tests.

## Endurance record

scripts/soak.mjs defaults to 24 hours against a loopback dashboard. NATSUI_SOAK_URL, NATSUI_SOAK_SECONDS, NATSUI_SOAK_INTERVAL and NATSUI_SOAK_FILE select the target, duration and output. The target must permit the recorder's read-only requests. The local keyless demo is suitable.

NATSUI_SOAK_CONTAINER optionally records Docker container CPU and approximate memory alongside readiness, collection, storage and request latency. This host-side recorder requires Docker CLI access. The dashboard container itself does not need Docker socket access.

The JSONL record and adjacent summary must be inspected together. Sleep, restarts and large sampling gaps can invalidate a sustained-duration claim even when the wall-clock deadline passes. A partial or interrupted recording must retain its actual duration and failure count.


## NATS-backed login, 2026-09-22

Local Windows verification used Rust 1.94.0, NATS Server 2.11.8 and disposable TLS certificates. All 54 Rust tests passed with ignored integration tests enabled. Formatting and Clippy with warnings denied passed.

The real-broker login regression covers invalid passwords, literal password punctuation and Unicode, separate NATS accounts, stream lists with denied consumer requests, message reads without list permission, denied message reads and deletion, permitted creation, session-owned previews, denied Core publishing observed independently, history redaction, logout and password changes on the next request. TLS coverage rejects unauthenticated servers, untrusted certificates and inherited collector client certificates, and verifies that collector credential files are not used for login connections.

The deterministic session-replacement test pauses an authenticated request, removes its session, then verifies denial and zero collector-route invocations. Unit tests cover closed-by-default routes and independent history/monitoring sharing.

scripts/smoke-nats-login.mjs passed with headless Microsoft Edge on desktop and mobile viewports. It checks the real login form, rejected and accepted credentials, disabled shared-data labels, restricted admin APIs, logout and browser errors. Screenshots are written to data/browser-check/nats-login.png and nats-login-mobile.png. The test is included in Linux browser CI alongside the existing identity and workspace regressions.

Existing OIDC signed-token, mapping, profile and revocation checks passed. SSE capacity and disconnect cleanup passed. UI route, trend, synthetic demo, JavaScript syntax and Pages link checks passed, including the generated NATS login guide. The running demo was not restarted or reconfigured. This record describes local verification, not a completed remote CI or publication run.


## Setup guide polish, 2026-09-22

Generated Pages links, downloaded recipe paths, section anchors and static demo isolation passed. Docker Compose configuration checks verified that compose.nats-login.yaml enables NATS login without a dashboard-key volume, preserves /data storage, defaults both sharing flags and writes to disabled, and combines with the server-TLS overlay.

A disposable headless Edge check verified copy success and selection fallback, format-neutral announcements, restoration of both guide and landing-page button labels, in-page contents links, heading permalinks and a 390-pixel mobile viewport without horizontal page overflow. Desktop and mobile screenshots were inspected. This was documentation and static-site verification. No running deployment was changed.

## Session-only NATS and certificate login, 2026-10-05

Local verification used Rust 1.94.0, NATS Server 2.11.8 and disposable TLS fixtures. All 58 Rust tests passed, including ignored real-broker tests. Formatting, strict Clippy, UI route contracts, trend logic, simulation invariants, JavaScript syntax and generated Pages links passed.

The certificate regression checks mapped NATS identity, permission denial, mismatched keys, invalid keys, disabled certificate login, untrusted server certificates, and removal of collector authentication. PEM certificates with preambles/BOM, PEM bundles and single DER certificates authenticate successfully. Mounted DER CA and client certificates use the same parser as browser uploads. Private keys remain unencrypted PEM.

Session regressions verify empty initial history, isolation between separate logins with the same username, discarded history after logout, valid graph window queries, closed monitoring access, and bounded in-memory history and retained snapshots. The review identified an unbounded retained snapshot and an inaccurate incident budget report. Both were corrected and covered by regression checks.

scripts/smoke-nats-sessions.mjs passed in headless Edge on desktop and mobile viewports. It starts the actual dashboard with two profiles against a TCP connection counter and verifies no NATS or monitoring connection before login. It then starts a disposable real NATS server, signs in through the form with both PEM and binary DER files named client.crt, verifies session history, rejected cross-profile requests, key mismatch feedback, cleared file inputs, empty history after a new login, revoked cookies, and no lingering connections or collection after logout. Screenshots are saved under data/browser-check/certificate-login*.png and were inspected. The existing password-login browser regression also passed. The new browser check is included in Linux CI.

Compose configuration validated both new options without starting containers. Docker Engine was unavailable locally, so no container image or Ansible deployment was run. This record describes local verification, not remote CI completion or publication. Existing deployments were not changed.


## Session investigation tools, 2026-10-05

Local verification used Rust 1.94.0 and NATS Server 2.11.8. All 59 Rust tests, including the real-broker suite, passed. Strict Clippy, formatting, diagnostic evidence tests, trend tests, static-demo invariants, literal UI routes, JavaScript syntax and generated Pages links passed.

The new real-broker regression verifies native pause and resume with read-back confirmation, denied pause permission, session-owned review tokens, revision conflicts, isolated action diaries, system-account monitoring and denied monitoring without collector fallback. It records a real terminated-message advisory produced by a fixture application, checks separate-login incident isolation and rejects concurrent watches. Broker shutdown during a watch marks the window incomplete.

Diagnostic regressions cover history gaps, partial samples, resource replacement, configuration changes, counter resets, current data contradicting history, paused observation windows and unknown retention limits. Historical consumer lookup is indexed once per visible diagnostic render. Current snapshot and history use the same configuration identity.

Headless Edge checks cover PEM and DER certificate details, expiry metadata, monitoring opt-in and denial, consumer pause/resume through the review form, retention headroom, session Activity, advisory watch controls, native node and connection views, and 390-pixel mobile layout. Screenshots under data/browser-check/session-*.png were inspected. The browser tests are included in Linux CI. These are local checks, not a claim of completed remote CI or publication. Existing running deployments were not restarted.

## Deployment doctor, 2026-10-05

Local checks used Rust 1.94.0 and NATS Server 2.11.8. All 61 Rust tests, including the real-broker suite, passed after the shared configuration and profile changes. Final response-shape changes passed the two focused diagnostic tests, strict Clippy and a fresh build. The real-broker doctor script passed again against the final binary.

scripts/test-doctor.mjs verifies no network traffic from default checks, no database creation, secret redaction, invalid policy and TLS settings, additional-profile validation, oversized-file rejection before parsing/probing, HTTP monitoring conflicts, session-only monitoring exclusion and storage-path errors. Live tests cover a restricted application user, a system user, rejected credentials and DER client-certificate mapping. Broker configuration remains unchanged. These checks are included in native and container verification jobs.

The existing desktop/mobile session browser regression passed against the new binary, covering collector-free startup, private histories, profile boundaries, certificate uploads and logout. Generated Pages links, JavaScript syntax, and Compose configuration with and without the server-TLS overlay passed.

Docker Engine was unavailable, so the container image was not built or executed locally. Ansible examples were not deployed. These results do not establish remote CI completion, publication, volume durability or reverse-proxy correctness. Existing deployments were not restarted.
