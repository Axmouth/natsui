'use strict';
globalThis.NatsuiDiagnostics = (() => {
  const numeric = value => typeof value === 'number' && Number.isFinite(value) && value >= 0;
  function indexed(history) { return history.map(sample=>({...sample,consumerIndex:new Map((sample.summary?.resources?.consumers||[]).map(row=>[row.stream+'\0'+row.name,row]))})); }
  function consumer(current, stream, snapshot, history) {
    const facts = [];
    if (current.paused === true) facts.push({kind:'paused', text:'Delivery is paused', detail:current.config?.pause_until || 'Resume time unavailable'});
    if (numeric(current.num_ack_pending) && current.config?.max_ack_pending > 0) {
      const pressure = current.num_ack_pending / current.config.max_ack_pending;
      if (pressure >= .8) facts.push({kind:'pressure',text:`${Math.round(pressure*100)}% of acknowledgment capacity outstanding`,detail:'Reported pending acknowledgments, not application completion.'});
    }
    let run = [];
    for (const sample of history) {
      if (sample.at < snapshot.observed_at - 300 || sample.at > snapshot.observed_at) continue;
      const row = sample.consumerIndex ? sample.consumerIndex.get(current.stream_name+'\0'+current.name) : sample.summary?.resources?.consumers?.find(row => row.name === current.name && row.stream === current.stream_name);
      const previous = run.at(-1);
      const valid = snapshot.status === 'complete' && sample.status === 'complete' && current.created && stream?.created && row?.created === current.created && row?.stream_created === stream.created && current.natsui_configuration && row.configuration === current.natsui_configuration && row.paused !== true && numeric(row.pending) && numeric(row.delivered);
      if (!valid) { run=[]; continue; }
      if (previous && sample.at <= previous.at) { run=[]; continue; }
      if (previous && (sample.at-previous.at > Math.max(2, sample.summary.interval_seconds || 5)*3+20 || row.delivered < previous.delivered || row.configuration !== previous.configuration)) run=[];
      run.push({at:sample.at,pending:row.pending,delivered:row.delivered,configuration:row.configuration});
    }
    const first=run[0], last=run.at(-1), seconds=last && first ? last.at-first.at : 0;
    if (seconds >= 30 && snapshot.observed_at-last.at <= 30 && current.num_pending === last.pending && current.delivered?.consumer_seq === last.delivered) {
      if (current.paused !== true && run.every(row=>row.pending>0) && last.delivered===first.delivered) facts.push({kind:'stalled',text:`No delivery progress observed for ${seconds}s with pending work`,detail:'The consumer delivery counter did not advance. Worker availability and the cause are unknown.'});
      if (last.pending > first.pending && run.every((row,index)=>!index || row.pending>=run[index-1].pending)) facts.push({kind:'growing',text:`Backlog rose by ${last.pending-first.pending} over ${seconds}s`,detail:'Derived from consecutive session observations. Expiry, deletion and filter changes may affect counts.'});
    }
    return {facts,seconds,coverage:seconds>=30?'No sustained delivery stall or backlog growth established by matching current observations.':'At least 30 seconds of consecutive matching observations is needed for progress diagnosis.'};
  }
  function retention(stream, observedAt) {
    const config=stream.config||{}, state=stream.state||{};
    const first=Date.parse(state.first_ts)/1000;
    const oldest=numeric(state.messages)&&state.messages>0&&Number.isFinite(first)&&first>0&&first<=observedAt?observedAt-first:null;
    const limits=[['Records',state.messages,config.max_msgs],['Bytes',state.bytes,config.max_bytes]].map(([name,used,max])=>({name,used:numeric(used)?used:null,limit:typeof max==='number'&&max>0?max:null,unlimited:max===-1,remaining:numeric(used)&&max>0?Math.max(0,max-used):null}));
    return {oldest,empty:state.messages===0,limits,maxAge:config.max_age>0?config.max_age/1e9:null,unlimitedAge:config.max_age===0,
      effect:config.discard==='new'?'New messages are rejected when applicable limits are reached.':config.discard==='old'?'Old records are discarded when applicable limits are reached.':'Discard policy unavailable.',
      policy:config.retention||'unavailable'};
  }
  return {consumer,retention,indexed};
})();
