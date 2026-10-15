import assert from 'node:assert/strict';
import {releaseTags, promotion, manifest} from './publish-version.mjs';

const digest = `sha256:${'a'.repeat(64)}`;
const source = {digest, manifest: {manifests: ['amd64', 'arm64'].map(architecture => ({platform: {os: 'linux', architecture}}))}};
assert.deepEqual(releaseTags(['unrelated', 'v0.1.0'], '0.1.0'), ['v0.1.0']);
assert.deepEqual(releaseTags([], '0.1.0'), []);
assert.throws(() => releaseTags(['v0.2.0'], '0.1.0'), /does not match/);
assert.equal(promotion(source, null), 'publish');
assert.equal(promotion(source, {digest}), 'keep');
assert.throws(() => promotion(source, {digest: `sha256:${'b'.repeat(64)}`}), /cannot be replaced/);
assert.throws(() => promotion(source, undefined), /cannot be replaced/);
assert.throws(() => promotion(null, null), /source digest/);
assert.throws(() => promotion({digest, manifest: {manifests: [source.manifest.manifests[0]]}}, null), /linux\/arm64/);
assert.equal(await manifest('axmouth/natsui', 'v0.1.0', 'test', async () => new Response(null, {status: 404})), null);
for (const status of [401, 403, 429, 500]) {
  await assert.rejects(manifest('axmouth/natsui', 'v0.1.0', 'test', async () => new Response(null, {status})), new RegExp(`HTTP ${status}`));
}
await assert.rejects(manifest('axmouth/natsui', 'v0.1.0', 'test', async () => { throw new Error('network unavailable'); }), /network unavailable/);
await assert.rejects(manifest('axmouth/natsui', 'v0.1.0', 'test', async () => new Response('{}')), /valid manifest digest/);
assert.deepEqual(await manifest('axmouth/natsui', 'v0.1.0', 'test', async () => new Response(JSON.stringify(source.manifest), {headers: {'docker-content-digest': digest}})), source);
console.log('Release guards passed: version match, both platforms, no replacement, registry failures');
