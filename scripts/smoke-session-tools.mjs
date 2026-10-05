import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {mkdtempSync,writeFileSync,mkdirSync,rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join,resolve,dirname,basename} from 'node:path';
import {once} from 'node:events';
import {createServer} from 'node:net';

const binary=resolve(process.argv[2] || 'target/debug/natsui'+(process.platform==='win32'?'.exe':''));
assert.ok(process.env.NATSUI_TEST_SERVER,'NATSUI_TEST_SERVER is required');
const {chromium}=await import(process.env.NATSUI_PLAYWRIGHT || 'playwright');
const directory=mkdtempSync(join(tmpdir(),'natsui-tools-browser-'));
const children=[];
let browser,logs='';
async function port(){const server=createServer().listen(0,'127.0.0.1');await once(server,'listening');const p=server.address().port;await new Promise(resolve=>server.close(resolve));return p;}
function launch(file,args,env){const child=spawn(file,args,{env,windowsHide:true,stdio:['ignore','pipe','pipe']});children.push(child);child.stdout.on('data',d=>logs+=d);child.stderr.on('data',d=>logs+=d);return child;}
async function ready(url){for(let i=0;i<100;i++){try{if((await fetch(url)).ok)return;}catch{}await new Promise(r=>setTimeout(r,100));}assert.fail(logs);}
try {
  const [natsPort,httpPort,monitorPort]=await Promise.all([port(),port(),port()]);
  const base=`http://127.0.0.1:${httpPort}`;
  const config=join(directory,'nats.json');
  writeFileSync(config,JSON.stringify({host:'127.0.0.1',port:natsPort,http_port:monitorPort,server_name:'browser-node',system_account:'SYS',jetstream:{store_dir:join(directory,'js')},accounts:{SYS:{users:[{user:'system',password:'secret'}]},APP:{jetstream:'enabled',users:[{user:'writer',password:'secret'}]}}}));
  launch(process.env.NATSUI_TEST_SERVER,['-c',config],process.env);
  await ready(`http://127.0.0.1:${monitorPort}/varz`);
  const env=Object.fromEntries(Object.entries(process.env).filter(([name])=>!name.startsWith('NATSUI_')));
  Object.assign(env,{NATSUI_PORT:String(httpPort),NATSUI_URL:`nats://127.0.0.1:${natsPort}`,NATSUI_DATA_DIR:join(directory,'data'),NATSUI_NATS_LOGIN:'1',NATSUI_NATS_LOGIN_SESSION_ONLY:'1',NATSUI_ALLOW_WRITES:'1'});
  launch(binary,[],env);await ready(base+'/readyz');
  browser=await chromium.launch({headless:true,...(process.env.NATSUI_BROWSER_EXECUTABLE?{executablePath:process.env.NATSUI_BROWSER_EXECUTABLE}:{})});
  const context=await browser.newContext({timezoneId:'UTC',viewport:{width:1440,height:1000}});
  const page=await context.newPage();const errors=[];page.on('pageerror',e=>errors.push(e.message));
  async function login(user){await page.goto(base);await page.locator('#nats-username').fill(user);await page.locator('#nats-password').fill('secret');await page.getByRole('button',{name:'Sign in with NATS',exact:true}).click();await page.waitForURL(base+'/');await page.waitForFunction(()=>data?.dashboard?.session_history);}
  await login('writer');
  async function operation(action,fields={}){
    await page.goto(base+'/#operations');await page.locator('#operation-action').selectOption(action);
    await page.locator('#operation-stream').fill('JOBS');
    for(const [id,value]of Object.entries(fields))await page.locator('#operation-'+id).fill(value);
    await page.locator('#operation-preview').click();await page.locator('#operation-review').waitFor({state:'visible'});
    const confirmation=(await page.locator('#operation-confirm-name').textContent()).replace('Confirmation: ','');
    await page.locator('#operation-confirm').fill(confirmation);await page.locator('#operation-apply').click();
    await page.waitForFunction(()=>/applied and verified|Stored in/.test(document.getElementById('operation-status').textContent));
  }
  await operation('create_stream',{subjects:'jobs',max:'100'});
  await operation('create_consumer',{consumer:'worker',filter:'jobs'});
  await operation('publish',{subject:'jobs',payload:'{"task":"browser-fixture"}'});
  await operation('pause_consumer',{consumer:'worker','pause-until':new Date(Date.now()+600_000).toISOString().slice(0,16)});
  await page.goto(base+'/#consumers');
  await page.waitForFunction(()=>document.querySelector('#page-consumers').textContent.includes('Delivery is paused'));
  const screenshots=resolve('data/browser-check');mkdirSync(screenshots,{recursive:true});
  await page.screenshot({path:join(screenshots,'session-consumer-progress.png'),fullPage:true});
  await operation('resume_consumer',{consumer:'worker'});
  await page.goto(base+'/#streams');
  await page.getByRole('heading',{name:'Retention and capacity',exact:true}).waitFor({state:'visible'});
  await page.waitForFunction(()=>document.querySelector('#page-streams').textContent.includes('99'));
  await page.screenshot({path:join(screenshots,'session-retention.png'),fullPage:true});
  await page.goto(base+'/#activity');
  await page.locator('#failure-watch-stream').selectOption('JOBS');
  await page.locator('#failure-watch-start').click();
  await page.waitForFunction(()=>document.getElementById('failure-watch-status').textContent.includes('0 events received'));
  assert.match(await page.locator('#page-activity').textContent(),/pause_consumer/);
  await page.locator('#failure-watch-start').click();
  assert.match(await page.locator('#failure-watch-status').textContent(),/in-flight/);
  await page.setViewportSize({width:390,height:844});
  for(const path of ['streams','consumers','activity']){
    await page.goto(base+'/#'+path);
    assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),true,path+' mobile overflow');
  }
  await page.locator('#sign-out').click();await page.waitForURL(base+'/login');
  await page.setViewportSize({width:1440,height:1000});
  await login('system');await page.goto(base+'/#nodes');
  await page.getByRole('button',{name:'Enable monitoring through NATS',exact:true}).click();
  await page.waitForFunction(()=>data?.monitoring?.nodes?.some(n=>n.server_name==='browser-node'));
  assert.match(await page.locator('#monitor-coverage').textContent(),/1 NATS responder/);
  await page.screenshot({path:join(screenshots,'session-native-monitoring.png'),fullPage:true});
  await page.goto(base+'/#connections');
  await page.waitForFunction(()=>document.getElementById('connections-table')?.querySelector('tbody tr'));
  assert.deepEqual(errors,[]);
  console.log('Session consumer diagnosis, retention, reviewed pause/resume, activity watch, native monitoring and mobile layout passed.');
} finally {
  if(browser)await browser.close();
  for(const child of children.reverse())if(child.exitCode===null){const stopped=once(child,'exit');child.kill();await stopped;}
  const cleanup=resolve(directory);assert.equal(dirname(cleanup),resolve(tmpdir()));assert.ok(basename(cleanup).startsWith('natsui-tools-browser-'));rmSync(cleanup,{recursive:true,force:true});
}
