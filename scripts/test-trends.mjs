import assert from 'node:assert/strict';
import '../web/trends.js';
const {series}=globalThis.NatsuiTrends;
const sample=(at,last,created='a',status='complete')=>({at,status,summary:{resources:{streams:[{name:'S',created,last_seq:last,messages:100,rate_eligible:true}]}}});
const id={name:'S',created:'a'};
assert.deepEqual(series([sample(1,100),sample(6,150)],'stream',id,'last_seq',5,true).map(p=>p.value),[null,10]);
assert.deepEqual(series([sample(1,100),sample(6,90),sample(11,100)],'stream',id,'last_seq',5,true).map(p=>p.value),[null,null,2]);
assert.equal(series([sample(1,100),sample(6,150,'b')],'stream',id,'last_seq',5,true)[1].value,null);
assert.equal(series([sample(1,100),sample(30,150)],'stream',id,'last_seq',5,true)[1].value,null);
assert.equal(series([sample(1,100),sample(6,150,'a','partial'),sample(11,200)],'stream',id,'last_seq',5,true)[2].value,null);
const node=(at,start='a',counter=100)=>({at,summary:{resources:{monitoring:{nodes:[{at,server_id:'N',start,status:'complete',in_msgs:counter}]}}}});
assert.deepEqual(series([node(1),node(1),node(6,'a',150)],'node',{server_id:'N',start:'a'},'in_msgs',5,true).map(p=>p.value),[null,10]);
assert.equal(series([node(1),node(6,'b',150)],'node',{server_id:'N',start:'a'},'in_msgs',5,true)[1].value,null);
console.log('Resource trend rates, restarts, counter resets, duplicate samples and coverage gaps passed.');

const consumerSample=(at,delivered,created='c',stream='S',status='complete')=>({at,status,summary:{resources:{consumers:[{stream,stream_created:'s',name:'C',created,pending:42,delivered}]}}});
const consumerId={stream:'S',stream_created:'s',name:'C',created:'c'};
assert.deepEqual(series([consumerSample(1,10),consumerSample(6,30)],'consumer',consumerId,'delivered',5,true).map(p=>p.value),[null,4]);
assert.equal(series([consumerSample(1,10),consumerSample(6,30,'other')],'consumer',consumerId,'pending',5)[1].value,null);
assert.equal(series([consumerSample(1,10),consumerSample(6,30,'c','OTHER')],'consumer',consumerId,'pending',5)[1].value,null);
assert.equal(series([consumerSample(1,10),consumerSample(6,2)],'consumer',consumerId,'delivered',5,true)[1].value,null);
assert.equal(series([consumerSample(1,10),consumerSample(6,20,'c','S','partial'),consumerSample(11,40)],'consumer',consumerId,'delivered',5,true)[2].value,null);
console.log('Consumer identity, counter resets and missing observations passed.');

assert.deepEqual(NatsuiTrends.direction([{at:1,value:10},{at:6,value:20}],5),{rate:2,seconds:5});assert.equal(NatsuiTrends.direction([{at:1,value:10},{at:6,value:null},{at:11,value:30}],5),null);assert.equal(NatsuiTrends.direction([{at:1,value:10},{at:60,value:30}],5),null);
