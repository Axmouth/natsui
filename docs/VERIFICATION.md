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
