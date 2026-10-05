import assert from 'node:assert/strict';
import '../web/diagnostics.js';

const {consumer,retention}=globalThis.NatsuiDiagnostics;
const current={name:'worker',stream_name:'JOBS',created:'consumer-1',natsui_configuration:'a',num_pending:14,delivered:{consumer_seq:20},num_ack_pending:80,config:{max_ack_pending:100}};
const stream={created:'stream-1'};
const snapshot={status:'complete',observed_at:100};
const samples=[60,70,80,90,100].map((at,index)=>({at,status:'complete',summary:{interval_seconds:5,resources:{consumers:[{name:'worker',stream:'JOBS',created:'consumer-1',stream_created:'stream-1',pending:10+index,delivered:20,configuration:'a'}]}}}));
const kinds=(c=current,h=samples,s=snapshot)=>consumer(c,stream,s,h).facts.map(f=>f.kind);
assert.deepEqual(kinds(),['pressure','stalled','growing']);
assert.deepEqual(kinds({...current,paused:true}),['paused','pressure','growing']);
assert.deepEqual(kinds(current,samples.slice(-2)),['pressure']);
assert.deepEqual(kinds(current,samples,{...snapshot,status:'partial'}),['pressure']);
assert.deepEqual(kinds(current,samples,{...snapshot,observed_at:150}),['pressure']);
for(const change of [row=>row.status='partial',row=>row.at=70,row=>row.summary.resources.consumers[0].delivered=1,row=>row.summary.resources.consumers[0].created='replaced',row=>row.summary.resources.consumers[0].configuration='b']){
  const input=structuredClone(samples);change(input[3]);
  assert.deepEqual(kinds(current,input),['pressure']);
}
assert.deepEqual(kinds({...current,num_pending:0,delivered:{consumer_seq:25}}),['pressure']);
assert.deepEqual(kinds({...current,num_pending:10}),['pressure']);
assert.deepEqual(kinds(current,globalThis.NatsuiDiagnostics.indexed(samples)),kinds());
const pausedHistory=structuredClone(samples);for(const sample of pausedHistory)sample.summary.resources.consumers[0].paused=true;
assert.deepEqual(kinds(current,pausedHistory),['pressure']);
assert.deepEqual(kinds({...current,natsui_configuration:'resumed'}),['pressure']);
const unknown=retention({},100);
assert.equal(unknown.oldest,null);
assert.equal(unknown.limits[0].unlimited,false);
assert.equal(unknown.unlimitedAge,false);
assert.equal(unknown.effect,'Discard policy unavailable.');
const capacity=retention({config:{max_msgs:100,max_bytes:-1,max_age:0,discard:'new'},state:{messages:90,bytes:500,first_ts:'1970-01-01T00:01:00Z'}},100);
assert.equal(capacity.oldest,40);
assert.equal(capacity.limits[0].remaining,10);
assert.equal(capacity.limits[1].unlimited,true);
assert.equal(capacity.unlimitedAge,true);
assert.match(capacity.effect,/rejected/);
assert.equal(retention({state:{messages:0,first_ts:'1970-01-01T00:01:00Z'}},100).oldest,null);
console.log('Consumer evidence windows and retention unknowns passed');
