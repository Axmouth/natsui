import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve, dirname, basename } from 'node:path';
import { once } from 'node:events';
import { createServer } from 'node:net';

const binary = resolve(process.argv[2] || 'target/debug/natsui' + (process.platform === 'win32' ? '.exe' : ''));
const brokerBinary = process.env.NATSUI_TEST_SERVER;
assert.ok(brokerBinary, 'NATSUI_TEST_SERVER is required');
const { chromium } = await import(process.env.NATSUI_PLAYWRIGHT || 'playwright');
const directory = mkdtempSync(join(tmpdir(), 'natsui-nats-login-ui-'));
const children = [];
let browser, output = '';
async function port() {
  const server = createServer().listen(0, '127.0.0.1');
  await once(server, 'listening');
  const result = server.address().port;
  await new Promise(resolve => server.close(resolve));
  return result;
}
function launch(file, args, env) {
  const child = spawn(file, args, { env, windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'] });
  child.stdout.on('data', chunk => output += chunk);
  child.stderr.on('data', chunk => output += chunk);
  children.push(child);
  return child;
}
try {
  const brokerPort = await port(), httpPort = await port();
  const base = 'http://127.0.0.1:' + httpPort;
  const password = 'reader,%40@secret';
  const brokerConfig = join(directory, 'nats.json');
  writeFileSync(brokerConfig, JSON.stringify({ host: '127.0.0.1', port: brokerPort,
    jetstream: { store_dir: join(directory, 'jetstream') },
    authorization: { users: [
      { user: 'collector', password: 'collector-secret' },
      { user: 'reader', password, permissions: { publish: { allow: ['$JS.API.STREAM.LIST'] }, subscribe: { allow: ['_INBOX.>'] } } },
    ] },
  }));
  launch(brokerBinary, ['-c', brokerConfig], process.env);
  const env = Object.fromEntries(Object.entries(process.env).filter(([name]) => !name.startsWith('NATSUI_')));
  Object.assign(env, {
    NATSUI_PORT: String(httpPort), NATSUI_DATA_DIR: join(directory, 'data'),
    NATSUI_URL: `nats://collector:collector-secret@127.0.0.1:${brokerPort}`,
    NATSUI_NATS_LOGIN: '1', NATSUI_ALLOW_WRITES: '1',
  });
  launch(binary, [], env);
  let ready = false;
  for (let attempt = 0; attempt < 100; attempt++) {
    try { if ((await fetch(base + '/readyz')).ok) { ready = true; break; } } catch {}
    await new Promise(resolve => setTimeout(resolve, 100));
  }
  assert.ok(ready, output);
  browser = await chromium.launch({ headless: true, ...(process.env.NATSUI_BROWSER_EXECUTABLE ? { executablePath: process.env.NATSUI_BROWSER_EXECUTABLE } : {}) });
  const context = await browser.newContext();
  const page = await context.newPage();
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  await page.goto(base);
  await page.locator('#nats-username').waitFor({ state: 'visible' });
  assert.equal(await page.locator('#nats-login-form').isVisible(), true);
  assert.equal(await page.locator('#key-login').getAttribute('open'), null);
  assert.match(await page.locator('#nats-sharing').textContent(), /disabled/);
  await page.locator('#nats-username').fill('reader');
  await page.locator('#nats-password').fill('wrong');
  await page.getByRole('button', { name: 'Sign in with NATS', exact: true }).click();
  await page.waitForFunction(() => document.getElementById('login-status').textContent.includes('NATS login failed'));
  assert.equal(await page.locator('#nats-password').inputValue(), '');
  await page.locator('#nats-password').fill(password);
  await page.getByRole('button', { name: 'Sign in with NATS', exact: true }).click();
  await page.waitForURL(base + '/');
  await page.waitForFunction(() => typeof data !== 'undefined' && data?.dashboard?.login_method === 'nats');
  assert.match(await page.locator('#monitor-coverage').textContent(), /not shared/);
  assert.match(await page.locator('#history-chart').textContent(), /not shared/);
  assert.equal(await page.evaluate(() => sessionStorage.getItem('natsui-profile')), 'default');
  assert.equal(await page.evaluate(() => fetch('/api/users').then(response => response.status)), 403);
  await page.setViewportSize({ width: 390, height: 844 });
  await page.locator('#sign-out').click();
  await page.waitForURL(base + '/login');
  await page.locator('#nats-username').waitFor({ state: 'visible' });
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true);
  const screenshots = resolve('data/browser-check');
  mkdirSync(screenshots, { recursive: true });
  await page.screenshot({ path: join(screenshots, 'nats-login-mobile.png'), fullPage: true });
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.screenshot({ path: join(screenshots, 'nats-login.png'), fullPage: true });
  assert.deepEqual(errors, []);
  console.log('NATS login form, rejected password, literal credentials, restricted dashboard, logout and mobile layout passed.');
} finally {
  if (browser) await browser.close();
  for (const child of children.reverse()) {
    if (child.exitCode === null) { const stopped = once(child, 'exit'); child.kill(); await stopped; }
  }
  const cleanup = resolve(directory);
  assert.equal(dirname(cleanup), resolve(tmpdir()));
  assert.ok(basename(cleanup).startsWith('natsui-nats-login-ui-'));
  rmSync(cleanup, { recursive: true, force: true });
}
