import assert from 'node:assert/strict';
import {readFile,writeFile,mkdtemp,unlink,rmdir} from 'node:fs/promises';
import {execFileSync} from 'node:child_process';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
const [dashboard,traffic]=process.argv.slice(2);
if(!dashboard||!traffic)throw new Error('Usage: node scripts/smoke-published.mjs DASHBOARD_IMAGE TRAFFIC_IMAGE');
const dir=await mkdtemp(join(tmpdir(),'natsui-published-'));
const file=join(dir,'compose.yaml'),project=`natsui-smoke-${process.pid}`;
const port=process.env.NATSUI_SMOKE_PORT||'14321';
const env={...process.env,NATSUI_DEMO_PORT:port};
const compose=(...args)=>execFileSync('docker',['compose','-p',project,'-f',file,...args],{env,stdio:'inherit'});
const endpoint=`http://127.0.0.1:${port}`;
const get=async path=>{const r=await fetch(endpoint+path,{signal:AbortSignal.timeout(10000)});assert.equal(r.status,200,`${path}: ${r.status}`);return r.json();};
await writeFile(file,(await readFile(new URL('../demo/published.yaml',import.meta.url),'utf8')).replace('ghcr.io/axmouth/natsui:latest',dashboard).replace('ghcr.io/axmouth/natsui-traffic:latest',traffic));
try{
 compose('up','-d','--wait','--wait-timeout','180','--pull','missing');
 const first=await get('/api/snapshot');assert.equal(first.snapshot.demo,false);assert.equal(first.snapshot.status,'complete');assert.equal(first.snapshot.streams.length,3);assert.equal(first.snapshot.consumers.length,5);
 assert.equal(first.monitoring.nodes.filter(n=>n.status==='complete').length,3);
 for(const stream of first.snapshot.streams){assert.equal(stream.config.num_replicas,3);assert.equal(stream.cluster.replicas.length,2);assert.ok(stream.cluster.replicas.every(r=>r.current&&!r.offline));}
 const pending=[];let last=first;
 for(let i=0;i<15;i++){last=await get('/api/snapshot');pending.push(last.snapshot.consumers.find(c=>c.name==='billing-worker').num_pending);if(i<14)await new Promise(r=>setTimeout(r,5000));}
 assert.ok(Math.max(...pending)-Math.min(...pending)>100,'Real billing backlog must change across workload phases');
 const initial=first.snapshot.streams.find(s=>s.config.name==='ORDERS');const final=last.snapshot.streams.find(s=>s.config.name==='ORDERS');assert.ok(final.state.last_seq>initial.state.last_seq);
 const record=await get('/api/latest/ORDERS');assert.equal(record.demo,false);assert.equal(record.message.subject,'orders.created');
 const history=await get('/api/history');assert.ok(history.length>=4);
 console.log(JSON.stringify({source:'real NATS',billing_pending_min:Math.min(...pending),billing_pending_max:Math.max(...pending),history_samples:history.length,retained_subject:record.message.subject}));
}catch(error){try{compose('logs','--tail','40');}catch{}throw error;}
finally{try{compose('down','-v');}finally{await unlink(file);await rmdir(dir);}}
