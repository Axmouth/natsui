# Publishing operations

Publication follows successful main-branch `Verify and package` runs through `Publish images and demo`. Pull requests and fork workflow runs do not receive publication access. The images run on native AMD64 and ARM64 runners and pass the complete demo startup and traffic smoke check before upload.

## Registry outputs

| Package | Purpose |
| --- | --- |
| ghcr.io/axmouth/natsui | Standalone Rust dashboard with embedded UI |
| ghcr.io/axmouth/natsui-traffic | Dedicated workload generator for the isolated demo |
| ghcr.io/axmouth/natsui-demo | Docker Compose OCI artifact containing the full demo |

Main-branch publications produce `latest`, `main` and `sha-<full-commit>` tags. The Compose artifact pins dashboard, traffic and NATS images to registry digests. No versioned release or stable-version guarantee is implied by `latest`.

Package visibility must allow anonymous reads. If a package is private, its owner can change visibility to Public in package settings. The initial Natsui publication was verified with anonymous access to all three packages. Anonymous manifest access and a fresh unauthenticated pull verify public availability. A workflow login succeeding is not evidence of anonymous access.

## Pages

Repository Settings, Pages, Build and deployment, Source must be GitHub Actions. The `pages` job uses the standard upload/deploy Pages actions and the `github-pages` environment. The site URL is https://axmouth.github.io/natsui/ and the simulation is under /natsui/demo/.

`node scripts/test-site.mjs` builds the site and checks project-relative assets and navigation, the simulation's network-blocking CSP, and representative demo API routes with network calls forbidden. `node scripts/build-site.mjs` writes only generated assets under `dist-site`. The source lives in `site` and the existing `web` dashboard remains the single UI implementation.

## Local verification

```sh
docker build -t natsui:publish-test .
docker build -t natsui-traffic:publish-test demo
node scripts/smoke-published.mjs natsui:publish-test natsui-traffic:publish-test
node scripts/test-site.mjs
```

The smoke script uses a unique Compose project, fresh named volumes and port 14321. `NATSUI_SMOKE_PORT` overrides that test port. Its cleanup removes only the test project and its volumes. Verification checks three replicated streams, five consumers, node monitoring, changing native backlog and append counters, retained record inspection and stored history.

Published-bundle lifecycle commands use the same OCI reference and Compose project name. Down retains named volumes; down -v explicitly deletes the selected demo's volumes. The browser-only demo never starts containers or connects to a broker.
