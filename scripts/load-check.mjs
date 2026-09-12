import assert from 'node:assert/strict';
const base=new URL(process.env.NATSUI_LOAD_URL||'http://127.0.0.1:4321');
assert.ok(['localhost','127.0.0.1'].includes(base.hostname));
const started=performance.now();let requests=0;const latencies=[];
await Promise.all(Array.from({length:8},async()=>{
 for(let i=0;i<5;i++)for(const path of ['/api/snapshot','/api/history','/api/incidents']){
  const tick=performance.now();const response=await fetch(new URL(path,base),{signal:AbortSignal.timeout(15000)});
  assert.equal(response.status,200);const value=await response.json();
  if(path==='/api/snapshot')assert.ok(value.snapshot.observed_at);else assert.ok(Array.isArray(value));
  latencies.push(performance.now()-tick);requests++;
 }
}));
latencies.sort((a,b)=>a-b);
console.log(JSON.stringify({clients:8,requests,elapsed_ms:Math.round(performance.now()-started),p95_ms:Math.round(latencies[Math.floor(latencies.length*.95)]),max_ms:Math.round(latencies.at(-1))}));
