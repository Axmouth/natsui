import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {mkdtempSync,writeFileSync,readFileSync,readdirSync,existsSync,rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join,resolve,dirname,basename} from 'node:path';
import {createServer} from 'node:net';
import {once} from 'node:events';

const binary=resolve(process.argv[2] || 'target/debug/natsui'+(process.platform==='win32'?'.exe':''));
const directory=mkdtempSync(join(tmpdir(),'natsui-doctor-'));
const baseline=Object.fromEntries(Object.entries(process.env).filter(([name])=>!name.startsWith('NATSUI_')));
const runtime={...baseline,NATSUI_NATS_LOGIN:'1',NATSUI_NATS_LOGIN_SESSION_ONLY:'1',NATSUI_DATA_DIR:join(directory,'must-not-be-created')};
const children=[];let trap;let hits=0;
async function port(){const server=createServer().listen(0,'127.0.0.1');await once(server,'listening');const result=server.address().port;await new Promise(r=>server.close(r));return result;}
function start(file,args,env){const child=spawn(file,args,{env,windowsHide:true,stdio:['ignore','pipe','pipe']});children.push(child);return child;}
async function doctor(args=[],extra={}){
  const child=start(binary,['doctor','--json',...args],{...runtime,...extra});let stdout='',stderr='';
  child.stdout.on('data',b=>stdout+=b);child.stderr.on('data',b=>stderr+=b);
  const timer=setTimeout(()=>child.kill(),35000);
  const [code]=await once(child,'exit');clearTimeout(timer);
  assert.ok(!stdout.includes('DOCTOR_SECRET')&&!stderr.includes('DOCTOR_SECRET'),'No secret output');
  return {code,report:code===2?null:JSON.parse(stdout),stdout,stderr};
}
const check=(report,id)=>{const item=report.checks.find(c=>c.id===id);assert.ok(item,'Missing '+id);return item;};
async function ready(url){for(let i=0;i<60;i++){try{if((await fetch(url)).ok)return;}catch{}await new Promise(r=>setTimeout(r,100));}assert.fail('Broker startup');}
try {
  trap=createServer(socket=>{hits++;socket.destroy();}).listen(0,'127.0.0.1');await once(trap,'listening');
  runtime.NATSUI_URL=`nats://127.0.0.1:${trap.address().port}`;
  let result=await doctor(['--require-session-only']);assert.equal(result.code,0,result.stdout);assert.equal(result.report.mode,'session_only');assert.equal(hits,0);assert.equal(existsSync(runtime.NATSUI_DATA_DIR),false);
  result=await doctor([],{NATSUI_URL:`nats://user:DOCTOR_SECRET@127.0.0.1:${trap.address().port}`});assert.equal(result.code,1);assert.equal(check(result.report,'profile.default.session_identity').status,'fail');assert.equal(hits,0);
  result=await doctor([],{NATSUI_NATS_LOGIN_SHARED_HISTORY:'1'});assert.equal(result.code,1);assert.match(check(result.report,'login.policy').guidance,/SHARED_HISTORY=0/);
  result=await doctor(['--require-session-only'],{NATSUI_NATS_LOGIN_SESSION_ONLY:'0'});assert.equal(result.code,1);assert.equal(check(result.report,'login.session_only').status,'fail');
  result=await doctor([],{NATSUI_NATS_LOGIN:'0',NATSUI_NATS_LOGIN_SESSION_ONLY:'0'});assert.equal(result.code,0);assert.equal(result.report.mode,'dashboard_identity_or_local');
  result=await doctor([],{NATSUI_TLS_CA:join(directory,'missing-ca'),NATSUI_PORT:'abc'});assert.equal(result.code,1);assert.equal(check(result.report,'profile.default.connection').status,'fail');assert.match(check(result.report,'profile.default.connection').detail,/NATSUI_TLS_CA is not readable/);assert.equal(check(result.report,'deployment.port').status,'fail');
  result=await doctor([],{NATSUI_NATS_LOGIN_SESSION_ONLY:'0',NATSUI_MONITOR_URLS:'http://127.0.0.1:8222',NATSUI_MONITOR_CONFIG_FILE:'missing'});assert.equal(result.code,1);assert.equal(check(result.report,'profile.default.http_monitoring').status,'fail');
  result=await doctor([],{NATSUI_MONITOR_URLS:'invalid',NATSUI_MONITOR_CONFIG_FILE:'missing'});assert.equal(result.code,0,'Session-only ignores HTTP monitoring settings');
  const invalidData=join(directory,'not-a-directory');writeFileSync(invalidData,'fixture');
  result=await doctor([],{NATSUI_DATA_DIR:invalidData});assert.equal(result.code,1);assert.equal(check(result.report,'storage.persistence').status,'fail');
  assert.equal(readFileSync(invalidData,'utf8'),'fixture');
  const profileFile=join(directory,'profiles.json');writeFileSync(profileFile,JSON.stringify({profiles:[{id:'second',name:'Second',urls:[runtime.NATSUI_URL],allow_writes:true}]}));
  result=await doctor([],{NATSUI_PROFILES_FILE:profileFile});assert.equal(result.code,0);assert.equal(check(result.report,'profile.second.operations').status,'unknown');assert.equal(check(result.report,'profile.default.operations').status,'disabled');
  writeFileSync(profileFile,JSON.stringify({profiles:[{id:'broken',name:'Broken',urls:[runtime.NATSUI_URL],key:join(directory,'missing.key')}]}));
  result=await doctor([],{NATSUI_PROFILES_FILE:profileFile,NATSUI_NATS_LOGIN_SESSION_ONLY:'0'});assert.equal(result.code,1);assert.equal(check(result.report,'profile.broken.configuration').status,'fail');
  assert.equal(check(result.report,'profile.broken.tls_files').status,'unknown');
  const largeCA=join(directory,'oversized-ca');writeFileSync(largeCA,Buffer.alloc(1_048_577));
  writeFileSync(profileFile,JSON.stringify({profiles:[{id:'broken',name:'Broken',urls:[runtime.NATSUI_URL],ca:largeCA}]}));
  result=await doctor(['--probe','--profile','broken','--username','reader','--password-file','must-not-be-read'],{NATSUI_PROFILES_FILE:profileFile});assert.equal(result.code,1);assert.equal(check(result.report,'profile.broken.tls_files').status,'unknown');assert.equal(check(result.report,'probe.configuration').status,'fail');
  writeFileSync(profileFile,JSON.stringify({profiles:[{id:'broken',name:'Broken',urls:[runtime.NATSUI_URL],monitor_urls:['http://127.0.0.1:8222'],monitor_config_file:'missing'}]}));
  result=await doctor([],{NATSUI_PROFILES_FILE:profileFile,NATSUI_NATS_LOGIN_SESSION_ONLY:'0'});assert.equal(result.code,1);assert.equal(check(result.report,'profile.broken.http_monitoring').status,'fail');
  writeFileSync(profileFile,JSON.stringify({profiles:[{id:'default',name:'Duplicate',urls:[runtime.NATSUI_URL]}]}));
  result=await doctor([],{NATSUI_PROFILES_FILE:profileFile});assert.equal(result.code,1);assert.equal(check(result.report,'profiles.file').status,'fail');
  for(const args of [['--probe'],['--password','DOCTOR_SECRET'],['--probe','--username','u','--password-file','x','--stream','>']])assert.equal((await doctor(args)).code,2);
  assert.equal(hits,0);await new Promise(r=>trap.close(r));trap=null;
  assert.ok(process.env.NATSUI_TEST_SERVER,'NATSUI_TEST_SERVER required for real broker checks');
  const [brokerPort,monitorPort]=await Promise.all([port(),port()]);
  const config=join(directory,'nats.json');writeFileSync(config,JSON.stringify({host:'127.0.0.1',port:brokerPort,http_port:monitorPort,system_account:'SYS',jetstream:{store_dir:join(directory,'js')},accounts:{SYS:{users:[{user:'system',password:'DOCTOR_SECRET'}]},APP:{jetstream:'enabled',users:[{user:'reader',password:'DOCTOR_SECRET',permissions:{publish:{allow:['$JS.API.STREAM.LIST']},subscribe:{allow:['_INBOX.>']}}}]}}}));
  const broker=start(process.env.NATSUI_TEST_SERVER,['-c',config],baseline);broker.stdout.resume();broker.stderr.resume();await ready(`http://127.0.0.1:${monitorPort}/varz`);
  runtime.NATSUI_URL=`nats://127.0.0.1:${brokerPort}`;const password=join(directory,'password');writeFileSync(password,'DOCTOR_SECRET\n');
  const before=readFileSync(config,'utf8');
  result=await doctor(['--probe','--username','reader','--password-file',password]);assert.equal(result.code,0,result.stdout);assert.equal(check(result.report,'probe.authentication').status,'pass');assert.equal(check(result.report,'probe.stream_list').status,'pass');assert.equal(check(result.report,'probe.system_monitoring').status,'unknown');
  result=await doctor(['--probe','--username','system','--password-file',password]);assert.equal(result.code,0,result.stdout);assert.equal(check(result.report,'probe.system_monitoring').status,'pass');assert.equal(check(result.report,'probe.stream_list').status,'unknown');
  writeFileSync(password,'wrong-DOCTOR_SECRET');result=await doctor(['--probe','--username','reader','--password-file',password]);assert.equal(result.code,1);assert.equal(check(result.report,'probe.authentication').status,'fail');
  assert.equal(readFileSync(config,'utf8'),before);assert.equal(existsSync(runtime.NATSUI_DATA_DIR),false);
  assert.ok(!readdirSync(directory).some(name=>name.endsWith('.sqlite3')));
  if(process.env.NATSUI_TLS_FIXTURES){
    const certs=process.env.NATSUI_TLS_FIXTURES;
    const [tlsPort,tlsMonitor]=await Promise.all([port(),port()]);const tlsConfig=join(directory,'tls.json');
    writeFileSync(tlsConfig,JSON.stringify({host:'127.0.0.1',port:tlsPort,http_port:tlsMonitor,jetstream:{store_dir:join(directory,'tls-js')},authorization:{users:[{user:'CN=natsui-test',permissions:{publish:{allow:['$JS.API.STREAM.LIST']},subscribe:{allow:['_INBOX.>']}}}]},tls:{cert_file:join(certs,'server.pem'),key_file:join(certs,'server.key'),ca_file:join(certs,'ca.pem'),verify_and_map:true}}));
    const tls=start(process.env.NATSUI_TEST_SERVER,['-c',tlsConfig],baseline);tls.stdout.resume();tls.stderr.resume();await ready(`http://127.0.0.1:${tlsMonitor}/varz`);
    const pem=readFileSync(join(certs,'client.pem'),'utf8');const der=Buffer.from(pem.split('-----BEGIN CERTIFICATE-----')[1].split('-----END CERTIFICATE-----')[0].replace(/\s/g,''),'base64');const cert=join(directory,'client.crt');writeFileSync(cert,der);
    result=await doctor(['--probe','--certificate-file',cert,'--key-file',join(certs,'client.key')],{NATSUI_NATS_LOGIN_CERTIFICATES:'1',NATSUI_URL:`tls://127.0.0.1:${tlsPort}`,NATSUI_TLS_CA:join(certs,'ca.pem')});assert.equal(result.code,0,result.stdout);assert.equal(check(result.report,'probe.authentication').status,'pass');assert.equal(check(result.report,'probe.stream_list').status,'pass');
  }
  console.log('Doctor configuration, profiles, no-network default, no storage writes, secret redaction and real NATS identity probes passed.');
}finally{
  if(trap)await new Promise(r=>trap.close(r));
  for(const child of children.reverse())if(child.exitCode===null){const stopped=once(child,'exit');child.kill();await stopped;}
  const cleanup=resolve(directory);assert.equal(dirname(cleanup),resolve(tmpdir()));assert.ok(basename(cleanup).startsWith('natsui-doctor-'));rmSync(cleanup,{recursive:true,force:true});
}
