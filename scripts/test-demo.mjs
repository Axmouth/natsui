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
