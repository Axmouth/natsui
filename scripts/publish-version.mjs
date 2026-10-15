import {execFileSync} from 'node:child_process';
import {readFileSync, appendFileSync} from 'node:fs';
import {resolve} from 'node:path';
import {fileURLToPath} from 'node:url';

const accept = 'application/vnd.oci.image.index.v1+json, application/vnd.docker.distribution.manifest.list.v2+json, application/vnd.oci.image.manifest.v1+json, application/vnd.docker.distribution.manifest.v2+json';
const digestPattern = /^sha256:[a-f0-9]{64}$/;

export function releaseTags(tags, version) {
  const releases = tags.filter(tag => /^v\d+\.\d+\.\d+$/.test(tag));
  for (const tag of releases) {
    if (tag !== `v${version}`) throw new Error(`Release tag ${tag} does not match Cargo version ${version}`);
  }
  return releases;
}

export function promotion(source, existing) {
  if (!digestPattern.test(source?.digest)) throw new Error('Missing or invalid source digest');
  const platforms = source.manifest?.manifests?.map(item => `${item.platform?.os}/${item.platform?.architecture}`) || [];
  for (const platform of ['linux/amd64', 'linux/arm64']) {
    if (!platforms.includes(platform)) throw new Error(`Source image lacks ${platform}`);
  }
  if (existing === null) return 'publish';
  if (existing?.digest === source.digest) return 'keep';
  throw new Error('Version tag already exists with a different digest. Release tags cannot be replaced.');
}

export async function manifest(repository, reference, token, request = fetch) {
  const response = await request(`https://ghcr.io/v2/${repository}/manifests/${reference}`, {
    headers: {Authorization: `Bearer ${token}`, Accept: accept},
    signal: AbortSignal.timeout(30000),
  });
  if (response.status === 404) return null;
  if (!response.ok) throw new Error(`Registry lookup failed for ${repository}:${reference}: HTTP ${response.status}`);
  const digest = response.headers.get('docker-content-digest');
  if (!digestPattern.test(digest || '')) throw new Error('Registry returned no valid manifest digest');
  return {digest, manifest: await response.json()};
}

async function main() {
  const sha = process.env.SHA;
  if (!/^[a-f0-9]{40}$/.test(sha || '')) throw new Error('SHA must identify the verified commit');
  const git = (...args) => execFileSync('git', args, {encoding: 'utf8'}).trim();
  if (git('rev-parse', 'HEAD') !== sha) throw new Error('Checkout does not match the verified commit');
  const version = readFileSync('Cargo.toml', 'utf8').match(/^version\s*=\s*"([^"]+)"/m)?.[1];
  const tags = releaseTags(git('tag', '--points-at', sha).split(/\r?\n/).filter(Boolean), version);
  if (!tags.length) { console.log('No release tag on the verified commit'); return; }
  const plans = [];
  // Preflight every image before publishing any version tag. Unknown registry state fails closed.
  for (const repository of ['axmouth/natsui', 'axmouth/natsui-traffic']) {
    const response = await fetch(`https://ghcr.io/token?service=ghcr.io&scope=repository:${repository}:pull`, {signal: AbortSignal.timeout(30000)});
    if (!response.ok) throw new Error(`Registry token request failed: HTTP ${response.status}`);
    const {token} = await response.json();
    if (!token) throw new Error('Registry returned no pull token');
    const source = await manifest(repository, `sha-${sha}`, token);
    for (const tag of tags) {
      const existing = await manifest(repository, tag, token);
      plans.push({repository, tag, token, source, action: promotion(source, existing)});
    }
  }
  for (const plan of plans) {
    const {repository, tag, token, source, action} = plan;
    if (action === 'publish') {
      execFileSync('docker', ['buildx', 'imagetools', 'create', '-t', `ghcr.io/${repository}:${tag}`, `ghcr.io/${repository}@${source.digest}`], {stdio: 'inherit'});
    }
    const result = await manifest(repository, tag, token);
    if (result?.digest !== source.digest) throw new Error(`Published digest mismatch for ${repository}:${tag}`);
    const line = `ghcr.io/${repository}:${tag} -> ${source.digest}`;
    console.log(line);
    if (process.env.GITHUB_STEP_SUMMARY) appendFileSync(process.env.GITHUB_STEP_SUMMARY, `${line}\n\n`);
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch(error => { console.error(error.message); process.exitCode = 1; });
}
