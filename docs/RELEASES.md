# Container releases

## Select a fixed version

```sh
docker pull ghcr.io/axmouth/natsui:v0.2.0
```

Both `ghcr.io/axmouth/natsui:v0.2.0` and `ghcr.io/axmouth/natsui-traffic:v0.2.0` contain Linux AMD64 and ARM64 images. Docker selects the matching architecture. The traffic image is only needed for generated demo workloads.

The [setup guide](SETUP.md) describes deployment settings, persistent volumes and NATS sign-in. Replacing `:latest` with `:v0.2.0` selects the fixed release without changing those settings.

## Tags and content pins

- `latest` and `main` advance after verification and a real cluster smoke test.
- `v0.2.0` identifies a release. The publishing workflow refuses to replace an existing version tag with a different manifest digest.
- `sha-<commit>` identifies images built from a commit. These tags can change if the commit is rebuilt with updated base images.
- `@sha256:<digest>` pins exact registry content independently of tag changes. Registry administrators can still alter tags outside the workflow.

Inspect a release digest:

```sh
docker buildx imagetools inspect ghcr.io/axmouth/natsui:v0.2.0
```

The top-level `Digest` identifies the multi-platform image. An exact deployment reference has the form `ghcr.io/axmouth/natsui@sha256:<digest>`, with `<digest>` replaced by the hexadecimal value after `sha256:` in that output.

The one-command demo uses a separate Compose artifact, `ghcr.io/axmouth/natsui-demo:latest`. A dashboard version tag does not version this artifact. Commit-specific demo bundles are available as `ghcr.io/axmouth/natsui-demo:sha-<full-commit>` and contain resolved image digests.

## Publish a release

The release commit must be on `main`, with a matching `version` in `Cargo.toml` and an annotated `vMAJOR.MINOR.PATCH` Git tag. For example:

```sh
git tag -a v0.2.0 -m "Release v0.2.0"
git push --atomic origin main v0.2.0
```

The atomic push makes the release tag available before verification starts. The main branch run verifies native builds, container behavior and browser regressions. Publication then builds both architectures, exercises real NATS traffic and verifies the published Compose bundle. Only these verified images receive the version tags. No separate rebuild occurs during version promotion.

The publisher checks all destination tags before writing any. An identical digest is left in place. A different existing digest stops publication. Authentication errors, network errors and missing architecture information also stop publication. A new release requires a new version rather than replacing an existing release.

For a tag added after a main branch run has already completed, rerun that commit's **Verify and package** workflow after pushing the tag. A tag push alone does not trigger verification. The **Publish images and demo** workflow summary records the released references and digests. A Git tag alone does not establish that an image was published successfully.
