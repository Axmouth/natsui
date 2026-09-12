'use strict';
const $ = id => document.getElementById(id);
const staticDemo = globalThis.NATSUI_STATIC_DEMO === true;
const pages = {nodes:['Nodes','Process resources, traffic and monitoring coverage.'],node:['Node detail','Reported process metrics and recorded trends.'],stream:['Stream detail','Retention, consumer progress and historical trends.'],about:['About','Origins and acknowledgments.'],overview:['Overview','Consumer progress, with the context to understand it.'],streams:['Streams','Stored records, subject capture and retention.'],consumers:['Consumers','Server-side delivery state. No worker polling required.'],messages:['Messages','Inspect what is retained, without taking work.'],activity:['Activity','A small, persistent record of this workspace.'],settings:['Settings','Clear boundaries between the dashboard and the server.'],coverage:['Data coverage','What is reported, what is derived, and what remains unknown.']};
let data = null, history = [], timer, busy = false, settingsDirty = false, lastOk = 0;
const recordsState={stream:'',subject:'',request:0,reading:0,pages:[],page:-1,next:null,loading:false};
const number = value => value == null ? '--' : new Intl.NumberFormat().format(value);
const bytes = value => value == null ? '--' : value >= 1073741824 ? `${(value / 1073741824).toFixed(1)} GiB` : value >= 1048576 ? `${(value / 1048576).toFixed(1)} MiB` : value >= 1024 ? `${(value / 1024).toFixed(1)} KiB` : `${Math.round(value)} B`;
function element(tag, text, cls) { const e=document.createElement(tag); if(text!=null)e.textContent=text; if(cls)e.className=cls; return e; }
function badge(text, cls='') { return element('span',text,`badge ${cls}`); }
function currentPage(){ return location.hash.slice(1).split('?')[0] || 'overview'; }
function filters(){ return new URLSearchParams(location.hash.split('?')[1] || ''); }
function filterUrl(key,value){ const params=filters(); if(value)params.set(key,value); else params.delete(key); historyReplace(`#${currentPage()}${params.size?'?'+params:''}`); }
function historyReplace(url){ window.history.replaceState(null,'',url); }
async function api(path, options){ if(staticDemo)return globalThis.NatsuiDemo.api(path,options);const response=await fetch(path,options); if(!response.ok)throw new Error((await response.text()).slice(0,250)); return response.json(); }
function navigate(){
 const page=pages[currentPage()]?currentPage():'overview';
 document.querySelectorAll('.page').forEach(e=>e.hidden=e.id!==`page-${page}`);
 const navPage={consumer:'consumers',stream:'streams',node:'nodes'}[page]||page;
 document.querySelectorAll('[data-page]').forEach(e=>{e.classList.toggle('active',e.dataset.page===navPage); if(e.dataset.page===navPage)e.setAttribute('aria-current','page');else e.removeAttribute('aria-current');});
 $('page-title').textContent=pages[page][0];$('page-description').textContent=pages[page][1];
 if(page==='streams')$('stream-search').value=filters().get('q')||'';
 if(page==='consumers'){ $('consumer-search').value=filters().get('q')||''; $('behind-only').checked=filters().get('behind')==='1'; }
 if(page==='messages'&&filters().has('stream')){recordsState.route=filters().get('stream');$('message-subject').value=filters().get('subject')||'';}
 if(data)render();
}
function table(headers, rows){ const t=element('table'),head=element('thead'),tr=element('tr');headers.forEach(([label,numeric])=>tr.append(element('th',label,numeric?'numeric':'')));head.append(tr);t.append(head);const body=element('tbody');rows.forEach(cells=>{const row=element('tr');cells.forEach(cell=>row.append(cell));body.append(row);});t.append(body);return t; }
function cell(text,numeric=false){return element('td',text,numeric?'numeric':'');}
// Preserve focus and click targets when a polling response has identical rows.
function setTable(target, table){ const host=$(target); if(host.firstElementChild?.outerHTML!==table.outerHTML)host.replaceChildren(table); }
function empty(target,message){$(target).replaceChildren(element('div',message,'empty'));}
function sortedConsumers(){return [...data.snapshot.consumers].sort((a,b)=>(b.num_pending??0)-(a.num_pending??0));}
function consumerTable(target, consumers){
 if(!consumers.length){empty(target,data.snapshot.status==='unavailable'?'Consumer information is unavailable.':'No observed consumers match this view.');return;}
 const host=$(target),key=JSON.stringify(consumers.map(c=>[c.stream_name,c.name,c.created]));
 if(host.dataset.rows!==key||!host.querySelector('table')){host.dataset.rows=key;host.replaceChildren(table([['Consumer'],['Pending trend'],['Pending delivery',true],['Awaiting ack',true],['Redelivered',true],['Observation']],consumers.map(()=>[cell(),cell(),cell(null,true),cell(null,true),cell(null,true),cell()])));}
 consumers.forEach((c,index)=>{const cells=host.querySelector('tbody').rows[index].cells;
 if(!cells[0].firstElementChild){const button=element('button',c.name,'name-button');button.onclick=()=>consumerDetail(c);cells[0].append(button,element('span',c.stream_name,'secondary'));}
 const mini=cells[1].firstElementChild||element('div',null,'sparkline');if(!mini.isConnected)cells[1].append(mini);NatsuiTrends.draw(mini,resourceSeries('consumer',consumerIdentity(c),'pending'),{label:`${c.name} / pending deliveries`,mini:true,interval:data.settings.refresh_seconds});cells[2].textContent=number(c.num_pending);cells[3].textContent=number(c.num_ack_pending);cells[4].textContent=number(c.num_redelivered);cells[5].replaceChildren(c.num_pending>data.settings.backlog_threshold?badge('Above threshold','warn'):badge('Within threshold'));
 });
}
function consumerDetail(c){location.hash='#consumer?'+new URLSearchParams({stream:c.stream_name,name:c.name});}

