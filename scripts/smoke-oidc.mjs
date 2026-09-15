import assert from 'node:assert/strict';
import { spawn, execFileSync } from 'node:child_process';
import { createServer } from 'node:https';
import { request as httpRequest } from 'node:http';
import { createServer as tcpServer } from 'node:net';
import { once } from 'node:events';
import { readFileSync, writeFileSync, mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  generateKeyPairSync,
  sign,
  createHash,
  randomBytes,
} from 'node:crypto';

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
const dir = mkdtempSync(join(tmpdir(), 'natsui-oidc-'));
const fixtures =
  process.env.NATSUI_TLS_FIXTURES ||
  execFileSync(process.execPath, [join(root, 'scripts/tls-fixtures.mjs')], {
    encoding: 'utf8',
  }).trim();
const pair = generateKeyPairSync('rsa', { modulusLength: 2048 });
const jwk = {
  ...pair.publicKey.export({ format: 'jwk' }),
  kid: 'test-key',
  alg: 'RS256',
  use: 'sig',
};
const codes = new Map();
let issuer,
  child,
  output = '';
const provider = createServer(
  {
    cert: readFileSync(join(fixtures, 'server.pem')),
    key: readFileSync(join(fixtures, 'server.key')),
  },
  async (req, res) => {
    const send = (value) => {
      res.setHeader('content-type', 'application/json');
      res.end(JSON.stringify(value));
    };
    if (req.url === '/.well-known/openid-configuration')
      return send({
        issuer,
        authorization_endpoint: issuer + '/authorize',
        token_endpoint: issuer + '/token',
        jwks_uri: issuer + '/jwks',
        response_types_supported: ['code'],
        subject_types_supported: ['public'],
        id_token_signing_alg_values_supported: ['RS256'],
        token_endpoint_auth_methods_supported: ['client_secret_basic'],
        code_challenge_methods_supported: ['S256'],
      });
    if (req.url.startsWith('/authorize?')) {
      const url = new URL(req.url, issuer),
        code = randomBytes(12).toString('hex');
      codes.set(code, {
        nonce: url.searchParams.get('nonce'),
        challenge: url.searchParams.get('code_challenge'),
        overrides: {},
      });
      const callback = new URL(url.searchParams.get('redirect_uri'));
      callback.searchParams.set('state', url.searchParams.get('state'));
      callback.searchParams.set('code', code);
      res.writeHead(302, { location: callback.href });
      return res.end();
    }
    if (req.url === '/jwks') return send({ keys: [jwk] });
    if (req.url === '/token') {
      let body = '';
      for await (const part of req) body += part;
      const q = new URLSearchParams(body),
        flow = codes.get(q.get('code'));
      codes.delete(q.get('code'));
      if (
        !flow ||
        req.headers.authorization !==
          'Basic ' + Buffer.from('client:test-secret').toString('base64') ||
        createHash('sha256')
          .update(q.get('code_verifier') || '')
          .digest('base64url') !== flow.challenge
      ) {
        res.statusCode = 400;
        return send({ error: 'invalid_grant' });
      }
      const now = Math.floor(Date.now() / 1000),
        claims = {
          iss: issuer,
          sub: 'test-subject',
          aud: 'client',
          exp: now + 300,
          iat: now,
          nonce: flow.nonce,
          ...flow.overrides,
        };
      const encoded =
        Buffer.from(
          JSON.stringify({
            alg: 'RS256',
            kid: 'test-key',
          }),
        ).toString('base64url') +
        '.' +
        Buffer.from(JSON.stringify(claims)).toString('base64url');
      let signature = sign(
        'RSA-SHA256',
        Buffer.from(encoded),
        pair.privateKey,
      ).toString('base64url');
      if (flow.badSignature)
        signature = Buffer.alloc(256).toString('base64url');
      return send({
        access_token: 'test-access',
        token_type: 'Bearer',
        id_token: encoded + '.' + signature,
      });
    }
    res.statusCode = 404;
    res.end();
  },
);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
async function freePort() {
  const server = tcpServer();
  server.listen(0, '127.0.0.1');
  await once(server, 'listening');
  const port = server.address().port;
  await new Promise((r) => server.close(r));
  return port;
}
await new Promise((r) => provider.listen(0, '127.0.0.1', r));
issuer = 'https://127.0.0.1:' + provider.address().port;
const port = await freePort();
const proxy = createServer(
  {
    cert: readFileSync(join(fixtures, 'server.pem')),
    key: readFileSync(join(fixtures, 'server.key')),
  },
  (req, res) => {
    const upstream = httpRequest(
      {
        host: '127.0.0.1',
        port,
        path: req.url,
        method: req.method,
        headers: req.headers,
      },
      (response) => {
        res.writeHead(response.statusCode, response.headers);
        response.pipe(res);
      },
    );
    upstream.on('error', () => {
      if (!res.headersSent) res.writeHead(502);
      res.end();
    });
    req.pipe(upstream);
  },
);
await new Promise((r) => proxy.listen(0, '127.0.0.1', r));
const origin = 'https://localhost:' + proxy.address().port;
function request(path, options = {}) {
  return new Promise((resolve, reject) => {
    const body = options.body ? JSON.stringify(options.body) : null;
    const req = httpRequest(
      {
        host: '127.0.0.1',
        port,
        path,
        method: body ? 'POST' : 'GET',
        headers: {
          Host: new URL(origin).host,
          ...(body
            ? {
                'content-type': 'application/json',
                'content-length': Buffer.byteLength(body),
                'x-natsui-request': '1',
                Origin: origin,
              }
            : {}),
          ...options.headers,
        },
      },
      (res) => {
        let text = '';
        res.on('data', (b) => (text += b));
        res.on('end', () => {
          let json;
          try {
            json = JSON.parse(text);
          } catch {}
          resolve({
            status: res.statusCode,
            headers: res.headers,
            text,
            json,
          });
        });
      },
    );
    req.on('error', reject);
    req.setTimeout(15000, () => req.destroy(Error('Timeout')));
    req.end(body);
  });
}
const key = randomBytes(32).toString('hex');
writeFileSync(join(dir, 'access.key'), key);
writeFileSync(join(dir, 'client-secret'), 'test-secret');
writeFileSync(
  join(dir, 'oidc.json'),
  JSON.stringify({
    issuer,
    client_id: 'client',
    client_secret_file: join(dir, 'client-secret'),
    ca_file: join(fixtures, 'ca.pem'),
    subjects: {
      'test-subject': {
        user: 'reader',
        identity_revision: 1,
      },
    },
  }),
);
writeFileSync(
  join(dir, 'policy.json'),
  JSON.stringify({ profiles: { default: ['reader'], stage: [] } }),
);
writeFileSync(
  join(dir, 'profiles.json'),
  JSON.stringify({
    profiles: [
      {
        id: 'stage',
        name: 'Denied stage',
        urls: ['nats://127.0.0.1:9'],
      },
    ],
  }),
);
try {
  child = spawn(binary, ['--demo'], {
    env: {
      ...process.env,
      NATSUI_PORT: String(port),
      NATSUI_DATA_DIR: dir,
      NATSUI_AUTH_TOKEN_FILE: join(dir, 'access.key'),
      NATSUI_PUBLIC_URL: origin,
      NATSUI_OIDC_CONFIG_FILE: join(dir, 'oidc.json'),
      NATSUI_ACCESS_POLICY_FILE: join(dir, 'policy.json'),
      NATSUI_PROFILES_FILE: join(dir, 'profiles.json'),
      NATSUI_CONTAINER: '0',
      NATSUI_MANAGED_CONFIG_FILE: undefined,
    },
    windowsHide: true,
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  child.stdout.on('data', (b) => (output += b));
  child.stderr.on('data', (b) => (output += b));
  let available;
  for (let i = 0; i < 80; i++) {
    try {
      available = await request('/api/auth/oidc');
      if (available.status === 200) break;
    } catch {}
    await sleep(100);
  }
  assert.equal(available?.json?.enabled, true, output);
  const login = await request('/api/auth/login', { body: { key } });
  assert.equal(login.status, 204);
  const admin = login.headers['set-cookie'][0].split(';')[0];
  const created = await request('/api/users', {
    body: { id: 'reader', role: 'viewer' },
    headers: { Cookie: admin },
  });
  assert.equal(created.status, 200, created.text);
  async function begin(overrides = {}, badSignature = false) {
    const started = await request('/api/auth/oidc/start');
    assert.equal(started.status, 307, started.text);
    const url = new URL(started.headers.location),
      code = randomBytes(12).toString('hex');
    codes.set(code, {
      nonce: url.searchParams.get('nonce'),
      challenge: url.searchParams.get('code_challenge'),
      overrides,
      badSignature,
    });
    return {
      path:
        '/api/auth/oidc/callback?' +
        new URLSearchParams({
          code,
          state: url.searchParams.get('state'),
        }),
      cookie: started.headers['set-cookie'][0].split(';')[0],
    };
  }
  const wrongCookie = await begin();
  assert.equal(
    (
      await request(wrongCookie.path, {
        headers: { Cookie: '__Host-natsui_oidc=wrong' },
      })
    ).status,
    401,
  );
  const valid = await begin(),
    signed = await request(valid.path, {
      headers: { Cookie: valid.cookie },
    });
  assert.equal(signed.status, 200, signed.text);
  const cookie = signed.headers['set-cookie']
    .find((c) => c.startsWith('natsui_session='))
    .split(';')[0];
  assert.equal(
    (
      await request(valid.path, {
        headers: { Cookie: valid.cookie },
      })
    ).status,
    401,
  );
  assert.equal(
    (
      await request('/api/snapshot', {
        headers: { Cookie: cookie },
      })
    ).status,
    200,
  );
  assert.equal(
    (
      await request('/api/snapshot', {
        headers: {
          Cookie: cookie,
          'X-Natsui-Profile': 'stage',
        },
      })
    ).status,
    403,
  );
  assert.deepEqual(
    (
      await request('/api/profiles', {
        headers: { Cookie: cookie },
      })
    ).json.profiles.map((p) => p.id),
    ['default'],
  );
  assert.equal(
    (
      await request('/api/profiles/select', {
        headers: { Cookie: cookie },
        body: { id: 'stage' },
      })
    ).status,
    403,
  );
  assert.equal(
    (await request('/api/users', { headers: { Cookie: cookie } })).status,
    403,
  );
  for (const overrides of [
    { iss: 'https://wrong.example' },
    { aud: 'wrong' },
    { nonce: 'wrong' },
    { exp: 1 },
    { sub: 'unmapped' },
    { azp: 'wrong' },
    { aud: ['client', 'other'] },
  ]) {
    const flow = await begin(overrides);
    assert.equal(
      (
        await request(flow.path, {
          headers: { Cookie: flow.cookie },
        })
      ).status,
      401,
      JSON.stringify(overrides),
    );
  }
  const bad = await begin({}, true);
  assert.equal(
    (await request(bad.path, { headers: { Cookie: bad.cookie } })).status,
    401,
  );
  if (process.env.NATSUI_OIDC_BROWSER === '1') {
    const { chromium } = await import(
      process.env.NATSUI_PLAYWRIGHT || 'playwright'
    );
    const browser = await chromium.launch({
      headless: true,
      ...(process.env.NATSUI_BROWSER_EXECUTABLE
        ? {
            executablePath: process.env.NATSUI_BROWSER_EXECUTABLE,
          }
        : {}),
    });
    try {
      const context = await browser.newContext({
        ignoreHTTPSErrors: true,
      });
      const page = await context.newPage();
      await page.goto(origin + '/login');
      await page.locator('#oidc-login').waitFor({ state: 'visible' });
      await page.locator('#oidc-login a').click();
      await page
        .waitForURL((url) => url.origin === origin && url.pathname === '/', {
          timeout: 15000,
        })
        .catch(async (error) => {
          console.error(
            'OIDC browser location',
            new URL(page.url()).pathname,
            'body',
            (await page.locator('body').innerText()).slice(0, 250),
          );
          throw error;
        });
      await page.waitForFunction(
        () =>
          document.querySelector('#app') ||
          document.querySelector('#page-overview'),
      );
      const identity = await page.evaluate(async () => {
        const response = await fetch('/api/auth/me');
        return {
          status: response.status,
          value: await response.json(),
        };
      });
      assert.equal(identity.status, 200);
      assert.equal(identity.value.role, 'viewer');
      const session = (await context.cookies()).find(
        (c) => c.name === 'natsui_session',
      );
      assert.equal(session.sameSite, 'Strict');
      assert.equal(session.secure, true);
      console.log(
        'Cross-site HTTPS OIDC browser redirect commits the Strict session and reaches the dashboard.',
      );
    } finally {
      await browser.close();
    }
  }
  const disabled = await request('/api/users/reader', {
    body: { action: 'disable', revision: created.json.revision },
    headers: { Cookie: admin },
  });
  assert.equal(disabled.status, 200, disabled.text);
  assert.equal(
    (
      await request('/api/snapshot', {
        headers: { Cookie: cookie },
      })
    ).status,
    401,
  );
  const denied = await begin();
  assert.equal(
    (
      await request(denied.path, {
        headers: { Cookie: denied.cookie },
      })
    ).status,
    401,
  );
  const deleted = await request('/api/users/reader', {
    body: { action: 'delete', revision: disabled.json.revision },
    headers: { Cookie: admin },
  });
  assert.equal(deleted.status, 200);
  const replacement = await request('/api/users', {
    body: { id: 'reader', role: 'admin' },
    headers: { Cookie: admin },
  });
  assert.equal(replacement.status, 200);
  const oldMapping = await begin();
  assert.equal(
    (
      await request(oldMapping.path, {
        headers: { Cookie: oldMapping.cookie },
      })
    ).status,
    401,
  );
  console.log(
    'OIDC HTTPS discovery/JWKS, PKCE, signed token, issuer/audience/nonce/expiry/azp rejection, browser binding, replay, explicit mapping, profile policy and revocation passed.',
  );
} finally {
  if (child && child.exitCode === null) {
    child.kill();
    await once(child, 'exit');
  }
  provider.closeAllConnections();
  await new Promise((r) => provider.close(r));
  proxy.closeAllConnections();
  await new Promise((r) => proxy.close(r));
  rmSync(dir, { recursive: true, force: true });
}
