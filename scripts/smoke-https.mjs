import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {mkdtempSync,readFileSync,writeFileSync,rmSync} from 'node:fs';
import {join,resolve,dirname,basename} from 'node:path';
import {tmpdir} from 'node:os';
import {once} from 'node:events';
import {createServer,connect} from 'node:net';
import {request as httpsRequest} from 'node:https';

const binary=resolve(process.argv[2]||'target/debug/natsui'+(process.platform==='win32'?'.exe':''));
const dir=mkdtempSync(join(tmpdir(),'natsui-https-'));
const certs=process.env.NATSUI_TLS_FIXTURES;
assert.ok(certs&&process.env.NATSUI_TEST_SERVER,'TLS fixtures and disposable NATS server are required');
const baseline=Object.fromEntries(Object.entries(process.env).filter(([k])=>!k.startsWith('NATSUI_')));
const children=[];const sockets=[];let logs='';
const ca=readFileSync(join(certs,'ca.pem'));
async function port(){const s=createServer().listen(0,'127.0.0.1');await once(s,'listening');const p=s.address().port;await new Promise(r=>s.close(r));return p;}
function launch(file,args,env){const p=spawn(file,args,{env,windowsHide:true,stdio:['ignore','pipe','pipe']});children.push(p);p.stdout.on('data',d=>logs+=d);p.stderr.on('data',d=>logs+=d);return p;}
async function cli(args,env){const p=launch(binary,args,env);let out='';p.stdout.on('data',b=>out+=b);const timer=setTimeout(()=>p.kill(),10000);try{const [code]=await once(p,'exit');return {code,out};}finally{clearTimeout(timer);}}
const pause=ms=>new Promise(r=>setTimeout(r,ms));
try{
 const [httpPort,natsPort,monitorPort]=await Promise.all([port(),port(),port()]);
 const origin='https://localhost:9443';
 const authKey='ab'.repeat(32);writeFileSync(join(dir,'auth.key'),authKey);
 const env={...baseline,NATSUI_PORT:String(httpPort),NATSUI_DATA_DIR:join(dir,'data'),NATSUI_AUTH_TOKEN_FILE:join(dir,'auth.key'),NATSUI_NATS_LOGIN:'1',NATSUI_NATS_LOGIN_SESSION_ONLY:'1',NATSUI_URL:`nats://127.0.0.1:${natsPort}`,NATSUI_PUBLIC_URL:origin,NATSUI_HTTPS_CERT:join(certs,'server.pem'),NATSUI_HTTPS_KEY:join(certs,'server.key'),NATSUI_HTTPS_CA:join(certs,'ca.pem')};
 const request=(path,{method='GET',body,headers={},trust=ca}={})=>new Promise((resolve,reject)=>{
  const req=httpsRequest({host:'127.0.0.1',port:httpPort,servername:'localhost',ca:trust,path,method,headers:{Host:'localhost:9443',Origin:origin,'X-Natsui-Request':'1','Content-Type':'application/json',...headers},timeout:4000},res=>{let text='';res.on('data',b=>text+=b);res.on('end',()=>resolve({status:res.statusCode,headers:res.headers,text}));});req.on('error',reject);req.on('timeout',()=>req.destroy(new Error('Request timeout')));req.end(body?JSON.stringify(body):undefined);
 });
 for(const patch of [{NATSUI_HTTPS_KEY:''},{NATSUI_PUBLIC_URL:'http://localhost:9443'},{NATSUI_HTTPS_KEY:join(certs,'client.key')}])assert.equal((await cli(['doctor','--json'],{...env,...patch})).code,1);
 const noAuth={...env,NATSUI_NATS_LOGIN:'0',NATSUI_NATS_LOGIN_SESSION_ONLY:'0'};delete noAuth.NATSUI_AUTH_TOKEN_FILE;
 assert.equal((await cli(['doctor','--json'],noAuth)).code,1);
 const natsConfig=join(dir,'nats.json');writeFileSync(natsConfig,JSON.stringify({host:'127.0.0.1',port:natsPort,http_port:monitorPort,authorization:{users:[{user:'reader',password:'https-fixture-password'}]},jetstream:{store_dir:join(dir,'js')}}));
 launch(process.env.NATSUI_TEST_SERVER,['-c',natsConfig],baseline);
 for(let n=0;n<60;n++){try{if((await fetch(`http://127.0.0.1:${monitorPort}/varz`)).ok)break;}catch{}await pause(100);}
 launch(binary,[],env);
 let ready=false;for(let n=0;n<80;n++){try{if((await request('/login')).status===200){ready=true;break;}}catch{}await pause(100);}assert.ok(ready,logs);
 assert.equal((await cli(['healthcheck'],env)).code,0,'Verified loopback HTTPS health check with different public port');
 const link=await cli(['login'],env);assert.equal(link.code,0,logs);assert.match(link.out,/https:\/\/localhost:9443\/login#ticket=[a-f0-9]{64}/);
 assert.equal((await cli(['healthcheck'],{...env,NATSUI_PUBLIC_URL:'https://wrong-host.invalid'})).code,1,'Hostname validation');
 const ipv6={...env,NATSUI_PUBLIC_URL:'https://[::1]:9443'};assert.equal((await cli(['healthcheck'],ipv6)).code,1,'A different public origin remains rejected even with a valid IPv6 SAN');
 const untrusted={...env};delete untrusted.NATSUI_HTTPS_CA;assert.equal((await cli(['healthcheck'],untrusted)).code,1,'Private CA must be trusted');
 assert.equal((await request('/login',{headers:{Host:'attacker.invalid'}})).status,403);
 assert.equal((await request('/api/auth/nats',{method:'POST',headers:{Origin:'https://attacker.invalid'},body:{profile:'default',username:'reader',password:'https-fixture-password'}})).status,403);
 const login=await request('/api/auth/nats',{method:'POST',body:{profile:'default',username:'reader',password:'https-fixture-password'}});
 assert.equal(login.status,204,login.text);assert.ok(login.headers['strict-transport-security']);
 const cookie=login.headers['set-cookie'][0];assert.match(cookie,/Secure/);assert.match(cookie,/HttpOnly/);assert.match(cookie,/SameSite=Strict/);
 const snapshot=await request('/api/snapshot',{headers:{Cookie:cookie.split(';')[0]}});assert.equal(snapshot.status,200);assert.equal(JSON.parse(snapshot.text).dashboard.session_history,true);
 const slow=connect(httpPort,'127.0.0.1');sockets.push(slow);slow.on('error',()=>{});await once(slow,'connect');
 assert.equal((await request('/login')).status,200,'Incomplete handshake must not block other clients');
 const plain=connect(httpPort,'127.0.0.1');sockets.push(plain);plain.on('error',()=>{});await once(plain,'connect');let plaintext='';plain.on('data',b=>plaintext+=b);plain.write('GET /login HTTP/1.1\r\nHost: localhost\r\n\r\n');await pause(200);assert.ok(!plaintext.includes('HTTP/1.1 200'),'HTTPS port must not serve HTTP');
 const der=(path)=>Buffer.from(readFileSync(path,'utf8').split('-----BEGIN CERTIFICATE-----')[1].split('-----END CERTIFICATE-----')[0].replace(/\s/g,''),'base64');writeFileSync(join(dir,'server.crt'),der(join(certs,'server.pem')));writeFileSync(join(dir,'ca.crt'),der(join(certs,'ca.pem')));
 const ipv6Port=await port();const ipv6Env={...env,NATSUI_PORT:String(ipv6Port),NATSUI_PUBLIC_URL:'https://[::1]:9443',NATSUI_DATA_DIR:join(dir,'ipv6-data'),NATSUI_HTTPS_CERT:join(dir,'server.crt'),NATSUI_HTTPS_CA:join(dir,'ca.crt')};launch(binary,[],ipv6Env);let ipv6Ready=false;for(let n=0;n<20;n++){if((await cli(['healthcheck'],ipv6Env)).code===0){ipv6Ready=true;break;}await pause(100);}assert.ok(ipv6Ready,'IPv6 public origin with verified loopback health client');
 const plainPort=await port();const plainEnv={...env,NATSUI_PORT:String(plainPort),NATSUI_DATA_DIR:join(dir,'plain-data')};for(const key of ['NATSUI_HTTPS_CERT','NATSUI_HTTPS_KEY','NATSUI_HTTPS_CA'])delete plainEnv[key];launch(binary,[],plainEnv);let plainReady=false;for(let n=0;n<20;n++){if((await cli(['healthcheck'],plainEnv)).code===0){plainReady=true;break;}await pause(100);}assert.ok(plainReady,'HTTP reverse-proxy hop health check');assert.equal((await cli(['login'],plainEnv)).code,0,'Existing HTTP proxy login CLI');
 assert.ok(!logs.includes('https-fixture-password')&&!logs.includes(authKey),'No credentials in process logs');
 console.log('Native HTTPS, verified local health/login CLI, hostname and CA checks, origin enforcement, Secure NATS sessions and concurrent handshakes passed.');
}finally{
 for(const s of sockets)s.destroy();
 for(const p of children.reverse())if(p.pid&&p.exitCode===null&&p.signalCode===null){const stop=once(p,'exit');p.kill();await stop;}
 const target=resolve(dir);assert.equal(dirname(target),resolve(tmpdir()));assert.ok(basename(target).startsWith('natsui-https-'));rmSync(target,{recursive:true,force:true});
}