function renderStreams(){
 const query=$('stream-search').value.toLowerCase();const streams=data.snapshot.streams.filter(s=>`${s.config?.name} ${s.config?.subjects?.join(' ')}`.toLowerCase().includes(query));
 if(!streams.length){empty('streams-table',data.snapshot.status==='unavailable'?'Stream inventory is unavailable.':'No observed streams match this view.');return;}
 const host=$('streams-table'),key=JSON.stringify(streams.map(s=>[s.config.name,s.created]));
 if(host.dataset.rows!==key||!host.querySelector('table')){host.dataset.rows=key;host.replaceChildren(table([['Stream'],['Retained trend'],['Records',true],['Stored',true],['Retention'],['Replicas',true]],streams.map(()=>[cell(),cell(),cell(null,true),cell(null,true),cell(),cell(null,true)])));}
 streams.forEach((s,index)=>{const cells=host.querySelector('tbody').rows[index].cells;const b=element('a',s.config.name,'name-button');b.href='#stream?name='+encodeURIComponent(s.config.name);cells[0].replaceChildren(b,element('span',(s.config.subjects||[]).join(', '),'secondary'));const mini=cells[1].firstElementChild||element('div',null,'sparkline');if(!mini.isConnected)cells[1].append(mini);NatsuiTrends.draw(mini,resourceSeries('stream',{name:s.config.name,created:s.created},'messages'),{label:s.config.name+' / retained records',mini:true,interval:data.settings.refresh_seconds});cells[2].textContent=number(s.state?.messages);cells[3].textContent=bytes(s.state?.bytes);cells[4].textContent=s.config.retention||'limits';cells[5].textContent=number(s.config.num_replicas);});
}
let inspectedTime = null;
function inspectChart(host, svg, samples, x, y, shape){
 const tip=element('div',null,'history-inspector');tip.id='history-inspector';tip.hidden=true;tip.setAttribute('role','status');
 const cursor=shape('line',{y1:15,y2:140,class:'inspection-cursor',visibility:'hidden'});
 const point=shape('circle',{r:4,class:'inspection-point',visibility:'hidden'});
 host.append(tip);host.tabIndex=0;host.setAttribute('role','group');host.setAttribute('aria-label','Backlog history. Hover or tap to inspect. Use left and right arrows to browse samples, Home and End to jump, Escape to clear.');host.setAttribute('aria-describedby',tip.id);
 const nearest=at=>samples.reduce((best,sample)=>Math.abs(sample.at-at)<Math.abs(best.at-at)?sample:best,samples[0]);
 function clear(){inspectedTime=null;tip.hidden=true;cursor.setAttribute('visibility','hidden');point.setAttribute('visibility','hidden');}
 function show(at){
  inspectedTime=Math.max(samples[0].at,Math.min(samples.at(-1).at,at));const sample=nearest(inspectedTime);
  const before=samples.findLast(item=>item.at<=inspectedTime),after=samples.find(item=>item.at>=inspectedTime);
  const gap=before&&after&&after.at-before.at>data.settings.refresh_seconds*3;
  cursor.setAttribute('x1',x(gap?inspectedTime:sample.at));cursor.setAttribute('x2',x(gap?inspectedTime:sample.at));cursor.setAttribute('visibility','visible');tip.hidden=false;
  const largest=sample.summary.largest,complete=sample.status==='complete'&&largest?.pending!=null;
  point.setAttribute('visibility',!gap&&complete?'visible':'hidden');
  if(gap){tip.replaceChildren(element('strong','No samples in this interval'),element('span',`${new Date(before.at*1000).toLocaleTimeString()} to ${new Date(after.at*1000).toLocaleTimeString()}`));return;}
  if(complete){point.setAttribute('cx',x(sample.at));point.setAttribute('cy',y(largest.pending));}
  tip.replaceChildren(element('time',new Date(sample.at*1000).toLocaleString()),element('strong',complete?`${number(largest.pending)} pending deliveries`:sample.status==='complete'?'No observed consumer':`Collection ${sample.status}`),element('span',complete?`${largest.name} / ${largest.stream}`:'No complete backlog value for this sample.'),element('span',complete?`${number(largest.ack_pending)} awaiting ack / ${sample.status} sample`:'Gaps are not zero traffic.'));
 }
 function pointer(event){const matrix=svg.getScreenCTM();if(!matrix)return;const p=svg.createSVGPoint();p.x=event.clientX;p.y=event.clientY;const local=p.matrixTransform(matrix.inverse());show(samples[0].at+(local.x-48)/540*(samples.at(-1).at-samples[0].at));}
 host.onpointermove=pointer;host.onpointerdown=event=>{host.focus({preventScroll:true});pointer(event);};
 host.onpointerleave=event=>{if(event.pointerType!=='touch'&&document.activeElement!==host)clear();};
 host.onfocus=()=>{if(inspectedTime==null)show(samples.at(-1).at);};host.onblur=clear;
 host.onkeydown=event=>{if(!['ArrowLeft','ArrowRight','Home','End','Escape'].includes(event.key))return;event.preventDefault();if(event.key==='Escape'){clear();return;}const index=samples.indexOf(nearest(inspectedTime??samples.at(-1).at));const next=event.key==='Home'?0:event.key==='End'?samples.length-1:Math.max(0,Math.min(samples.length-1,index+(event.key==='ArrowLeft'?-1:1)));show(samples[next].at);};
 if(inspectedTime!=null)show(inspectedTime);
}
function renderChart(){
 const host=$('history-chart'); const valid=history.filter(h=>h.status==='complete' && h.summary.largest?.pending!=null);
 if(valid.length<2){host.replaceChildren(element('div','History begins with collection. Waiting for a second complete sample.','empty'));return;}
 const NS='http://www.w3.org/2000/svg';const svg=document.createElementNS(NS,'svg');svg.setAttribute('viewBox','0 0 600 180');svg.setAttribute('role','img');svg.setAttribute('aria-label','Largest consumer backlog across collected samples');
 function shape(tag,attrs,text){const e=document.createElementNS(NS,tag);for(const [k,v]of Object.entries(attrs))e.setAttribute(k,v);if(text)e.textContent=text;svg.append(e);return e;}
 const max=Math.max(1,...valid.map(h=>h.summary.largest.pending))*1.1;const first=history[0].at,last=history.at(-1).at;const x=t=>48+(t-first)/Math.max(1,last-first)*540;const y=v=>140-v/max*125;
 for(let i=0;i<4;i++){const value=max*i/3;shape('line',{x1:48,y1:y(value),x2:588,y2:y(value),class:'grid'});shape('text',{x:0,y:y(value)+3},new Intl.NumberFormat('en',{notation:'compact',maximumFractionDigits:0}).format(value));}
 let path='',previous=null;
 history.forEach(h=>{if(h.status!=='complete'||h.summary.largest?.pending==null){previous=null;return;}const gap=previous && h.at-previous.at>data.settings.refresh_seconds*3;path+=`${!previous||gap?'M':'L'}${x(h.at)} ${y(h.summary.largest.pending)} `;previous=h;});
 shape('path',{d:path,class:'line'});const end=valid.at(-1);shape('circle',{cx:x(end.at),cy:y(end.summary.largest.pending),r:3,class:'point'});
 shape('text',{x:48,y:169},new Date(first*1000).toLocaleTimeString());shape('text',{x:588,y:169,'text-anchor':'end'},new Date(last*1000).toLocaleTimeString());host.replaceChildren(svg);inspectChart(host,svg,history,x,y,shape);
}
function renderAttention(){
 const host=$('attention-items');host.replaceChildren();const consumers=sortedConsumers();let count=0;
 const add=(title,body,href)=>{const e=element('div',null,'attention-item');e.append(element('strong',title),element('p',body));if(href){const a=element('a','Inspect consumers');a.href=href;e.append(a);}host.append(e);count++;};
 if(data.snapshot.status==='unavailable')add('Evidence is unavailable','Collection has not established stream or consumer state. This is not a healthy zero.','#coverage');
 else {
 if(data.summary.behind)add(`${number(data.summary.behind)} consumers exceed the backlog threshold`,`More than ${number(data.settings.backlog_threshold)} messages awaiting delivery. A threshold crossing is a reason to inspect, not proof of a worker failure.`,'#consumers?behind=1');
 const pressured=consumers.filter(c=>c.config?.max_ack_pending>0&&c.num_ack_pending/c.config.max_ack_pending>=.8);
 if(pressured.length)add(`${pressured.length} ${pressured.length===1?'consumer':'consumers'} near the ack limit`,'At least 80% of configured acknowledgment capacity is outstanding. Inspect processing and acknowledgment behavior.','#consumers?q='+encodeURIComponent(pressured[0].name));
 if(!count)add('No configured threshold conditions observed','This assessment covers pending deliveries and acknowledgment pressure only.','#coverage');
 }$('attention-count').textContent=count;
}
function render(){
 const s=data.snapshot,summary=data.summary;
 let health=$('storage-health');if(!health){health=element('div',null,'issue');health.id='storage-health';health.setAttribute('role','status');$('collection-issues').before(health);}
 const storage=data.dashboard?.storage;health.hidden=!storage||storage.status==='ok';
 if(!health.hidden){const failed=Object.entries(storage.operations||{}).filter(([,v])=>v.status==='failed').map(([k])=>k.replaceAll('_',' ')).join(', ');health.textContent=`Dashboard storage needs attention${failed?': '+failed:''}. History or incidents may not be saved. The last known settings remain in use. Check disk space and directory access.`;}

 $('scenario-panel').hidden=!s.scenario;
 if(s.scenario){$('scenario-phase').textContent=s.scenario.phase;$('scenario-description').textContent=s.scenario.description;$('scenario-time').textContent=`${s.scenario.second}s / ${s.scenario.duration}s`;$('scenario-progress').value=s.scenario.second;$('scenario-source').textContent=s.demo?'SIMULATED SCENARIO':'LIVE WORKLOAD';}
 $('scenario-pause').hidden=!staticDemo;$('scenario-reset').hidden=!staticDemo;
 $('profile').textContent=s.scope;$('demo-banner').hidden=!s.demo;$('mode').textContent=s.demo?'Simulation':s.status==='complete'?'Live / read-only':s.status;$('mode').className=`badge ${s.demo?'demo':s.status==='complete'?'good':'warn'}`;
 $('largest').textContent=summary.largest?number(summary.largest.pending):s.status==='unavailable'?'--':'0';$('largest-name').textContent=summary.largest?`${summary.largest.name} / ${summary.largest.stream}`:'No observed consumer';
 $('behind').textContent=number(summary.behind);$('threshold-label').textContent=`More than ${number(data.settings.backlog_threshold)} pending deliveries`;$('streams-count').textContent=number(summary.streams);$('consumer-count').textContent=`${number(summary.consumers)} observed consumers`;$('storage').textContent=bytes(summary.stored_bytes);$('nav-streams').textContent=number(summary.streams);$('nav-consumers').textContent=number(summary.consumers);
 const issueNodes=s.issues.map(text=>element('div',text,'issue'));if(s.status==='partial')issueNodes.unshift(element('div','Partial coverage: counts below describe observed resources, not the entire account.','issue'));$('collection-issues').replaceChildren(...issueNodes);
 $('freshness').textContent=`Observed ${new Date(s.observed_at*1000).toLocaleTimeString()} / ${s.status}`;
 renderResources();if(globalThis.renderWorkspace)renderWorkspace();renderAttention();renderChart();consumerTable('overview-table',sortedConsumers().slice(0,5));renderStreams();
 const query=$('consumer-search').value.toLowerCase();consumerTable('consumers-table',sortedConsumers().filter(c=>`${c.name} ${c.stream_name}`.toLowerCase().includes(query)&&(!$('behind-only').checked||c.num_pending>data.settings.backlog_threshold)));
 if(document.activeElement!==$('message-stream')){const selected=$('message-stream').value;$('message-stream').replaceChildren(...s.streams.map(stream=>{const option=element('option',stream.config.name);option.value=stream.config.name;return option;}));if([...$('message-stream').options].some(o=>o.value===selected))$('message-stream').value=selected;}
 syncMessages();
 if(!settingsDirty){$('setting-threshold').value=data.settings.backlog_threshold;$('setting-refresh').value=data.settings.refresh_seconds;$('setting-retention').value=data.settings.retention_days;}
}
async function refresh(){
 if(busy)return;busy=true;$('refresh').disabled=true;
 try{const results=await Promise.allSettled([api('/api/snapshot'),api('/api/history'),api('/api/activity'),api('/api/incidents')]);
 if(results[0].status==='rejected')throw results[0].reason;
 if(globalThis.acceptIncidents)acceptIncidents(results[3]);
 data=results[0].value;lastOk=Date.now();history=results[1].status==='fulfilled'?results[1].value:[];render();
 if(results[1].status==='rejected')empty('history-chart','History storage is unavailable.');
 if(results[2].status==='fulfilled'){$('activity-list').replaceChildren(...results[2].value.map(event=>{const row=element('div',null,'activity-row');row.append(element('time',new Date(event.at*1000).toLocaleString()),badge(event.kind),element('p',event.detail));return row;}));if(!results[2].value.length)empty('activity-list','No recorded activity for this workspace.');}else empty('activity-list','Activity storage is unavailable.');
 }catch(error){renderMonitorCoverage(data?.monitoring,true);$('collection-issues').replaceChildren(element('div',`Dashboard unavailable. ${lastOk?'Previously shown values are stale. ':''}${error.message}`,'issue'));$('mode').textContent='Disconnected';$('mode').className='badge warn';$('freshness').textContent=lastOk?`Last dashboard response ${new Date(lastOk).toLocaleTimeString()}`:'No dashboard response';}
 finally{busy=false;$('refresh').disabled=false;clearTimeout(timer);timer=setTimeout(refresh,3000);}
}
$('refresh').onclick=refresh;
const flavors=[['','Original'],['neuronic','Neuronic'],['chlorophyll','Chlorophyll'],['crimson','Crimson'],['eosin','Eosin'],['azure','Azure'],['iris','Iris'],['carotene','Carotene']];
function closeFlavors(focus=false){$('flavor-menu').hidden=true;$('flavor-btn').setAttribute('aria-expanded','false');if(focus)$('flavor-btn').focus();}
function applyFlavor(value){
 document.documentElement.dataset.flavor=value;
 const label=flavors.find(([id])=>id===value)[1];
 $('flavor-btn').title=`Color theme: ${label}`;$('flavor-btn').setAttribute('aria-label',`Choose color theme, current: ${label}`);
 document.querySelectorAll('#flavor-menu button').forEach(button=>{const active=button.dataset.flavor===value;button.classList.toggle('on',active);button.setAttribute('aria-pressed',String(active));});
}
for(const [value,label] of flavors){const button=element('button',null,'flavor-item');button.type='button';button.dataset.flavor=value;const swatch=element('span',null,'swatch');swatch.setAttribute('aria-hidden','true');button.append(swatch,element('span',label));button.onclick=()=>{applyFlavor(value);localStorage.setItem('natsui-flavor',value);closeFlavors(true);};$('flavor-menu').append(button);}
const savedFlavor=localStorage.getItem('natsui-flavor')||'';applyFlavor(flavors.some(([id])=>id===savedFlavor)?savedFlavor:'');
$('flavor-btn').onclick=()=>{const open=$('flavor-menu').hidden;$('flavor-menu').hidden=!open;$('flavor-btn').setAttribute('aria-expanded',String(open));if(open)$('flavor-menu').querySelector('[aria-pressed="true"]').focus();};
document.addEventListener('pointerdown',event=>{if(!$('flavor-pick').contains(event.target))closeFlavors();});
document.addEventListener('keydown',event=>{if(event.key==='Escape'&&!$('flavor-menu').hidden){event.preventDefault();closeFlavors(true);}});
$('flavor-pick').addEventListener('focusout',event=>{if(!$('flavor-pick').contains(event.relatedTarget))closeFlavors();});
$('scenario-pause').onclick=()=>{const paused=globalThis.NatsuiDemo.pause();$('scenario-pause').textContent=paused?'Resume simulation':'Pause simulation';refresh();};
$('scenario-reset').onclick=()=>{globalThis.NatsuiDemo.reset();$('scenario-pause').textContent='Pause simulation';refresh();};
function applyTheme(theme){document.documentElement.dataset.theme=theme;const label=`Use ${theme==='dark'?'light':'dark'} theme`;$('theme-toggle').textContent=theme==='dark'?'\u263e':'\u2600';$('theme-toggle').setAttribute('aria-label',label);$('theme-toggle').title=label;}
$('theme-toggle').onclick=()=>{const theme=document.documentElement.dataset.theme==='dark'?'light':'dark';applyTheme(theme);localStorage.setItem('natsui-theme',theme);};
applyTheme(localStorage.getItem('natsui-theme')==='light'?'light':'dark');
$('consumer-search').oninput=()=>{filterUrl('q',$('consumer-search').value);render();};$('stream-search').oninput=()=>{filterUrl('q',$('stream-search').value);render();};$('behind-only').onchange=()=>{filterUrl('behind',$('behind-only').checked?'1':'');render();};
$('detail-close').onclick=()=>$('detail-dialog').close();$('palette-close').onclick=()=>$('palette').close();
function palette(){ $('palette-links').replaceChildren(...Object.entries(pages).filter(([,v])=>v[0].toLowerCase().includes($('palette-search').value.toLowerCase())).map(([id,v])=>{const a=element('a',v[0]);a.href=`#${id}`;a.onclick=()=>$('palette').close();return a;})); }
$('palette-open').onclick=()=>{palette();$('palette').showModal();$('palette-search').focus();};$('palette-search').oninput=palette;document.addEventListener('keydown',e=>{if((e.ctrlKey||e.metaKey)&&e.key.toLowerCase()==='k'){e.preventDefault();$('palette-open').click();}});
$('settings-form').oninput=()=>settingsDirty=true;
$('settings-form').onsubmit=async e=>{e.preventDefault();const button=e.submitter;button.disabled=true;try{const settings={backlog_threshold:Number($('setting-threshold').value),refresh_seconds:Number($('setting-refresh').value),retention_days:Number($('setting-retention').value)};await api('/api/settings',{method:'PUT',headers:{'Content-Type':'application/json','X-Natsui-Request':'1'},body:JSON.stringify(settings)});settingsDirty=false;$('settings-result').textContent='Saved. The collector uses these settings on its next cycle.';await refresh();}catch(error){$('settings-result').textContent=error.message;}finally{button.disabled=false;}};
function matchingConsumers(stream,subject){return data.snapshot.consumers.filter(c=>c.stream_name===stream&&NatsuiSubjects.filters(c).some(filter=>NatsuiSubjects.matches(filter,subject)));}
function resetRecords(){recordsState.request++;recordsState.reading++;recordsState.pages=[];recordsState.page=-1;recordsState.next=null;recordsState.loading=false;$('record-list').replaceChildren();$('message-result').replaceChildren();$('record-status').textContent='Browse storage to verify retained records for this selection.';pageButtons();}
function pageButtons(){$('records-previous').disabled=recordsState.loading||recordsState.page<=0;$('records-next').disabled=recordsState.loading||!recordsState.next;$('browse-form').querySelector('button[type="submit"]').disabled=recordsState.loading;}
function syncMessages(){
 const selector=$('message-stream');if(recordsState.route){if([...selector.options].some(o=>o.value===recordsState.route))selector.value=recordsState.route;delete recordsState.route;}
 const stream=selector.value,subject=$('message-subject').value.trim();if(recordsState.stream!==stream||recordsState.subject!==subject){recordsState.stream=stream;recordsState.subject=subject;resetRecords();}
 const s=data.snapshot.streams.find(s=>s.config.name===stream),host=$('message-context');if(!s){host.replaceChildren(element('p','Stream information is unavailable.'));return;}
 const left=element('div'),right=element('div'),consumers=data.snapshot.consumers.filter(c=>c.stream_name===stream),capture=s.config.subjects||[];
 left.append(element('h3','Stream storage'),element('p',`${number(s.state.messages)} retained records / ${bytes(s.state.bytes)} / ${s.config.retention} retention`),element('p',s.state.messages?`Sequence bounds: ${s.state.first_seq} to ${s.state.last_seq}. Gaps may exist.`:'No retained records at the last observation.'),element('p',`Captured subjects: ${capture.join(', ')||'None configured (check sources or mirror)'}`),element('span',`Inventory observed ${new Date(data.snapshot.observed_at*1000).toLocaleTimeString()} / ${data.snapshot.status}`,'secondary'));
 right.append(element('h3','Consumer filters'),element('p','Select a filter to browse matching stored records. Matching means eligible by subject, not delivered, acknowledged or completed.','secondary'));
 for(const c of consumers){const line=element('p');line.append(element('strong',c.name+' '));for(const filter of NatsuiSubjects.filters(c)){const button=element('button',filter,'quiet');button.type='button';button.onclick=()=>{$('message-subject').value=filter;$('message-start').value='';syncMessages();};line.append(button);}right.append(line);}
 if(!consumers.length)right.append(element('p','No consumers in the observed inventory.'));
 host.replaceChildren(left,right);$('subject-suggestions').replaceChildren(...[...new Set([...capture,...consumers.flatMap(NatsuiSubjects.filters)])].map(filter=>{const o=element('option');o.value=filter;return o;}));
}
async function browseRecords(start,mode='new'){
 const stream=$('message-stream').value,subject=$('message-subject').value.trim()||'>';
 if(!NatsuiSubjects.valid(subject)||start&&!/^[1-9][0-9]*$/.test(start)){$('record-status').textContent='Use a positive sequence and a valid NATS subject filter.';return;}
 const ticket=++recordsState.request;recordsState.loading=true;pageButtons();$('record-status').textContent='Reading stream storage...';
 try{
  const params=new URLSearchParams({subject});if(start)params.set('start',start);
  const result=await api(`/api/records/${encodeURIComponent(stream)}?${params}`);if(ticket!==recordsState.request)return;
  if(mode==='new'){recordsState.pages=[start||''];recordsState.page=0;}else if(mode==='next'){recordsState.pages=recordsState.pages.slice(0,recordsState.page+1);recordsState.pages.push(start);recordsState.page++;}else recordsState.page--;
  recordsState.next=result.next_seq;
  $('record-status').textContent=`${result.demo?'SIMULATION / ':''}${result.records.length} records found in ${stream} at ${new Date(result.observed_at*1000).toLocaleTimeString()}. ${result.exhausted?'End of matching records within the observed bounds.':'More records may be available.'} Storage can change after this read.`;
  $('record-list').replaceChildren(table([['Sequence'],['Subject'],['Stored at'],['Payload',true],['Matching consumer filters']],result.records.map(record=>{
   const seq=cell(),button=element('button',record.seq,'name-button');button.onclick=()=>inspectMessage(stream,record.seq);seq.append(button);
   const matches=matchingConsumers(stream,record.subject);return [seq,cell(record.subject),cell(record.time?new Date(record.time).toLocaleString():'Not provided'),cell(bytes(record.bytes),true),cell(matches.map(c=>c.name).join(', ')||'None observed')];
  })));
  if(!result.records.length)$('record-list').replaceChildren(element('p','No matching retained records found at this read.','empty'));
 }catch(error){if(ticket===recordsState.request){$('record-list').replaceChildren();$('record-status').textContent=error.message;recordsState.next=null;}}
 finally{if(ticket===recordsState.request){recordsState.loading=false;pageButtons();}}
}
async function inspectMessage(stream,seq,latest=false){
 $('message-title').textContent=latest?`${stream} / newest matching record`:`${stream} / ${seq}`;if(!$('message-dialog').open)$('message-dialog').showModal();
 const ticket=++recordsState.reading;$('message-result').replaceChildren(element('p',latest?'Reading newest matching record...':`Reading ${stream} / sequence ${seq}...`));
 try{const result=await api(latest?`/api/latest/${encodeURIComponent(stream)}?${new URLSearchParams({subject:$('message-subject').value.trim()||'>'})}`:`/api/messages/${encodeURIComponent(stream)}/${encodeURIComponent(seq)}`);if(ticket!==recordsState.reading)return;const message=result.message||result;
 const nodes=[element('h3',message.subject||'Message'),badge(result.demo?'Illustrative demo record':'Verified in stream storage','good'),element('p',`${stream} / sequence ${message.seq} / stored ${message.time||'timestamp not provided'} / read ${new Date((result.observed_at||Date.now()/1000)*1000).toLocaleTimeString()}`)];
 const s=data.snapshot.streams.find(s=>s.config.name===stream),capture=(s?.config.subjects||[]).filter(f=>NatsuiSubjects.matches(f,message.subject)),consumers=matchingConsumers(stream,message.subject);
 nodes.push(element('p',`Matching capture rules: ${capture.join(', ')||'None in current observed configuration'}. Matching consumer filters: ${consumers.map(c=>c.name).join(', ')||'None observed'}.`),element('p','Subject matches describe current configuration, not delivery or acknowledgment status. A retained record may disappear after this read.','panel-note'));
 if(message.data){const raw=Uint8Array.from(atob(message.data),c=>c.charCodeAt(0));let text;try{text=new TextDecoder('utf-8',{fatal:true}).decode(raw);try{text=JSON.stringify(JSON.parse(text),null,2);}catch{}}catch{text=`Binary payload (${raw.length} bytes). Base64:\n${message.data}`;}nodes.push(element('pre',text));}else nodes.push(element('p','Empty payload'));
 if(message.hdrs){let headers;try{headers=new TextDecoder().decode(Uint8Array.from(atob(message.hdrs),c=>c.charCodeAt(0)));}catch{headers=message.hdrs;}nodes.push(element('h3','Headers'),element('pre',headers));}
 $('message-result').replaceChildren(...nodes);$('message-seq').value=String(message.seq);
 }catch(error){if(ticket===recordsState.reading)$('message-result').replaceChildren(element('div',error.message,'issue'));}
}
$('message-close').onclick=()=>{recordsState.reading++;$('message-dialog').close();};
$('newest-record').onclick=()=>inspectMessage($('message-stream').value,null,true);
$('message-stream').onchange=()=>{$('message-start').value='';$('message-subject').value='';filterUrl('stream',$('message-stream').value);filterUrl('subject','');syncMessages();};
$('message-subject').oninput=()=>{filterUrl('subject',$('message-subject').value);syncMessages();};
$('browse-form').onsubmit=e=>{e.preventDefault();browseRecords($('message-start').value.trim());};
$('records-next').onclick=()=>browseRecords(recordsState.next,'next');
$('records-previous').onclick=()=>browseRecords(recordsState.pages[recordsState.page-1],'previous');
$('message-form').onsubmit=e=>{e.preventDefault();inspectMessage($('message-stream').value,$('message-seq').value);};
window.addEventListener('hashchange',navigate);
function resourceSeries(kind,identity,field,rate=false){return NatsuiTrends.series(history,kind,identity,field,data.settings.refresh_seconds,rate);}
function shortNumber(value){return value==null?'--':new Intl.NumberFormat('en',{notation:'compact',maximumFractionDigits:1}).format(value);}
function percent(used,max){return typeof used==='number'&&typeof max==='number'&&max>0?`${(used/max*100).toFixed(1)}%`:null;}
function resourceCharts(target,key,charts){
 const host=$(target);
 if(host.dataset.key!==key){host.dataset.key=key;host.replaceChildren(...charts.map(c=>{const panel=element('section',null,'panel');const head=element('div',null,'panel-heading');head.append(element('h2',c.label));const plot=element('div',null,'chart');plot.dataset.metric=c.field;panel.append(head,plot,element('p',c.note||'Recorded samples. Hover, tap or use arrow keys to inspect.','panel-note'));return panel;}));}
 charts.forEach(c=>NatsuiTrends.draw(host.querySelector(`[data-metric="${c.field}"]`),c.points,{label:c.label,format:c.format||shortNumber,readout:c.field==='bytes'||c.field==='mem'?v=>`${bytes(v)} (${number(v)} bytes)`:c.field==='cpu'?v=>`${v.toFixed(1)}%`:undefined,interval:data.settings.refresh_seconds}));
}
function replicas(s){
 if(!s.cluster)return s.config.num_replicas===1?'Single replica':'Replication state unavailable';
 const peers=s.cluster.replicas||[];
 return `Leader: ${s.cluster.leader||'unknown'}. ${peers.length} observed followers. `+peers.map(p=>`${p.name}: ${p.offline?'offline':p.current?'current':'not current'}${p.lag!=null?`, lag ${number(p.lag)}`:''}`).join('; ');
}
function renderMonitorCoverage(monitor, disconnected=false){
 const host=$('monitor-coverage'),nodes=monitor?.nodes||[];
 const reporting=nodes.filter(n=>n.status==='complete'&&Date.now()/1000-n.at<=15).length;
 let state='neutral',label='Monitoring not configured',detail='Set NATSUI_MONITOR_URLS to enable node monitoring.';
 if(disconnected){state='unavailable';label='Monitoring unavailable';detail='The dashboard could not refresh monitoring data.';}
 else if(monitor.status!=='not_configured'){
  if(nodes.length){state=reporting===nodes.length?'complete':reporting?'partial':'unavailable';label=`${reporting}/${nodes.length} reporting`;detail=`${reporting} of ${nodes.length} configured monitoring endpoints have successful observations within 15 seconds.`;}
  else{state=monitor.status==='connecting'?'partial':'unavailable';label=monitor.status==='connecting'?'Connecting to monitoring':'Monitoring unavailable';detail='Waiting for node monitoring observations.';}
 }
 host.dataset.state=state;host.textContent=label;
 host.title=detail+' CPU and RAM measure NATS processes, not container limits or remaining capacity.';
}
function renderResources(){
 const monitor=data.monitoring||{status:'not_configured',nodes:[]},nodes=monitor.nodes||[];
 renderMonitorCoverage(monitor);
 if(!nodes.length)empty('nodes-table','No server monitoring observations.');
 else {
  const host=$('nodes-table'),key=JSON.stringify(nodes.map(n=>[n.slot,n.server_id,n.start]));
  if(host.dataset.rows!==key||!host.querySelector('table')){host.dataset.rows=key;host.replaceChildren(table([['Node'],['CPU',true],['Resident RAM',true],['Connections',true],['Subscriptions',true],['Slow detections (total)',true],['Coverage']],nodes.map(()=>[cell(),cell(),cell(),cell(),cell(),cell(null,true),cell()])));}
  nodes.forEach((n,index)=>{const cells=host.querySelector('tbody').rows[index].cells,valid=n.status==='complete'&&Date.now()/1000-n.at<=15;
   if(n.server_id){const link=element('a',n.server_name||n.server_id,'name-button');link.href='#node?id='+encodeURIComponent(n.server_id);cells[0].replaceChildren(link,element('span',`NATS ${n.version}`,'secondary'));}else cells[0].textContent=`Endpoint ${n.slot+1}`;
   [['cpu',v=>`${v.toFixed(1)}%`,'CPU'],['mem',bytes,'Resident RAM'],['connections',number,'Connections'],['subscriptions',number,'Subscriptions']].forEach(([field,format,label],i)=>{
    const td=cells[i+1];if(!td.firstElementChild){const wrapper=element('div',null,'metric-trend');wrapper.append(element('span'),element('div',null,'sparkline'));td.append(wrapper);}const wrapper=td.firstElementChild;wrapper.firstElementChild.textContent=valid&&n[field]!=null?format(n[field]):'--';
    NatsuiTrends.draw(wrapper.lastElementChild,resourceSeries('node',{server_id:n.server_id,start:n.start},field),{label:`${n.server_name||'Node'} / ${label}`,mini:true,readout:format,interval:data.settings.refresh_seconds});
   });
   cells[5].textContent=valid?number(n.slow_consumers):'--';cells[6].textContent=valid?'Observed '+new Date(n.at*1000).toLocaleTimeString():n.error||'Stale / unavailable';
  });
 }
 if(currentPage()==='stream'){
  const s=data.snapshot.streams.find(s=>s.config.name===filters().get('name'));
  if(!s){empty('stream-summary','Stream is not in the current observed inventory.');$('stream-charts').replaceChildren();delete $('stream-charts').dataset.key;return;}
  $('page-title').textContent=s.config.name;const msgPercent=percent(s.state.messages,s.config.max_msgs),bytePercent=percent(s.state.bytes,s.config.max_bytes);
  const summary=$('stream-summary');const configOpen=summary.dataset.stream===s.config.name&&summary.querySelector('details')?.open;summary.dataset.stream=s.config.name;summary.replaceChildren(element('h2',`${number(s.state.messages)} retained records / ${bytes(s.state.bytes)}`),element('p',`${s.config.retention} retention. ${msgPercent?msgPercent+' of record limit. ':''}${bytePercent?bytePercent+' of byte limit. ':''}Retained records are not consumer backlog.`),element('p',replicas(s)));
  const config=element('details');config.open=!!configOpen;config.append(element('summary','Reported configuration'),element('pre',JSON.stringify(s.config,null,2)));summary.append(config);const browse=element('a','Browse retained messages');browse.href='#messages?stream='+encodeURIComponent(s.config.name);summary.append(browse);
  const identity={name:s.config.name,created:s.created};
  resourceCharts('stream-charts',JSON.stringify(identity),[
   {field:'messages',label:'Retained records',points:resourceSeries('stream',identity,'messages'),note:'A flat retention limit can coexist with incoming traffic.'},
   {field:'pending',label:'Largest consumer backlog',points:resourceSeries('stream',identity,'pending'),note:'Maximum pending deliveries within this stream; overlapping consumers are not summed.'},
   {field:'bytes',label:'Logical stored bytes',format:bytes,points:resourceSeries('stream',identity,'bytes')},
   {field:'last_seq',label:'Estimated append rate / second',points:resourceSeries('stream',identity,'last_seq',true),note:'Sequence progress between observations, not producer attempts. Disabled for mirrors, sources and rollups. Sequence jumps can affect this estimate.'}
  ]);
 }
 if(currentPage()==='node'){
  const n=nodes.find(n=>n.server_id===filters().get('id'));
  if(!n){empty('node-summary','Node monitoring is unavailable or stale.');$('node-charts').replaceChildren();delete $('node-charts').dataset.key;$('load-connections').disabled=true;return;}
  const nodeValid=n.status==='complete'&&Date.now()/1000-n.at<=15;$('load-connections').disabled=!nodeValid||$('load-connections').dataset.busy==='1';$('page-title').textContent=n.server_name||n.server_id;
  const identity={server_id:n.server_id,start:n.start};
  if($('node-summary').dataset.identity!==n.server_id){$('node-connections').replaceChildren();$('node-summary').dataset.identity=n.server_id;}
  $('node-summary').replaceChildren(element('h2',nodeValid?`${n.cpu??'--'}% CPU / ${bytes(n.mem)} resident RAM`:'Monitoring unavailable. Historical observations remain below.'),element('p',`NATS ${n.version} / uptime ${n.uptime} / ${number(n.connections)} connections. Observed ${new Date(n.at*1000).toLocaleTimeString()}.`),element('p',`JetStream file storage: ${bytes(n.js_storage)}${percent(n.js_storage,n.js_max_storage)?' / '+percent(n.js_storage,n.js_max_storage)+' of configured budget':''}. Memory-store bytes: ${bytes(n.js_memory)}. These are separate from process RAM.`),element('p','CPU is reported by NATS and is not container-quota utilization. Slow-consumer counts describe transport pressure, not JetStream pending delivery.'));
  resourceCharts('node-charts',JSON.stringify(identity),[
   {field:'cpu',label:'Process CPU (%)',format:v=>`${v.toFixed(1)}%`,points:resourceSeries('node',identity,'cpu')},
   {field:'mem',label:'Resident memory',format:bytes,points:resourceSeries('node',identity,'mem')},
   {field:'in_msgs',label:'Inbound messages / second',points:resourceSeries('node',identity,'in_msgs',true),note:'Native server counter delta; includes management traffic, not business throughput.'},
   {field:'out_msgs',label:'Outbound messages / second',points:resourceSeries('node',identity,'out_msgs',true)},
   {field:'api_errors',label:'JetStream API errors / second',points:resourceSeries('node',identity,'api_errors',true),note:'Includes expected management errors. Not failed application jobs.'},
   {field:'total_connections',label:'New connections / second',points:resourceSeries('node',identity,'total_connections',true),note:'Counter deltas reset across server restarts and missing observations.'}
  ]);
  $('load-connections').onclick=async()=>{const id=n.server_id;const button=$('load-connections');button.dataset.busy='1';button.disabled=true;try{const result=await api(`/api/nodes/${n.slot}/connections`);if(filters().get('id')!==id)return;const host=$('node-connections');host.replaceChildren(element('p',`${result.connections.length} of ${result.total} connections, observed ${new Date(result.at*1000).toLocaleTimeString()}`),table([['Client'],['SDK'],['RTT'],['Pending bytes',true],['Subscriptions',true]],result.connections.map(c=>[cell(c.name||`Connection ${c.cid}`),cell(`${c.lang||'unknown'} ${c.version||''}`),cell(c.rtt||'--'),cell(number(c.pending_bytes),true),cell(number(c.subscriptions),true)])));}catch(e){empty('node-connections',e.message);}finally{delete button.dataset.busy;button.disabled=false;}};
 }
}
