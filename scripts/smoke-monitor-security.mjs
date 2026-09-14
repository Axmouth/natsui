import assert from 'node:assert/strict';
import { spawn, execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync, mkdtempSync, rmSync } from 'node:fs';
import { createServer } from 'node:https';
import { createServer as tcpServer } from 'node:net';
import { once } from 'node:events';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(
  process.env.NATSUI_TEST_ROOT || fileURLToPath(new URL('..', import.meta.url)),
);
const binary = resolve(
  process.argv[2] ||
    join(
      root,
      'target/debug',
      process.platform === 'win32' ? 'natsui.exe' : 'natsui',
    ),
);
const fixtures =
  process.env.NATSUI_TLS_FIXTURES ||
  execFileSync(process.execPath, [join(root, 'scripts/tls-fixtures.mjs')], {
    encoding: 'utf8',
  }).trim();
const dir = mkdtempSync(join(tmpdir(), 'natsui-monitor-security-'));
const ca = readFileSync(join(fixtures, 'ca.pem'));
const tls = {
  cert: readFileSync(join(fixtures, 'server.pem')),
  key: readFileSync(join(fixtures, 'server.key')),
};
const servers = [];
const credentials = [];
let redirects = 0;
let child;
const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
async function listen(options, handler) {
  const server = createServer(options, handler);
  server.on('tlsClientError', () => {});
  servers.push(server);
  server.listen(0, '127.0.0.1');
  await once(server, 'listening');
  return `https://127.0.0.1:${server.address().port}`;
}
const respond = (request, response) => {
  response.setHeader('Content-Type', 'application/json');
  response.end(
    JSON.stringify({
      total: 1,
      connections: [
        {
          cid: 7,
          name: 'secured-worker',
          subscriptions: 2,
        },
      ],
    }),
  );
};
try {
  const secure = await listen(
    { ...tls, ca, requestCert: true, rejectUnauthorized: true },
    respond,
  );
  const basic = await listen(tls, (request, response) => {
    credentials.push(['basic', request.headers.authorization]);
    if (
      request.headers.authorization !==
      'Basic ' + Buffer.from('observer:basic-secret').toString('base64')
    ) {
      response.writeHead(401).end();
      return;
    }
    respond(request, response);
  });
  const bearer = await listen(tls, (request, response) => {
    credentials.push(['bearer', request.headers.authorization]);
    if (request.headers.authorization !== 'Bearer endpoint-only-token') {
      response.writeHead(401).end();
      return;
    }
    respond(request, response);
  });
  const sink = await listen(tls, (request, response) => {
    redirects++;
    respond(request, response);
  });
  const redirect = await listen(tls, (_, response) =>
    response.writeHead(302, { Location: sink + '/connz' }).end(),
  );
  writeFileSync(join(dir, 'password'), 'basic-secret');
  writeFileSync(join(dir, 'bearer'), 'endpoint-only-token');
  const authority = { ca_file: join(fixtures, 'ca.pem') };
  const config = [
    {
      url: secure,
      ...authority,
      cert_file: join(fixtures, 'client.pem'),
      key_file: join(fixtures, 'client.key'),
    },
    {
      url: basic,
      ...authority,
      username: 'observer',
      password_file: join(dir, 'password'),
    },
    { url: bearer, ...authority, bearer_file: join(dir, 'bearer') },
    { url: secure, ...authority },
    { url: basic, ...authority },
    { url: bearer },
    {
      url: redirect,
      ...authority,
      bearer_file: join(dir, 'bearer'),
    },
  ];
  writeFileSync(join(dir, 'endpoints.json'), JSON.stringify(config));
  const portProbe = tcpServer().listen(0, '127.0.0.1');
  await once(portProbe, 'listening');
  const port = portProbe.address().port;
  await new Promise((resolve) => portProbe.close(resolve));
  const env = Object.fromEntries(
    Object.entries(process.env).filter(([name]) => !name.startsWith('NATSUI_')),
  );
  child = spawn(binary, ['--demo'], {
    env: {
      ...env,
      NATSUI_PORT: String(port),
      NATSUI_DATA_DIR: join(dir, 'data'),
      NATSUI_MONITOR_CONFIG_FILE: join(dir, 'endpoints.json'),
    },
    stdio: 'ignore',
    windowsHide: true,
  });
  const base = `http://127.0.0.1:${port}`;
  let ready = false;
  for (let attempt = 0; attempt < 60; attempt++) {
    if (child.exitCode !== null)
      throw new Error('Test dashboard exited during startup');
    try {
      if ((await fetch(base + '/healthz')).ok) {
        ready = true;
        break;
      }
    } catch {}
    await delay(100);
  }
  assert.ok(ready, 'Test dashboard started');
  const result = await fetch(base + '/api/monitoring/connections', {
    signal: AbortSignal.timeout(10000),
  });
  assert.equal(result.status, 200);
  const { nodes } = await result.json();
  assert.deepEqual(
    nodes.map((node) => node.status),
    [
      'complete',
      'complete',
      'complete',
      'unavailable',
      'unavailable',
      'unavailable',
      'unavailable',
    ],
  );
  for (const node of nodes.slice(0, 3))
    assert.equal(node.rows[0].name, 'secured-worker');
  for (const node of nodes.slice(3))
    assert.equal(
      node.rows,
      undefined,
      'Failed collection must not become an empty healthy inventory',
    );
  assert.equal(
    redirects,
    0,
    'Monitoring redirects must never contact the target',
  );
  assert.ok(
    credentials.some(
      ([kind, value]) => kind === 'basic' && value === undefined,
    ),
    'An endpoint without credentials must not inherit another entry',
  );
  assert.ok(
    !credentials.some(
      ([kind, value]) => kind === 'basic' && value?.startsWith('Bearer'),
    ),
  );
  console.log(
    'Monitoring private CA, mTLS, Basic/Bearer isolation, missing credentials, untrusted certificate and redirect rejection passed.',
  );
} finally {
  if (child && child.exitCode === null) {
    const stopped = once(child, 'exit');
    child.kill();
    await stopped;
  }
  await Promise.all(
    servers.map(
      (server) =>
        new Promise((resolve) => {
          server.close(resolve);
          server.closeAllConnections();
        }),
    ),
  );
  rmSync(dir, { recursive: true, force: true });
}
