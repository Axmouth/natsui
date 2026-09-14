import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {mkdtempSync,unlinkSync,rmdirSync,existsSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
const image=process.argv[2];if(!image)throw new Error('Usage: node scripts/smoke-auth-storage.mjs IMAGE');
const prefix=`natsui-auth-smoke-${randomUUID().slice(0,8)}`,auth=`${prefix}-auth`,history=`${prefix}-history`,restored=`${prefix}-restored`;
const backup=mkdtempSync(join(tmpdir(),'natsui-backup-')),port=Number(process.env.NATSUI_AUTH_SMOKE_PORT||14326),base=`http://127.0.0.1:${port}`;
const docker=(...args)=>execFileSync('docker',args,{encoding:'utf8',stdio:['ignore','pipe','pipe']}).trim();
const delay=ms=>new Promise(resolve=>setTimeout(resolve,ms));
async function ready(){for(let i=0;i<40;i++){try{if((await fetch(base+'/readyz')).ok)return;}catch{}await delay(500);}throw new Error('Authenticated demo did not become ready');}
const launch=volume=>docker('run','-d','--name',prefix,'-p',`127.0.0.1:${port}:4321`,'-e','NATSUI_AUTH_TOKEN_FILE=/run/auth/access.key','-e','NATSUI_PROFILE=auth-smoke','-v',`${volume}:/data`,'-v',`${auth}:/run/auth:ro`,'--read-only','--cap-drop','ALL',image,'--demo');
const request=(path,cookie,method='GET',body)=>fetch(base+path,{method,headers:{...(cookie?{Cookie:cookie}:{}),'X-Natsui-Request':'1',Origin:base,'Content-Type':'application/json'},...(body?{body:JSON.stringify(body)}:{})});
async function login(key){const response=await request('/api/auth/login',null,'POST',{key});assert.equal(response.status,204);const cookie=response.headers.get('set-cookie');assert.ok(cookie.includes('HttpOnly')&&cookie.includes('SameSite=Strict'));return cookie.split(';')[0];}
try{
 docker('run','--rm','-v',`${auth}:/data`,image,'--init-auth','/data/access.key');
 const key=docker('run','--rm','--entrypoint','cat','-v',`${auth}:/data:ro`,image,'/data/access.key');
 assert.match(key,/^[0-9a-f]{64}$/);
 assert.throws(()=>docker('run','--rm','-v',`${auth}:/data`,image,'--init-auth','/data/access.key'));
 launch(history);await ready();
 assert.equal((await request('/api/snapshot')).status,401);
 assert.equal((await request('/api/auth/login',null,'POST',{key:'wrong'})).status,401);
 let cookie=await login(key);
 const changed={backlog_threshold:12345,refresh_seconds:2,retention_days:7};
 assert.equal((await request('/api/settings',cookie,'PUT',changed)).status,200);
 await delay(5500);
 const samples=await (await request('/api/history',cookie)).json();assert.ok(samples.length>=2);
 const first=samples[0].at;
 assert.equal((await request('/api/auth/logout',cookie,'POST')).status,204);
 assert.equal((await request('/api/snapshot',cookie)).status,401);
 cookie=await login(key);
 docker('stop',prefix);
 docker('run','--rm','--user','0','--entrypoint','tar','-v',`${history}:/data:ro`,'--mount',`type=bind,source=${backup},target=/backup`,image,'-czf','/backup/history.tar.gz','-C','/data','.');
 docker('rm',prefix);
 launch(history);await ready();
 assert.equal((await request('/api/snapshot',cookie)).status,401);
 cookie=await login(key);
 assert.deepEqual(await (await request('/api/settings',cookie)).json(),changed);
 assert.ok((await (await request('/api/history',cookie)).json()).some(s=>s.at===first));
 docker('stop',prefix);docker('rm',prefix);
 docker('run','--rm','--user','0','--entrypoint','tar','-v',`${restored}:/data`,'--mount',`type=bind,source=${backup},target=/backup,readonly`,image,'-xzf','/backup/history.tar.gz','-C','/data','.');
 launch(restored);await ready();
 assert.equal((await request('/api/snapshot',cookie)).status,401);
 cookie=await login(key);
 assert.deepEqual(await (await request('/api/settings',cookie)).json(),changed);
 assert.ok((await (await request('/api/history',cookie)).json()).some(s=>s.at===first));
 console.log('Access-key initialization, authentication, logout, session revocation on replacement, persistent settings/history, stopped backup and fresh-volume restore passed.');
}finally{
 try{docker('rm','-f',prefix);}catch{}
 for(const volume of [history,restored,auth]){try{docker('volume','rm',volume);}catch{}}
 if(existsSync(join(backup,'history.tar.gz')))unlinkSync(join(backup,'history.tar.gz'));
 rmdirSync(backup);
}
