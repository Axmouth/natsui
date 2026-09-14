/* Standalone demo adapter. It uses no network and never contacts NATS. Counts
   follow one reproducible workload cycle; history is explicitly synthetic. */
(function(root) {
 'use strict';
 // Integrating phase rates keeps counters monotonic when the workload repeats.
 function counter(elapsed,rates){
  const durations=[20,20,20,60],cycle=Math.floor(elapsed/120);
  let remaining=elapsed%120,total=cycle*rates.reduce((sum,rate,i)=>sum+rate*durations[i],0);
  for(let i=0;i<rates.length;i++){const seconds=Math.min(remaining,durations[i]);total+=seconds*rates[i];remaining-=seconds;}
  return Math.floor(total);
 }
 function clients(elapsed,slot){
  const tick=elapsed%120,cycle=Math.floor(elapsed/120),burst=tick>=40&&tick<60;
  const specs=[
   ['order-producer','javascript',['_INBOX.demo.orders'],[300,300,950,300],[300,300,950,300]],
   ['billing-worker','rust',['_INBOX.demo.billing'],[300,60,120,650],[300,60,120,650]],
   ['dispatch-worker','go',['dispatch.*','telemetry.>'],[2,2,4,2],[180,180,450,250]],
   ['burst-producer','javascript',['_INBOX.demo.burst'],[0,0,600,0],[0,0,600,0]]
  ];
  return specs.filter((_,i)=>i<3||burst).map(([name,lang,subjects,inRates,outRates],i)=>{
   const transient=i===3,origin=transient?cycle*120:0,scale=(slot+1)/3;
   const inbound=Math.floor((counter(elapsed,inRates)-counter(origin,inRates))*scale);
   const outbound=Math.floor((counter(elapsed,outRates)-counter(origin,outRates))*scale);
   return {cid:transient?1000+cycle:10+i,name:`demo-${name}-${slot+1}`,lang,version:'simulated',
    start:transient?`simulation-cycle-${cycle}-burst`:'simulation-start',subjects:subjects.map(subject=>subject.startsWith('_INBOX.')?`${subject}.${slot+1}`:subject),subscriptions:subjects.length,
    in_msgs:inbound,out_msgs:outbound,in_bytes:inbound*220,out_bytes:outbound*220,
    rtt:`${(0.3+slot*0.2+(1+Math.sin(elapsed/6+slot))*0.15).toFixed(2)}ms`,
    pending_bytes:i===2&&tick>=40&&tick<60?Math.floor((tick-40)*2048):0};
  });
 }
 function monitoring(elapsed,at){
  const tick=elapsed%120,cycle=Math.floor(elapsed/120),burst=tick>=40&&tick<60;
  return {demo:true,status:'complete',nodes:[0,1,2].map(slot=>{
   const rows=clients(elapsed,slot),scale=(slot+1)/3;
   const inbound=Math.floor(counter(elapsed,[602,362,1674,952])*scale);
   const outbound=Math.floor(counter(elapsed,[780,540,2120,1200])*scale);
   return {slot,server_id:`SIMULATED-NODE-${slot+1}`,server_name:`demo-${slot+1}`,start:'simulation-start',
    at:Math.floor(at),status:'complete',version:'2.11.8 (simulated)',uptime:`${Math.floor(elapsed)}s`,
    cpu:Number((8+slot*4+(burst?26:tick>=60?12:0)+4*Math.sin(elapsed/7+slot)).toFixed(1)),
    mem:Math.floor((45+slot*9+(burst?18:8)+4*Math.sin(elapsed/11+slot))*1024*1024),
    connections:rows.length,subscriptions:rows.reduce((n,c)=>n+c.subscriptions,0),
    total_connections:3+cycle+(tick>=40?1:0),slow_consumers:cycle+(tick>=50?1:0),
    in_msgs:inbound,out_msgs:outbound,in_bytes:inbound*220,out_bytes:outbound*220,
    js_storage:Math.floor((90+8*Math.sin(elapsed/18))*1024*1024),js_max_storage:1024*1024*1024,
    js_memory:0,js_max_memory:256*1024*1024,api_errors:Math.floor(elapsed/35)};
  })};
 }
 function inventory(elapsed,at,kind,page=0){
  if(!['connections','subscriptions'].includes(kind)||!Number.isSafeInteger(page)||page<0)throw new Error('Invalid simulated inventory request.');
  return {demo:true,page,at:Math.floor(at),scope:'Synthetic clients and counters; no monitoring endpoints are contacted',nodes:monitoring(elapsed,at).nodes.map(n=>{
   const connections=clients(elapsed,n.slot);
   const rows=kind==='connections'?connections:connections.flatMap(c=>c.subjects.map((subject,i)=>({
    account:'DEMO',subject,qgroup:subject==='dispatch.*'?'dispatch-workers':'',cid:c.cid,
    msgs:Math.floor(c.out_msgs/c.subjects.length)+(i<c.out_msgs%c.subjects.length?1:0)
   })));
   return {slot:n.slot,name:n.server_name,server_id:n.server_id,start:n.start,at:n.at,status:n.status,total:rows.length,rows:rows.slice(page*100,(page+1)*100)};
  })};
 }
 function model(elapsed,at=Date.now()/1000) {
  const tick=Math.floor(elapsed)%120;
  const [phase,description,extra]=tick<20?['Steady traffic','Producers and workers are keeping pace. Small backlogs are normal.',tick*20]:tick<40?['Slow billing worker','Billing processes less than arrives; fulfillment continues independently.',400+(tick-20)*450]:tick<60?['Order burst','A short producer burst grows billing and job backlogs.',9400+(tick-40)*650]:['Recovery','Production returns to baseline and workers drain accumulated work.',Math.max(0,22400-(tick-60)*374)];
  const streams=[],consumers=[];
  [['ORDERS','orders.created',300,1],['PAYMENTS','payments.captured',120,3],['JOBS','jobs.resize',180,2]].forEach(([name,subject,rate,factor],index)=>{
   const last=100000+Math.floor(elapsed)*rate,pending=600+Math.floor(extra/factor),acks=tick>=20&&tick<60?800+tick*5:30+tick%11,retained=name==='JOBS'?pending+acks:Math.min(last,200000);
   streams.push({cluster:{name:'simulated-cluster',leader:'demo-'+(index+1),replicas:[1,2,3].filter(n=>n!==index+1).map(n=>({name:'demo-'+n,current:true,lag:0,offline:false}))},created:'2026-09-11T00:00:00Z',config:{name,subjects:[subject],retention:name==='JOBS'?'workqueue':'limits',storage:'file',num_replicas:3,max_msgs:200000,metadata:{natsui:'simulation'}},state:{messages:retained,bytes:retained*220,first_seq:last-retained+1,last_seq:last,consumer_count:name==='JOBS'?1:2}});
   for(let worker=0;worker<(name==='JOBS'?1:2);worker++){
    const backlog=worker===0?pending:20+(tick*13+index*5)%190,outstanding=worker===0?acks:8+tick%8;
    consumers.push({created:'2026-09-11T00:00:00Z',stream_name:name,name:[['billing-worker','fulfillment'],['settlement','receipts'],['image-workers']][index][worker],num_pending:backlog,num_ack_pending:outstanding,num_redelivered:tick>=20&&tick<60&&worker===0?tick-19:0,num_waiting:tick>=20&&tick<60&&worker===0?0:1,config:{ack_policy:'explicit',max_ack_pending:1200,max_deliver:5,filter_subject:subject},delivered:{consumer_seq:last-backlog,stream_seq:last-backlog},ack_floor:{stream_seq:last-backlog-outstanding}});
   }
  });
  return {observed_at:Math.floor(at),scope:'Standalone simulation',demo:true,status:'complete',issues:[],streams,consumers,monitoring:monitoring(elapsed,at),scenario:{phase,description,second:tick,duration:120,source:'simulation'}};
 }
 function summary(s,threshold){const c=s.consumers.reduce((a,b)=>a.num_pending>b.num_pending?a:b);return {resources:{consumers:s.consumers.map(c=>({name:c.name,stream:c.stream_name,created:c.created,stream_created:s.streams.find(v=>v.config.name===c.stream_name).created,pending:c.num_pending,ack_pending:c.num_ack_pending,redelivered:c.num_redelivered,waiting:c.num_waiting,delivered:c.delivered.consumer_seq})),streams:s.streams.map(stream=>{const consumer=s.consumers.filter(c=>c.stream_name===stream.config.name).sort((a,b)=>b.num_pending-a.num_pending)[0];return {name:stream.config.name,created:stream.created,messages:stream.state.messages,bytes:stream.state.bytes,last_seq:stream.state.last_seq,pending:consumer?.num_pending??null,consumer:consumer?.name,rate_eligible:true};}),monitoring:s.monitoring},largest:{name:c.name,stream:c.stream_name,pending:c.num_pending,ack_pending:c.num_ack_pending},behind:s.consumers.filter(c=>c.num_pending>threshold).length,threshold,streams:s.streams.length,consumers:s.consumers.length,stored_bytes:s.streams.reduce((n,s)=>n+s.state.bytes,0)};}
 let settings={backlog_threshold:10000,refresh_seconds:2,retention_days:7};
 let start=Date.now(),offset=120,paused=false,frozen=120,lastSecond=-1,samples=[],events=[];
 function elapsed(){return paused?frozen:offset+(Date.now()-start)/1000;}
 function record(detail){events.unshift({at:Math.floor(Date.now()/1000),kind:'simulation',detail});events=events.slice(0,100);}
 function seed(){samples=[];const at=Date.now()/1000;for(let i=0;i<120;i+=2){const s=model(i,at-120+i);samples.push({at:s.observed_at,status:'complete',summary:summary(s,settings.backlog_threshold)});}record('Synthetic prior cycle generated for this demonstration. No broker data is used.');}
 seed();
 async function api(path,options){const e=elapsed(),s=model(e);if(Math.floor(e)!==lastSecond){samples.push({at:s.observed_at,status:s.status,summary:summary(s,settings.backlog_threshold)});samples=samples.slice(-240);lastSecond=Math.floor(e);}
  if(path==='/api/snapshot')return {snapshot:s,monitoring:s.monitoring,summary:summary(s,settings.backlog_threshold),settings:{...settings}};
  if(path==='/api/history')return samples;
  if(path.startsWith('/api/history/window?')){const q=new URL(path,'http://demo.local').searchParams,from=Number(q.get('from')),to=Number(q.get('to')),rows=samples.filter(s=>s.at>=from&&s.at<=to);return {from,to,samples:rows.slice(-240),total:rows.length,truncated:rows.length>240};}
  if(path==='/api/activity')return events;
  if(path.split('?')[0]==='/api/incidents'){const q=new URL(path,'http://demo.local').searchParams,from=Number(q.get('from')||0),to=Number(q.get('to')||Infinity),result=[];for(let i=1;i<samples.length;i++){const a=samples[i-1].summary.largest,b=samples[i].summary.largest;if(a&&b&&a.name===b.name&&(a.pending>settings.backlog_threshold)!==(b.pending>settings.backlog_threshold)){const at=samples[i].at;if(at>=from&&at<=to)result.push({at,kind:b.pending>settings.backlog_threshold?'backlog-high':'backlog-recovered',source:'simulation',resource:'consumer',stream:b.stream,name:b.name,detail:`Synthetic observation: pending delivery ${a.pending} to ${b.pending}; threshold ${settings.backlog_threshold}`});}}return result.reverse();}
  if(path==='/api/settings'){if(options){const next=JSON.parse(options.body);if(!Number.isInteger(next.backlog_threshold)||next.backlog_threshold<1||next.backlog_threshold>1e9||next.refresh_seconds<2||next.refresh_seconds>300||next.retention_days<1||next.retention_days>90)throw new Error('Settings outside supported bounds');settings=next;record('Demo settings changed for this browser session. No server settings were edited.');}return {...settings};}
  if(path.startsWith('/api/monitoring/')){const url=new URL(path,'http://demo.local');return inventory(e,s.observed_at,url.pathname.split('/').at(-1),Number(url.searchParams.get('page')||0));}
  if(path.startsWith('/api/nodes/')){const match=/^\/api\/nodes\/(\d+)\/connections$/.exec(path),slot=match?Number(match[1]):-1;if(slot<0||slot>2)throw new Error('Node is not in the simulated inventory.');const rows=clients(e,slot);return {demo:true,at:s.observed_at,total:rows.length,connections:rows};}
  if(path.startsWith('/api/records/')){const url=new URL(path,'http://demo.local'),name=decodeURIComponent(url.pathname.split('/').at(-1)),stream=s.streams.find(v=>v.config.name===name);if(!stream)throw new Error('Stream is not in the simulated inventory.');const subject=url.searchParams.get('subject')||'>',start=Math.max(stream.state.first_seq,Number(url.searchParams.get('start')||stream.state.first_seq));if(!NatsuiSubjects.valid(subject)||!Number.isSafeInteger(start)||start<1)throw new Error('Invalid subject filter or sequence.');const records=[];if(NatsuiSubjects.matches(subject,stream.config.subjects[0]))for(let seq=start;seq<=stream.state.last_seq&&records.length<20;seq++)records.push({seq:String(seq),subject:stream.config.subjects[0],bytes:11});const next=records.length&&Number(records.at(-1).seq)<stream.state.last_seq?String(Number(records.at(-1).seq)+1):null;return {demo:true,stream:name,observed_at:s.observed_at,state:stream.state,records,next_seq:next,exhausted:!next};}
  if(path.startsWith('/api/latest/')){const url=new URL(path,'http://demo.local'),name=decodeURIComponent(url.pathname.split('/').at(-1)),stream=s.streams.find(v=>v.config.name===name),subject=url.searchParams.get('subject')||'>';if(!stream||!NatsuiSubjects.valid(subject)||!NatsuiSubjects.matches(subject,stream.config.subjects[0]))throw new Error('No simulated retained record matches this filter.');return api(`/api/messages/${encodeURIComponent(name)}/${stream.state.last_seq}`);}
  if(path.startsWith('/api/messages/')){const [, , ,name,raw]=path.split('/');const stream=s.streams.find(v=>v.config.name===decodeURIComponent(name));const seq=Number(raw);if(!stream||!Number.isSafeInteger(seq)||seq<stream.state.first_seq||seq>stream.state.last_seq)throw new Error('Sequence is outside this simulated stream retention range.');return {demo:true,message:{subject:stream.config.subjects[0],seq,time:new Date(s.observed_at*1000).toISOString(),data:btoa(JSON.stringify({demo:true,id:`DEMO-${seq}`,workload:stream.config.name,phase:s.scenario.phase}))}};}
  if(path==='/api/buckets')return {status:'complete',buckets:[{kind:'kv',bucket:'app-settings',bytes:512,replicas:3},{kind:'object',bucket:'reports',bytes:4096,replicas:3}]};
  if(path.startsWith('/api/buckets/')){const url=new URL(path,'http://demo.local'),kind=url.pathname.split('/')[3];if(url.pathname.endsWith('/entry'))return kind==='kv'?{kind:'kv',key:url.searchParams.get('key'),operation:'PUT',present:true,revision:42,text:'{"batch_size":100}',demo:true}:{kind:'object',name:'daily-report.json',size:4096,chunks:1,deleted:false,revision:17,demo:true};return {rows:[{key:kind==='kv'?'worker.batch':'ZGFpbHktcmVwb3J0Lmpzb24',name:kind==='kv'?'worker.batch':'daily-report.json',retained_revisions:2}],next_offset:null,demo:true};}
  throw new Error('This operation is not available in the standalone simulation.');
 }
 function pause(){if(paused){offset=frozen;start=Date.now();paused=false;}else{frozen=elapsed();paused=true;}return paused;}
 function reset(){start=Date.now();offset=120;frozen=120;paused=false;lastSecond=-1;seed();}
 const subjectExamples=[
  {subject:'orders.created',detail:'ORDERS capture with billing and fulfillment consumers.'},
  {subject:'payments.captured',detail:'PAYMENTS capture with settlement and receipt consumers.'},
  {subject:'jobs.resize',detail:'JOBS work-queue capture with image workers.'},
  {subject:'dispatch.resize',detail:'Core NATS queue-group interest; no configured stream capture.'},
  {subject:'telemetry.worker.cpu',detail:'Wildcard subscription interest; no configured stream capture.'},
  {subject:'unmatched.example',detail:'No capture or subscription match in this demo inventory.'}
 ];
 root.NatsuiDemo={model,summary,inventory,subjectExamples,api,pause,reset};
})(globalThis);
