'use strict';
(() => {
  const diagnostics=element('section',null,'panel inspector session-insights');
  diagnostics.append(element('h2','Consumer progress'),element('p','Reported pressure and progress derived from consecutive observations. A quiet consumer is not evidence of a dead worker.'));
  const diagnosticRows=element('div',null,'table-wrap');diagnostics.append(diagnosticRows);$('page-consumers').append(diagnostics);
  const retention=element('section',null,'panel inspector session-insights');retention.append(element('h2','Retention and capacity'));
  const retentionRows=element('div',null,'table-wrap');retention.append(retentionRows,element('p','Oldest retained age is not oldest unprocessed age. Logical stream bytes are not filesystem usage. Per-subject limits and interest/work-queue retention can remove records sooner.','panel-note'));$('page-streams').append(retention);
  const certPanel=element('section',null,'panel inspector session-insights');certPanel.hidden=true;$('page-settings').prepend(certPanel);
  const monitoring=element('section',null,'panel inspector session-insights');monitoring.hidden=true;
  const monitorButton=element('button','Enable monitoring through NATS','quiet'),monitorNote=element('p');
  monitoring.append(element('h2','Session node monitoring'),element('p','Uses this NATS identity to request node metrics and inventories. Requires system service access or configured service imports. No HTTP collector fallback.'),monitorButton,monitorNote);$('page-nodes').prepend(monitoring);
  const watchPanel=element('section',null,'panel inspector session-insights'),watchSelect=element('select'),watchButton=element('button','Start failure watch','quiet'),watchStatus=element('p'),watchEvents=element('div');
  watchSelect.setAttribute('aria-label','Stream to watch');watchSelect.id='failure-watch-stream';watchButton.id='failure-watch-start';watchStatus.id='failure-watch-status';
  const controls=element('div',null,'record-controls');controls.append(watchSelect,watchButton);
  watchPanel.append(element('h2','Failure events'),element('p','Watches max-delivery and terminated-message advisories in bounded eight-second windows. Events outside those windows are missed. Silence does not verify subscription permission. This is not a DLQ.'),controls,watchStatus,watchEvents);$('page-activity').prepend(watchPanel);
  let sessionInfo=null, sessionLoading=false, watchActive=false, watchAbort=null, watchTimer=null, watchToken=0;
  const post=(path,body)=>api(path,{method:'POST',headers:{'Content-Type':'application/json','X-Natsui-Request':'1'},body:JSON.stringify(body)});
  function stopWatch() {
    watchActive=false;watchToken++;clearTimeout(watchTimer);watchAbort?.abort();watchAbort=null;watchButton.textContent='Start failure watch';watchSelect.disabled=false;
  }
  async function watchWindow(token) {
    if(!watchActive||token!==watchToken)return;
    watchAbort=new AbortController();
    try {
      watchStatus.textContent='Listening for failure advisories. Subscription delivery is permission-dependent...';
      const result=await api('/api/watch?stream='+encodeURIComponent(watchSelect.value),{signal:watchAbort.signal});
      if(token!==watchToken)return;
      watchStatus.textContent=`${result.events.length} events received in this window. ${result.gap?`${result.end_reason}. Coverage is incomplete. `:''}${result.detail}`;
      for(const event of result.events){
        const row=element('p',`${event.name}: ${event.detail} / sequence ${event.stream_seq ?? '--'} `);
        if(Number.isSafeInteger(event.stream_seq)&&event.stream_seq>0){const inspect=element('button','Inspect retained record','quiet');inspect.onclick=()=>inspectEvent(event);row.append(inspect);}
        watchEvents.prepend(row);
      }
      while(watchEvents.children.length>120)watchEvents.lastChild.remove();
      refresh();
      watchTimer=setTimeout(()=>watchWindow(token),500);
    } catch(error) {if(token!==watchToken)return;stopWatch();watchStatus.textContent='Watch stopped. '+error.message;}
  }
  async function inspectEvent(event){
    $('detail-title').textContent=`${event.stream} / sequence ${event.stream_seq}`;$('detail-body').replaceChildren(element('p','Reading retained record with current credentials...'));$('detail-dialog').showModal();
    try{const value=await api('/api/messages/'+encodeURIComponent(event.stream)+'/'+event.stream_seq);$('detail-body').replaceChildren(element('pre',JSON.stringify(value,null,2),'json'));}catch(error){$('detail-body').replaceChildren(element('p',error.message));}
  }
  watchButton.onclick=()=>{if(watchActive){stopWatch();watchStatus.textContent='Watch stopped. An in-flight window can finish within eight seconds. No new windows will start.';return;}watchActive=true;watchSelect.disabled=true;watchButton.textContent='Stop watch';watchWindow(++watchToken);};
  window.addEventListener('hashchange',()=>{if(currentPage()!=='activity')stopWatch();});
  document.addEventListener('visibilitychange',()=>{if(document.hidden)stopWatch();});
  window.addEventListener('pagehide',stopWatch);
  monitorButton.onclick=async()=>{monitorButton.disabled=true;try{const result=await post('/api/session/monitoring',{enabled:!sessionInfo?.monitoring});sessionInfo.monitoring=result.enabled;monitorNote.textContent=result.enabled?'Querying permitted NATS system services. Responders do not prove full cluster coverage.':'Monitoring stopped. Earlier session observations remain in history.';refresh();}catch(error){monitorNote.textContent=error.message;}finally{monitorButton.disabled=false;}};
  async function loadSession(){
    if(sessionLoading||sessionInfo||data?.dashboard?.login_method!=='nats')return;
    sessionLoading=true;try{sessionInfo=await api('/api/session');renderTools();}catch{}finally{sessionLoading=false;}
  }
  function age(seconds){return seconds==null?'Unavailable':seconds>=86400?`${(seconds/86400).toFixed(1)} days`:seconds>=3600?`${(seconds/3600).toFixed(1)} hours`:seconds>=60?`${Math.floor(seconds/60)} min`:`${Math.floor(seconds)}s`;}
  function renderTools(){
    if(!data)return;
    const snapshot=data.snapshot;
    if(currentPage()==='consumers'){
    const indexedHistory=NatsuiDiagnostics.indexed(history);
    const rows=visibleConsumers().map(c=>{const stream=snapshot.streams.find(s=>s.config.name===c.stream_name);const d=NatsuiDiagnostics.consumer(c,stream,snapshot,indexedHistory);return [cell(c.stream_name+' / '+c.name),cell(d.facts.map(f=>f.text).join('. ')||d.coverage),cell(d.facts.map(f=>f.detail).join(' '))];});
    diagnosticRows.replaceChildren(table([['Consumer'],['Observation'],['Evidence boundary']],rows));
    }
    if(currentPage()==='streams')retentionRows.replaceChildren(table([['Stream'],['Oldest retained'],['Record headroom'],['Byte headroom'],['Retention / age limit'],['At capacity']],snapshot.streams.filter(s=>`${s.config?.name} ${s.config?.subjects?.join(' ')}`.toLowerCase().includes($('stream-search').value.toLowerCase())).map(stream=>{const r=NatsuiDiagnostics.retention(stream,snapshot.observed_at);return [cell(stream.config.name),cell(r.empty?'Empty stream':age(r.oldest)),cell(r.limits[0].limit==null?(r.limits[0].unlimited?'No configured limit':'Unavailable'):number(r.limits[0].remaining)),cell(r.limits[1].limit==null?(r.limits[1].unlimited?'No configured limit':'Unavailable'):bytes(r.limits[1].remaining)),cell(`${r.policy} / ${r.maxAge==null?(r.unlimitedAge?'no age limit':'age limit unavailable'):age(r.maxAge)}`),cell(r.effect)];})));
    const sessionOnly=!!data.dashboard?.session_history;
    watchPanel.hidden=staticDemo || (data.dashboard?.login_method==='nats'&&!sessionOnly);
    if(!watchActive){const selected=watchSelect.value;watchSelect.replaceChildren(...snapshot.streams.map(stream=>{const option=element('option',stream.config.name);option.value=stream.config.name;return option;}));if([...watchSelect.options].some(o=>o.value===selected))watchSelect.value=selected;watchButton.disabled=!watchSelect.options.length;}
    monitoring.hidden=!sessionOnly;monitorButton.textContent=sessionInfo?.monitoring?'Stop NATS monitoring':'Enable monitoring through NATS';
    if(sessionOnly&&currentPage()==='activity')$('page-description').textContent='Actions and observed changes from this login only. Discarded when the session ends.';
    loadSession();
    const cert=sessionInfo?.certificate;certPanel.hidden=!cert;
    if(cert){const remaining=cert.expires_at-Date.now()/1000;certPanel.replaceChildren(element('h2','Session certificate'),element('p',`Subject: ${cert.subject}`),element('p',`Issuer: ${cert.issuer}`),element('p',`Valid from ${new Date(cert.not_before*1000).toLocaleString()} until ${new Date(cert.expires_at*1000).toLocaleString()}`),element('p',`SHA-256: ${cert.fingerprint}`,'json'),element('p',remaining<=0?'Certificate has expired':`Expires in ${age(remaining)}`,remaining<30*86400?'issue':'panel-note'));
      if(remaining<30*86400){const attention=element('div',remaining<=0?'The session certificate has expired.':'The session certificate expires in '+age(remaining)+'.','issue');$('collection-issues').append(attention);}}
  }
  globalThis.renderSessionTools=renderTools;
  renderTools();
})();
