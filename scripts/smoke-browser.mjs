import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdirSync } from 'node:fs';
import { resolve, join } from 'node:path';

const { chromium } = await import(
  process.env.NATSUI_PLAYWRIGHT || 'playwright'
);
const binary = process.argv[2] ? resolve(process.argv[2]) : null;
const base = process.env.NATSUI_BROWSER_URL || 'http://127.0.0.1:4337';
assert.equal(new URL(base).hostname, '127.0.0.1');
assert.equal(
  process.env.NATSUI_BROWSER_TEST_WRITES,
  '1',
  'A disposable write-enabled cluster is required',
);
const output = resolve(
  process.env.NATSUI_BROWSER_ARTIFACTS || 'data/browser-check',
);
mkdirSync(output, { recursive: true });
const loginCommand = process.env.NATSUI_BROWSER_CONTAINER
  ? [
      'docker',
      ['exec', process.env.NATSUI_BROWSER_CONTAINER, 'natsui', 'login'],
    ]
  : [binary, ['login']];
const issued = execFileSync(...loginCommand, {
  encoding: 'utf8',
  windowsHide: true,
}).match(/http[^\s]+\/login#ticket=[a-f0-9]{64}/)?.[0];
const link = issued ? base + '/login' + new URL(issued).hash : null;
assert.ok(link, 'CLI should issue a one-time login link');
const browser = await chromium.launch({
  headless: true,
  ...(process.env.NATSUI_BROWSER_EXECUTABLE
    ? { executablePath: process.env.NATSUI_BROWSER_EXECUTABLE }
    : {}),
});
const context = await browser.newContext({
  viewport: { width: 1440, height: 1050 },
});
const page = await context.newPage();
const errors = [];
context.on('page', (page) =>
  page.on('pageerror', (error) => errors.push(error.message)),
);
page.on('pageerror', (error) => errors.push(error.message));
const stream = 'BROWSER_' + Date.now();
const user = 'browser_' + Date.now();
let userCreated = false,
  streamCreated = false;
const request = async (path, body, profile = 'default', method = 'POST') => {
  const response = await context.request.fetch(base + path, {
    method,
    headers: {
      'X-Natsui-Request': '1',
      'X-Natsui-Profile': profile,
      Origin: base,
    },
    ...(body ? { data: body } : {}),
  });
  assert.ok(
    response.ok(),
    `Test API request failed: ${path} (${response.status()})`,
  );
  return response.status() === 204 ? null : response.json();
};
async function operation(action, configure, confirmation) {
  await page.locator('#operation-action').selectOption(action);
  await configure();
  await page.locator('#operation-preview').click();
  await page.locator('#operation-review').waitFor({ state: 'visible' });
  await page.locator('#operation-confirm').fill(confirmation);
  const applied = page.waitForResponse((response) =>
    response.url().endsWith('/api/operations/apply'),
  );
  await page.locator('#operation-apply').click();
  const response = await applied;
  assert.equal(response.status(), 200);
  return response.json();
}
try {
  await page.goto(link);
  await page.waitForURL(base + '/');
  assert.ok(!page.url().includes('ticket='));
  await page.locator('.profile-select').waitFor({ state: 'visible' });
  const second = await context.newPage();
  await second.goto(base + '/#settings');
  await second.locator('.profile-select').selectOption('staging');
  await second.waitForURL(base + '/#overview');
  await second.waitForFunction(() => document.querySelector('.profile-select')?.value === 'staging');
  assert.equal(await second.locator('.profile-select').inputValue(), 'staging');
  await page.reload();
  await page.waitForFunction(() => document.querySelector('.profile-select')?.value === 'default');
  assert.equal(await page.locator('.profile-select').inputValue(), 'default');
  const initial = await request('/api/settings', undefined, 'default', 'GET');
  const staging = await request('/api/settings', undefined, 'staging', 'GET');
  await request(
    '/api/settings',
    { ...staging, backlog_threshold: 23456 },
    'staging',
    'PUT',
  );
  assert.deepEqual(
    await request('/api/settings', undefined, 'default', 'GET'),
    initial,
  );
  assert.equal(
    (await request('/api/settings', undefined, 'staging', 'GET'))
      .backlog_threshold,
    23456,
  );
  await request('/api/settings', staging, 'staging', 'PUT');

  await page.goto(base + '/#operations');
  await page.locator('#operation-preview').waitFor({ state: 'visible' });
  await page.waitForFunction(
    () => !document.getElementById('operation-preview').disabled,
  );
  const created = await operation(
    'create_stream',
    async () => {
      await page.locator('#operation-stream').fill(stream);
      await page
        .locator('#operation-subjects')
        .fill(stream.toLowerCase() + '.>');
    },
    stream,
  );
  assert.ok(created.verified);
  streamCreated = true;
  const published = await operation(
    'publish',
    async () => {
      await page.locator('#operation-stream').fill(stream);
      await page
        .locator('#operation-subject')
        .fill(stream.toLowerCase() + '.test');
      await page
        .locator('#operation-payload')
        .fill('{"example":"browser check"}');
    },
    stream.toLowerCase() + '.test',
  );
  assert.ok(published.storage_verified);
  await page.screenshot({
    path: join(output, 'operations.png'),
    fullPage: true,
  });
  await operation(
    'delete_stream',
    async () => {
      await page.locator('#operation-stream').fill(stream);
    },
    stream,
  );
  streamCreated = false;

  await page.goto(base + '/#settings');
  await page.locator('#user-name').fill(user);
  await page.locator('#user-role').selectOption('viewer');
  await page.locator('#user-create button').click();
  await page.locator('#user-key-dialog').waitFor({ state: 'visible' });
  userCreated = true;
  const key = await page.locator('#user-key-value').inputValue();
  assert.match(key, /^[a-f0-9]{64}$/);
  await page.locator('#user-key-close').click();
  await page.waitForFunction(
    () => document.getElementById('user-key-value').value === '',
  );
  const viewer = await browser.newContext();
  const login = await viewer.request.post(base + '/api/auth/login', {
    headers: { 'X-Natsui-Request': '1', Origin: base },
    data: { key },
  });
  assert.equal(login.status(), 204);
  const viewerPage = await viewer.newPage();
  await viewerPage.goto(base + '/#operations');
  await viewerPage
    .locator('#operation-access')
    .filter({ hasText: 'disabled' })
    .waitFor();
  assert.ok(await viewerPage.locator('#operation-preview').isDisabled());
  assert.equal(
    (
      await viewer.request.post(base + '/api/operations/preview', {
        headers: { 'X-Natsui-Request': '1', Origin: base },
        data: {},
      })
    ).status(),
    403,
  );
  await viewer.close();
  await page.screenshot({ path: join(output, 'settings.png'), fullPage: true });

  const recovery = await context.newPage();
  await recovery.addInitScript(() => {
    if (!sessionStorage.getItem('natsui-profile'))
      sessionStorage.setItem('natsui-profile', 'removed');
  });
  await recovery.route('**/api/profiles', (route) =>
    route.fulfill({
      json: {
        selected: route.request().headers()['x-natsui-profile'],
        profiles: [{ id: 'default', name: 'Default' }],
      },
    }),
  );
  await recovery.goto(base);
  await recovery.locator('.profile-select').waitFor({ state: 'visible' });
  assert.equal(await recovery.locator('.profile-select option').count(), 1);
  const selectedRequest = recovery.waitForRequest((request) =>
    request.url().endsWith('/api/profiles/select'),
  );
  await recovery.locator('.profile-select').selectOption('default');
  assert.ok(await selectedRequest);
  await recovery.waitForFunction(
    () =>
      document.querySelector('.profile-select')?.value === 'default' &&
      document.querySelector('.profile-select')?.hidden,
  );
  await recovery.close();
  assert.deepEqual(errors, []);
  console.log(
    'Browser login link, two-tab profiles, isolated settings, native create/publish/delete, named viewer restrictions and removed-profile recovery passed.',
  );
} finally {
  if (streamCreated) {
    try {
      const preview = await request('/api/operations/preview', {
        action: 'delete_stream',
        stream,
      });
      await request('/api/operations/apply', {
        token: preview.token,
        confirmation: stream,
      });
    } catch {}
  }
  if (userCreated) {
    try {
      const users = await request('/api/users', undefined, 'default', 'GET');
      const row = users.users.find((row) => row.id === user);
      if (row)
        await request('/api/users/' + user, {
          revision: row.revision,
          action: 'delete',
        });
    } catch {}
  }
  await browser.close();
}
