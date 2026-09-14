import { execFileSync } from 'node:child_process';
import { mkdtempSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { randomUUID } from 'node:crypto';

const [image, traffic] = process.argv.slice(2);
if (!image || !traffic)
  throw new Error(
    'Usage: node scripts/smoke-browser-stack.mjs DASHBOARD_IMAGE TRAFFIC_IMAGE',
  );
const root = resolve(fileURLToPath(new URL('..', import.meta.url)));
const dir = mkdtempSync(join(tmpdir(), 'natsui-browser-stack-'));
const project = 'natsui-browser-' + randomUUID().slice(0, 8),
  auth = project + '-auth';
const env = { ...process.env, NATSUI_DEMO_PORT: '14337' };
const docker = (...args) =>
  execFileSync('docker', args, {
    env,
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'pipe'],
    windowsHide: true,
  }).trim();
const config = {
  profiles: [
    {
      id: 'staging',
      name: 'Browser staging',
      urls: ['nats://nats-1:4222'],
      allow_writes: true,
    },
  ],
};
const override = {
  services: {
    dashboard: {
      image,
      environment: {
        NATSUI_AUTH_TOKEN_FILE: '/run/auth/access.key',
        NATSUI_PROFILES_FILE: '/run/profiles.json',
      },
      volumes: [
        {
          type: 'volume',
          source: 'browser-auth',
          target: '/run/auth',
          read_only: true,
        },
        {
          type: 'bind',
          source: join(dir, 'profiles.json'),
          target: '/run/profiles.json',
          read_only: true,
        },
      ],
    },
    traffic: { image: traffic },
  },
  volumes: { 'browser-auth': { external: true, name: auth } },
};
writeFileSync(join(dir, 'profiles.json'), JSON.stringify(config), {
  mode: 0o644,
});
writeFileSync(join(dir, 'override.json'), JSON.stringify(override));
const compose = [
  'compose',
  '-p',
  project,
  '-f',
  join(root, 'demo/published.yaml'),
  '-f',
  join(dir, 'override.json'),
];
try {
  docker(
    'run',
    '--rm',
    '-v',
    auth + ':/data',
    image,
    '--init-auth',
    '/data/access.key',
  );
  docker(...compose, 'up', '-d', '--wait', '--wait-timeout', '150');
  const container = docker(...compose, 'ps', '-q', 'dashboard');
  execFileSync(process.execPath, [join(root, 'scripts/smoke-browser.mjs')], {
    env: {
      ...env,
      NATSUI_BROWSER_URL: 'http://127.0.0.1:14337',
      NATSUI_BROWSER_TEST_WRITES: '1',
      NATSUI_BROWSER_CONTAINER: container,
    },
    stdio: 'inherit',
    windowsHide: true,
  });
  console.log('Authenticated disposable cluster browser regression passed.');
} finally {
  try {
    docker(...compose, 'down', '-v', '--remove-orphans');
  } catch {}
  try {
    docker('volume', 'rm', auth);
  } catch {}
  rmSync(dir, { recursive: true, force: true });
}
