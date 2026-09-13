import assert from 'node:assert/strict';
await import('../web/subjects.js');
await import('../web/demo.js');
const {model,summary,api}=globalThis.NatsuiDemo;
assert.ok(model(55).consumers[0].num_pending>model(25).consumers[0].num_pending);
assert.ok(model(115).consumers[0].num_pending<model(65).consumers[0].num_pending);
for(let t=0;t<240;t++){
 const s=model(t),v=summary(s,10000);
 assert.equal(v.largest.pending,Math.max(...s.consumers.map(c=>c.num_pending)));
 for(const c of s.consumers){assert.ok(c.ack_floor.stream_seq<=c.delivered.stream_seq);assert.ok(c.num_pending>=0);}
 for(const stream of s.streams)assert.ok(stream.state.messages<=200000);
}
await assert.rejects(()=>api('/api/messages/JOBS/0'));
assert.ok((await api('/api/history')).length>=60);
console.log('Simulation invariants, cycle progression, maximum backlog and missing record checks passed.');

const page=await api("/api/records/ORDERS?subject=orders.*");assert.equal(page.records.length,20);assert.ok(page.next_seq);assert.equal((await api("/api/records/ORDERS?subject=payments.*")).records.length,0);
assert.equal(NatsuiSubjects.matches("a.>","a"),false);assert.equal(NatsuiSubjects.matches("a.*","a.b.c"),false);assert.equal(NatsuiSubjects.matches("a.>","a.b.c"),true);assert.equal(NatsuiSubjects.valid("a.>.b"),false);

assert.equal((await api("/api/latest/ORDERS?subject=orders.*")).message.subject,"orders.created");await assert.rejects(()=>api("/api/latest/ORDERS?subject=payments.*"));

// Node identities and lifetime counters must survive repeated workload cycles.
const {inventory}=globalThis.NatsuiDemo;
for(let t=0;t<=360;t++){
 const s=model(t,1000+t),previous=t?model(t-1,999+t):null;
 const connections=inventory(t,1000+t,'connections'),subscriptions=inventory(t,1000+t,'subscriptions');
 assert.equal(s.monitoring.demo,true);
 assert.equal(s.monitoring.nodes.length,3);
 assert.equal(summary(s,10000).resources.monitoring.nodes.length,3);
 for(const n of s.monitoring.nodes){
  const rows=connections.nodes[n.slot].rows,subs=subscriptions.nodes[n.slot].rows;
  assert.equal(n.connections,rows.length);assert.equal(n.subscriptions,subs.length);
  assert.ok(n.cpu>0&&n.cpu<100);assert.ok(n.mem>0);
  for(const sub of subs)assert.ok(rows.some(c=>c.cid===sub.cid&&c.subjects.includes(sub.subject)));
  for(const c of rows)assert.equal(subs.filter(sub=>sub.cid===c.cid).reduce((sum,sub)=>sum+sub.msgs,0),c.out_msgs);
  if(previous)for(const key of ['in_msgs','out_msgs','in_bytes','out_bytes','total_connections','slow_consumers','api_errors'])assert.ok(n[key]>=previous.monitoring.nodes[n.slot][key],`${key} decreased at ${t}`);
  if(t)for(const c of rows){const old=inventory(t-1,999+t,'connections').nodes[n.slot].rows.find(p=>p.cid===c.cid&&p.start===c.start);if(old){assert.ok(c.in_msgs>=old.in_msgs);assert.ok(c.out_msgs>=old.out_msgs);}}
 }
}
assert.notEqual(model(10).monitoring.nodes[0].cpu,model(50).monitoring.nodes[0].cpu);
assert.notEqual(model(10).monitoring.nodes[0].mem,model(50).monitoring.nodes[0].mem);
assert.equal(inventory(50,1050,'connections').nodes[0].rows.length,4);
assert.equal(inventory(60,1060,'connections').nodes[0].rows.length,3);
assert.notEqual(inventory(50,1050,'connections').nodes[0].rows.at(-1).start,inventory(170,1170,'connections').nodes[0].rows.at(-1).start);
assert.equal(inventory(50,1050,'connections',1).nodes[0].rows.length,0);
assert.equal(inventory(50,1050,'connections',1).nodes[0].total,4);
assert.equal((await api('/api/snapshot')).monitoring.nodes.length,3);
assert.equal((await api('/api/monitoring/connections?page=0')).nodes.length,3);
assert.ok((await api('/api/nodes/0/connections')).connections.length>=3);
assert.ok((await api('/api/history')).every(s=>s.summary.resources.monitoring.nodes.length===3));
await assert.rejects(()=>api('/api/nodes/3/connections'));
await assert.rejects(()=>api('/api/monitoring/connections?page=-1'));
await assert.rejects(()=>api('/api/monitoring/unknown'));
console.log('Synthetic node histories, inventory consistency, counter continuity, client turnover and monitoring routes passed.');
