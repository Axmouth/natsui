import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {mkdtempSync, mkdirSync, writeFileSync, rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join, resolve, dirname, basename} from 'node:path';
import {once} from 'node:events';
import {createServer} from 'node:net';

const binary = resolve(process.argv[2] || 'target/debug/natsui' + (process.platform === 'win32' ? '.exe' : ''));
assert.ok(process.env.NATSUI_TEST_SERVER && process.env.NATSUI_TLS_FIXTURES, 'Broker and TLS fixtures are required');
const {chromium} = await import(process.env.NATSUI_PLAYWRIGHT || 'playwright');
const directory = mkdtempSync(join(tmpdir(), 'natsui-session-ui-'));
const children = [];
let browser, output = '', connections = 0, trap;
async function port() {
  const server = createServer().listen(0, '127.0.0.1');
  await once(server, 'listening');
  const result = server.address().port;
  await new Promise(resolve => server.close(resolve));
  return result;
}
function launch(file, args, env) {
  const child = spawn(file, args, {env, windowsHide: true, stdio: ['ignore', 'pipe', 'pipe']});
  child.stdout.on('data', chunk => output += chunk);
  child.stderr.on('data', chunk => output += chunk);
  children.push(child);
  return child;
}
async function waitReady(url) {
  for (let attempt = 0; attempt < 100; attempt++) {
    try { const response = await fetch(url); if (response.ok) return response.json(); } catch {}
    await new Promise(resolve => setTimeout(resolve, 100));
  }
  assert.fail(output);
}
try {
  const brokerPort = await port(), httpPort = await port(), monitorPort = await port();
  const base = `http://127.0.0.1:${httpPort}`;
  trap = createServer(socket => { connections++; socket.destroy(); }).listen(brokerPort, '127.0.0.1');
  await once(trap, 'listening');
  const profiles = join(directory, 'profiles.json');
  writeFileSync(profiles, JSON.stringify({profiles:[{id:'second', name:'Second cluster', urls:[`tls://127.0.0.1:${brokerPort}`], ca:join(process.env.NATSUI_TLS_FIXTURES, 'ca.pem'), monitor_urls:[`http://127.0.0.1:${brokerPort}`]}]}));
  const env = Object.fromEntries(Object.entries(process.env).filter(([name]) => !name.startsWith('NATSUI_')));
  Object.assign(env, {
    NATSUI_PORT:String(httpPort), NATSUI_DATA_DIR:join(directory,'data'),
    NATSUI_URL:`tls://127.0.0.1:${brokerPort}`, NATSUI_MONITOR_URLS:`http://127.0.0.1:${brokerPort}`,
    NATSUI_PROFILES_FILE:profiles, NATSUI_TLS_CA:join(process.env.NATSUI_TLS_FIXTURES, 'ca.pem'),
    NATSUI_NATS_LOGIN:'1', NATSUI_NATS_LOGIN_SESSION_ONLY:'1', NATSUI_NATS_LOGIN_CERTIFICATES:'1',
  });
  launch(binary, [], env);
  const ready = await waitReady(base + '/readyz');
  assert.equal(ready.collection, 'on_sign_in');
  await new Promise(resolve => setTimeout(resolve, 1200));
  assert.equal(connections, 0, 'No collector, profile, advisory or HTTP monitoring connection before login');
  await new Promise(resolve => trap.close(resolve)); trap = null;
  const certs = process.env.NATSUI_TLS_FIXTURES;
  const brokerConfig = join(directory, 'broker.json');
  writeFileSync(brokerConfig, JSON.stringify({host:'127.0.0.1', port:brokerPort, http_port:monitorPort,
    jetstream:{store_dir:join(directory,'jetstream')},
    authorization:{users:[{user:'CN=natsui-test', permissions:{publish:{allow:['$JS.API.STREAM.LIST']},subscribe:{allow:['_INBOX.>']}}}]},
    tls:{cert_file:join(certs,'server.pem'),key_file:join(certs,'server.key'),ca_file:join(certs,'ca.pem'),verify_and_map:true},
  }));
  launch(process.env.NATSUI_TEST_SERVER, ['-c',brokerConfig], process.env);
  await waitReady(`http://127.0.0.1:${monitorPort}/varz`);
  browser = await chromium.launch({headless:true, ...(process.env.NATSUI_BROWSER_EXECUTABLE ? {executablePath:process.env.NATSUI_BROWSER_EXECUTABLE} : {})});
  const context = await browser.newContext();
  const page = await context.newPage();
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  await page.goto(base);
  await page.locator('#nats-method-label').waitFor({state:'visible'});
  assert.match(await page.locator('#nats-sharing').textContent(), /Session history/);
  await page.locator('#nats-method').selectOption('certificate');
  assert.equal(await page.locator('#nats-password').isDisabled(), true);
  const {readFileSync} = await import('node:fs');
  await page.locator('#nats-certificate').setInputFiles({name:'client.crt',mimeType:'application/x-x509-ca-cert',buffer:readFileSync(join(certs,'client.pem'))});
  await page.locator('#nats-private-key').setInputFiles(join(certs,'server.key'));
  await page.getByRole('button',{name:'Sign in with NATS',exact:true}).click();
  await page.waitForFunction(() => document.getElementById('login-status').textContent.includes('mismatched'));
  assert.equal(await page.locator('#nats-private-key').inputValue(), '');
  await page.locator('#nats-certificate').setInputFiles({name:'client.crt',mimeType:'application/x-x509-ca-cert',buffer:readFileSync(join(certs,'client.pem'))});
  await page.locator('#nats-private-key').setInputFiles(join(certs,'client.key'));
  await page.getByRole('button',{name:'Sign in with NATS',exact:true}).click();
  await page.waitForURL(base + '/');
  await page.waitForFunction(() => typeof data !== 'undefined' && data?.dashboard?.session_history);
  const history = await context.request.get(base + '/api/history');
  assert.equal(history.status(),200);
  assert.ok((await history.json()).length >= 1);
  assert.equal((await context.request.get(base + '/api/monitoring/routes')).status(),403);
  assert.equal((await context.request.get(base + '/api/snapshot',{headers:{'x-natsui-profile':'second'}})).status(),403);
  await page.goto(base + '/#settings');
  await page.getByRole('heading',{name:'Session certificate',exact:true}).waitFor({state:'visible'});
  assert.match(await page.locator('#page-settings').textContent(), /CN=natsui-test/);
  assert.match(await page.locator('#page-settings').textContent(), /SHA-256/);
  await page.setViewportSize({width:390,height:844});
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth),true);
  await page.setViewportSize({width:1280,height:900});
  await page.goto(base + '/#nodes');
  await page.getByRole('button',{name:'Enable monitoring through NATS',exact:true}).click();
  await page.waitForFunction(() => data?.monitoring?.source==='session_nats');
  assert.equal(await page.evaluate(() => data.monitoring.status),'unavailable');
  assert.match(await page.locator('#monitor-coverage').textContent(), /0 NATS responders/);
  await page.getByRole('button',{name:'Stop NATS monitoring',exact:true}).click();
  await page.goto(base + '/#activity');
  await page.getByRole('heading',{name:'Failure events',exact:true}).waitFor({state:'visible'});
  assert.equal(await page.locator('#failure-watch-start').isDisabled(),true);
  assert.match(await page.locator('#page-description').textContent(), /this login only/);
  await page.setViewportSize({width:390,height:844});
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth),true);
  await page.setViewportSize({width:1280,height:900});
  const firstCookie = (await context.cookies()).find(cookie => cookie.httpOnly);
  await page.locator('#sign-out').click();
  await page.waitForURL(base + '/login');
  await page.locator('#nats-method-label').waitFor({state:'visible'});
  // Same certificate, new session: no observations before the first snapshot request.
  const response = await context.request.post(base + '/api/auth/nats', {headers:{'x-natsui-request':'1'}, data:{profile:'default',certificate:readFileSync(join(certs,'client.pem')).toString('base64'),private_key:readFileSync(join(certs,'client.key'),'utf8')}});
  assert.equal(response.status(),204);
  assert.deepEqual(await (await context.request.get(base + '/api/history')).json(),[]);
  assert.equal((await context.request.get(base + '/api/history',{headers:{cookie:`${firstCookie.name}=${firstCookie.value}`}})).status(),401);
  await context.request.post(base + '/api/auth/logout',{headers:{'x-natsui-request':'1'},data:{}});
  const pem = readFileSync(join(certs, 'client.pem'), 'utf8');
  const der = Buffer.from(pem.split('-----BEGIN CERTIFICATE-----')[1].split('-----END CERTIFICATE-----')[0].replace(/\s/g, ''), 'base64');
  await page.locator('#nats-method').selectOption('certificate');
  await page.locator('#nats-certificate').setInputFiles({name:'client.crt',mimeType:'application/x-x509-ca-cert',buffer:der});
  await page.locator('#nats-private-key').setInputFiles(join(certs,'client.key'));
  await page.getByRole('button',{name:'Sign in with NATS',exact:true}).click();
  await page.waitForURL(base + '/');
  await page.waitForFunction(() => typeof data !== 'undefined' && data?.dashboard?.session_history);
  await page.locator('#sign-out').click();
  await page.waitForURL(base + '/login');
  await page.locator('#nats-method-label').waitFor({state:'visible'});
  await new Promise(resolve => setTimeout(resolve, 300));
  const before = await (await fetch(`http://127.0.0.1:${monitorPort}/varz`)).json();
  await new Promise(resolve => setTimeout(resolve, 1200));
  const after = await (await fetch(`http://127.0.0.1:${monitorPort}/varz`)).json();
  assert.equal(after.total_connections,before.total_connections,'No collection after logout');
  assert.equal(after.connections,0,'No lingering session NATS connections');
  await page.locator('#nats-method').selectOption('certificate');
  await page.setViewportSize({width:390,height:844});
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth),true);
  const screenshots = resolve('data/browser-check'); mkdirSync(screenshots,{recursive:true});
  await page.screenshot({path:join(screenshots,'certificate-login-mobile.png'),fullPage:true});
  await page.setViewportSize({width:1280,height:900});
  await page.screenshot({path:join(screenshots,'certificate-login.png'),fullPage:true});
  assert.deepEqual(errors,[]);
  console.log('Collector-free startup and logout, certificate upload, private session history, profile boundary and mobile layout passed.');
} finally {
  if(browser) await browser.close();
  if(trap) await new Promise(resolve => trap.close(resolve));
  for(const child of children.reverse()) {
    if(child.exitCode===null) { const stopped=once(child,'exit'); child.kill(); await stopped; }
  }
  const cleanup=resolve(directory);
  assert.equal(dirname(cleanup),resolve(tmpdir()));
  assert.ok(basename(cleanup).startsWith('natsui-session-ui-'));
  rmSync(cleanup,{recursive:true,force:true});
}
