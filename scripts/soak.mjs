import {appendFileSync, mkdirSync, statSync, writeFileSync, renameSync} from 'node:fs';
import {resolve,dirname} from 'node:path';
import {performance} from 'node:perf_hooks';
import {execFileSync} from 'node:child_process';

// Each request has a deadline. The record contains operational evidence only.
const base=new URL(process.env.NATSUI_SOAK_URL||'http://127.0.0.1:4321');
if(!['127.0.0.1','localhost'].includes(base.hostname))throw new Error('The soak target must be local.');
const duration=Number(process.env.NATSUI_SOAK_SECONDS||86400);
const interval=Number(process.env.NATSUI_SOAK_INTERVAL||30);
if(!Number.isFinite(duration)||duration<=0||!Number.isFinite(interval)||interval<1)throw new Error('Invalid duration or interval');
const file=resolve(process.env.NATSUI_SOAK_FILE||`data/soak/${new Date().toISOString().replaceAll(':','-')}.jsonl`);
mkdirSync(dirname(file),{recursive:true});
const container=process.env.NATSUI_SOAK_CONTAINER;
if(container&&!/^[a-zA-Z0-9][a-zA-Z0-9_.-]{0,127}$/.test(container))throw new Error('Invalid container identity');
const processId=process.env.NATSUI_SOAK_PID;
if(processId&&!/^\d+$/.test(processId))throw new Error('Invalid process ID');
const started=Date.now(),summary={started_at:new Date(started).toISOString(),requested_seconds:duration,samples:0,failures:0,partial:0,max_latency_ms:0,complete:false};
console.log(`Recording soak evidence in ${file}`);
let stopped=false;process.on('SIGINT',()=>stopped=true);process.on('SIGTERM',()=>stopped=true);
const bytes=path=>{try{return statSync(path).size;}catch{return null;}};
while(!stopped) {
 const now=performance.now(),row={at:new Date().toISOString()};
 try {
  const [snapshot,ready]=await Promise.all(['/api/snapshot','/readyz'].map(async path=>{
   const r=await fetch(new URL(path,base),{signal:AbortSignal.timeout(10000)});
   if(path==='/api/snapshot'&&!r.ok)throw new Error('Snapshot request failed');
   return await r.json();
  }));
  Object.assign(row,{collection:snapshot.snapshot.status,observed_at:snapshot.snapshot.observed_at,streams:snapshot.summary.streams,consumers:snapshot.summary.consumers,storage:snapshot.dashboard?.storage,ready:ready.ready,monitoring:snapshot.monitoring?.status});
  if(snapshot.snapshot.status==='partial')summary.partial++;
  if(!ready.ready)summary.failures++;
 }catch {row.error='Dashboard response unavailable or invalid';summary.failures++;}
 row.latency_ms=Math.round(performance.now()-now);summary.max_latency_ms=Math.max(summary.max_latency_ms,row.latency_ms);
 if(process.env.NATSUI_SOAK_DATA_DIR){const db=resolve(process.env.NATSUI_SOAK_DATA_DIR,'natsui.sqlite3');row.database_bytes=bytes(db);row.wal_bytes=bytes(db+'-wal');}
 if(processId&&process.platform==='win32') {
  try {
   const metrics=JSON.parse(execFileSync('powershell.exe',['-NoProfile','-NonInteractive','-Command',`Get-Process -Id ${processId} | Select-Object WorkingSet64,CPU,StartTime | ConvertTo-Json -Compress`],{encoding:'utf8',windowsHide:true,timeout:5000,stdio:['ignore','pipe','ignore']}));
   row.dashboard_rss_bytes=metrics.WorkingSet64;row.dashboard_cpu_seconds=metrics.CPU;row.dashboard_process_started=metrics.StartTime;
   summary.max_dashboard_rss_bytes=Math.max(summary.max_dashboard_rss_bytes||0,metrics.WorkingSet64);
  }catch {row.process_metrics='unavailable';}
 }
 if(container) {
  try {
   const stats=JSON.parse(execFileSync('docker',['stats','--no-stream','--format','{{json .}}',container],{encoding:'utf8',windowsHide:true,timeout:10000,stdio:['ignore','pipe','ignore']}));
   const used=stats.MemUsage.split('/')[0].trim().match(/^([\d.]+)(B|KiB|MiB|GiB)$/);
   row.dashboard_container=container;
   row.dashboard_cpu_percent=Number.parseFloat(stats.CPUPerc);
   if(used){row.dashboard_memory_bytes=Number(used[1])*({B:1,KiB:1024,MiB:1048576,GiB:1073741824}[used[2]]);summary.max_dashboard_memory_bytes=Math.max(summary.max_dashboard_memory_bytes||0,row.dashboard_memory_bytes);}
  }catch {row.container_metrics='unavailable';}
 }
 appendFileSync(file,JSON.stringify(row)+'\n');summary.samples++;
 summary.elapsed_seconds=Math.round((Date.now()-started)/1000);
 summary.complete=(Date.now()-started)>=duration*1000;
 writeFileSync(file+'.summary.tmp',JSON.stringify(summary,null,2));renameSync(file+'.summary.tmp',file+'.summary.json');
 if(summary.complete||stopped)break;
 await new Promise(resolve=>setTimeout(resolve,Math.min(interval*1000,Math.max(1,duration*1000-(Date.now()-started)))));
}
console.log(JSON.stringify(summary));
if(summary.failures)process.exitCode=1;
