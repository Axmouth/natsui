import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdtempSync, rmSync, mkdirSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { once } from 'node:events';
import { createServer } from 'node:net';
const binary = resolve(
  process.argv[2] ||
    'target/debug/natsui' + (process.platform === 'win32' ? '.exe' : ''),
);
const dir = mkdtempSync(join(tmpdir(), 'natsui-workspace-'));
const listener = createServer();
listener.listen(0, '127.0.0.1');
await once(listener, 'listening');
const port = listener.address().port;
await new Promise((r) => listener.close(r));
const base = 'http://127.0.0.1:' + port;
let child,
  browser,
  output = '';
const streams = [];
const delay = (ms) => new Promise((r) => setTimeout(r, ms));
try {
  child = spawn(binary, ['--demo'], {
    env: {
      ...process.env,
      NATSUI_PORT: String(port),
      NATSUI_DATA_DIR: dir,
      NATSUI_AUTH_TOKEN_FILE: undefined,
      NATSUI_PUBLIC_URL: undefined,
      NATSUI_OIDC_CONFIG_FILE: undefined,
      NATSUI_ACCESS_POLICY_FILE: undefined,
      NATSUI_PROFILES_FILE: undefined,
      NATSUI_MANAGED_CONFIG_FILE: undefined,
    },
    windowsHide: true,
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  child.stdout.on('data', (b) => (output += b));
  child.stderr.on('data', (b) => (output += b));
  let ready = false;
  for (let i = 0; i < 100; i++) {
    try {
      if ((await fetch(base + '/readyz')).ok) {
        ready = true;
        break;
      }
    } catch {}
    await delay(100);
  }
  assert.ok(ready, output);
  for (let i = 0; i < 128; i++) {
    const controller = new AbortController();
    const response = await fetch(base + '/api/events', {
      signal: controller.signal,
    });
    assert.equal(response.status, 200);
    await response.body.getReader().read();
    streams.push(controller);
  }
  assert.equal((await fetch(base + '/api/events')).status, 429);
  for (const controller of streams) controller.abort();
  await delay(100);
  const reusable = new AbortController();
  const after = await fetch(base + '/api/events', {
    signal: reusable.signal,
  });
  assert.equal(after.status, 200);
  await after.body.getReader().read();
  reusable.abort();
  console.log(
    'SSE capacity is bounded and disconnected clients release permits.',
  );
  if (process.env.NATSUI_WORKSPACE_BROWSER === '1') {
    const { chromium } = await import(
      process.env.NATSUI_PLAYWRIGHT || 'playwright'
    );
    browser = await chromium.launch({
      headless: true,
      ...(process.env.NATSUI_BROWSER_EXECUTABLE
        ? {
            executablePath: process.env.NATSUI_BROWSER_EXECUTABLE,
          }
        : {}),
    });
    const context = await browser.newContext();
    await context.addInitScript(() => {
      globalThis.delivered = [];
      globalThis.Notification = class {
        static permission = 'granted';
        static async requestPermission() {
          return 'granted';
        }
        constructor(title, options) {
          delivered.push({ title, ...options });
        }
        close() {}
      };
    });
    const page = await context.newPage(),
      errors = [];
    page.on('pageerror', (e) => errors.push(e.message));
    await page.goto(base);
    await page.waitForFunction(
      () => typeof data !== 'undefined' && data && history.length > 0,
    );
    await page.locator('#backlog-window').selectOption('21600');
    await page.waitForFunction(
      () =>
        backlog.window === 21600 ||
        Number(document.querySelector('#backlog-window').value) === 21600,
    );
    await page.goto(base + '/#replicas');
    await page
      .getByRole('heading', {
        name: 'Cluster routes',
        exact: true,
      })
      .waitFor();
    await page.goto(base + '/#investigate');
    await page.evaluate(() =>
      loadWindow(
        Math.floor(Date.now() / 1000) - 300,
        Math.floor(Date.now() / 1000),
      ),
    );
    await page.getByLabel('Saved investigation name').fill('Recent incident');
    await page
      .getByRole('button', {
        name: 'Save current view',
        exact: true,
      })
      .click();
    await page.reload();
    await page
      .getByRole('button', {
        name: 'Recent incident',
        exact: true,
      })
      .click();
    await page
      .getByText(
        'Loaded Recent incident. Retention may have removed older observations.',
      )
      .waitFor();
    // A delayed older route must not select resources on a newer saved window.
    const race = await page.evaluate(async () => {
      const original = api;
      let release;
      const gate = new Promise((r) => (release = r));
      let calls = 0;
      api = async (path, ...rest) => {
        if (path.startsWith('/api/history/window') && calls++ === 0) await gate;
        return original(path, ...rest);
      };
      const now = Math.floor(Date.now() / 1000);
      const older = loadWindow(now - 300, now);
      const newer = loadWindow(now - 60, now);
      const won = await newer;
      release();
      const lost = await older;
      api = original;
      return {
        won,
        lost,
        from: investigation.range.from,
        expected: now - 60,
      };
    });
    assert.equal(race.won, true);
    assert.equal(race.lost, false);
    assert.equal(race.from, race.expected);
    const refreshes = await page.evaluate(async () => {
      while (busy) await new Promise((r) => setTimeout(r, 10));
      const original = api;
      let release,
        calls = 0;
      const gate = new Promise((r) => (release = r));
      api = async (path, ...rest) => {
        if (path === '/api/snapshot' && ++calls === 1) await gate;
        return original(path, ...rest);
      };
      const first = refresh();
      await Promise.resolve();
      refresh();
      release();
      await first;
      while (busy || refreshPending)
        await new Promise((r) => setTimeout(r, 10));
      api = original;
      return calls;
    });
    assert.ok(refreshes >= 2);
    await page.evaluate(() => {
      localStorage.removeItem('natsui-notification-seen-live-default');
      localStorage.setItem('natsui-notifications-live-default', 'true');
      acceptIncidents({ status: 'fulfilled', value: [] });
    });
    await delay(100);
    const event = {
      id_local: 1,
      at: Math.floor(Date.now() / 1000),
      kind: 'backlog-high',
      stream: 'ORDERS',
      name: 'worker',
      detail: 'Threshold exceeded',
    };
    await delay(1100);
    const other = await context.newPage();
    await other.route('**/api/incidents', (route) =>
      route.fulfill({ json: [event] }),
    );
    await other.goto(base);
    await other.waitForFunction(() => typeof data !== 'undefined' && data);
    await delay(100);
    await page.evaluate(
      (event) =>
        acceptIncidents({
          status: 'fulfilled',
          value: [event],
        }),
      event,
    );
    await page.waitForFunction(() => delivered.length === 1);
    assert.equal(await other.evaluate(() => delivered.length), 0);
    await page.evaluate(
      (event) =>
        acceptIncidents({
          status: 'fulfilled',
          value: [event],
        }),
      event,
    );
    await delay(100);
    assert.equal(await page.evaluate(() => delivered.length), 1);
    await other.close();
    await page
      .getByRole('button', {
        name: 'Remove saved investigation Recent incident',
      })
      .click();
    assert.equal(
      await page
        .getByRole('button', {
          name: 'Recent incident',
          exact: true,
        })
        .count(),
      0,
    );
    assert.deepEqual(errors, []);
    mkdirSync('data/browser-check', { recursive: true });
    await page.screenshot({
      path: 'data/browser-check/workspace.png',
      fullPage: true,
    });
    console.log(
      'Browser saved views, superseded windows, coalesced refresh and cross-tab notification delivery passed.',
    );
  }
} finally {
  for (const controller of streams) controller.abort();
  if (browser) await browser.close();
  if (child && child.exitCode === null) {
    child.kill();
    await once(child, 'exit');
  }
  rmSync(dir, { recursive: true, force: true });
}
