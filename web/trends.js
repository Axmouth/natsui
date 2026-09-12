/* Historical rates require matching resource identity and consecutive evidence. */
(function(root){
 const numeric=v=>typeof v==='number'&&Number.isFinite(v)&&v>=0;
 function series(history,kind,identity,field,interval,rate=false){
  const points=[];let previous=null;
  for(const sample of history){
   const resources=sample.summary.resources;
   if(!resources&&!points.length)continue;
   const rows=kind==='stream'?resources?.streams:kind==='consumer'?resources?.consumers:resources?.monitoring?.nodes;
   const item=rows?.find(item=>kind==='consumer'?item.name===identity.name&&item.stream===identity.stream&&item.created===identity.created&&item.stream_created===identity.stream_created:kind==='stream'?item.name===identity.name&&item.created===identity.created:item.server_id===identity.server_id&&item.start===identity.start);
   const at=kind==='node'&&item?item.at:sample.at;
   const valid=item&&(kind==='node'?item.status==='complete'&&sample.at-at<=15:sample.status==='complete');
   const value=valid&&numeric(item[field])?item[field]:null;
   if(kind==='node'&&previous&&at===previous.at&&valid)continue;
   const usable=value!=null&&(!rate||kind==='node'||(kind==='consumer'?!!identity.created&&!!identity.stream_created:item.rate_eligible&&identity.created));
   let result=usable?value:null;
   if(rate){const elapsed=previous?at-previous.at:0;result=usable&&previous?.value!=null&&elapsed>0&&elapsed<=Math.max(interval,5)*3&&value>=previous.value?(value-previous.value)/elapsed:null;}
   points.push({at,value:result,label:kind==='stream'&&field==='pending'?item?.consumer:null});
   previous={at,value:usable?value:null};
  }
  return points;
 }
 function draw(host,points,{label,format=String,readout=v=>new Intl.NumberFormat(undefined,{maximumFractionDigits:2}).format(v),mini=false,interval=5,group=null,domain=null}={}){
  const doc=host.ownerDocument,ns='http://www.w3.org/2000/svg';if(group)host.dataset.syncGroup=group;else delete host.dataset.syncGroup;delete host.inspectAt;delete host.clearInspection;
  if(!points.some(p=>p.value!=null)){host.removeAttribute('tabindex');host.removeAttribute('role');host.removeAttribute('aria-label');for(const event of ['onpointermove','onpointerdown','onpointerleave','onfocus','onblur','onkeydown'])host[event]=null;delete host.dataset.inspected;host.replaceChildren(Object.assign(doc.createElement('span'),{className:'empty',textContent:mini?'Collecting history':'No historical evidence yet. Rates need two valid samples.'}));return;}
  const svg=doc.createElementNS(ns,'svg');svg.setAttribute('viewBox',mini?'0 0 150 36':'0 0 600 180');svg.setAttribute('role','img');svg.setAttribute('aria-label',label);
  const width=mini?146:540,left=mini?2:48,top=mini?2:15,bottom=mini?33:140;
  const first=domain?.from??points[0].at,last=domain?.to??points.at(-1).at,max=Math.max(1,...points.map(p=>p.value??0))*1.1;
  const x=at=>left+(at-first)/Math.max(1,last-first)*width,y=v=>bottom-(bottom-top)*v/max;
  function shape(tag,attrs,text){const e=doc.createElementNS(ns,tag);for(const[k,v]of Object.entries(attrs))e.setAttribute(k,v);if(text!=null)e.textContent=text;svg.append(e);return e;}
  if(!mini)for(let i=0;i<4;i++){const v=max*i/3;shape('line',{x1:left,x2:left+width,y1:y(v),y2:y(v),class:'grid'});shape('text',{x:0,y:y(v)+3},format(v));}
  let path='',previous=null;for(const p of points){if(p.value==null){previous=null;continue;}path+=`${!previous||p.at-previous.at>Math.max(interval,5)*3?'M':'L'}${x(p.at)} ${y(p.value)} `;previous=p;}
  shape('path',{d:path,class:'line'});
  if(!mini&&root.NatsuiWorkspace){for(const event of root.NatsuiWorkspace.chartEvents().filter(e=>e.at>=first&&e.at<=last).slice(0,40)){const marker=shape('line',{x1:x(event.at),x2:x(event.at),y1:top,y2:bottom,class:'event-marker',tabindex:0,role:'button','aria-label':`${event.kind}: ${event.detail}`});const title=doc.createElementNS(ns,'title');title.textContent=event.detail;marker.append(title);marker.onclick=e=>{e.stopPropagation();root.NatsuiWorkspace.openIncident(event);};marker.onkeydown=e=>{if(e.key==='Enter'||e.key===' '){e.preventDefault();root.NatsuiWorkspace.openIncident(event);}};}}
  const recent=points.findLast(p=>p.value!=null);shape('circle',{cx:x(recent.at),cy:y(recent.value),r:mini?2:3,class:'point'});
  host.replaceChildren(svg);
  if(!mini){shape('text',{x:left,y:169},new Date(first*1000).toLocaleTimeString());shape('text',{x:left+width,y:169,'text-anchor':'end'},new Date(last*1000).toLocaleTimeString());}
  const cursor=shape('line',{y1:top,y2:bottom,class:'inspection-cursor',visibility:'hidden'}),dot=shape('circle',{r:4,class:'inspection-point',visibility:'hidden'});
  const tip=doc.createElement('div');tip.className=mini?'history-inspector spark-inspector':'history-inspector';if(mini)tip.setAttribute('popover','manual');tip.hidden=true;tip.setAttribute('role','status');host.append(tip);
  host.tabIndex=0;host.setAttribute('role','group');host.setAttribute('aria-label',`${label}. Arrow keys inspect samples; Home, End and Escape supported.`);
  const nearest=at=>points.reduce((a,b)=>Math.abs(a.at-at)<=Math.abs(b.at-at)?a:b);
  function clear(mirrored=false){if(group&&!mirrored)doc.querySelectorAll('[data-sync-group]').forEach(peer=>{if(peer!==host&&peer.dataset.syncGroup===group)peer.clearInspection?.(true);});delete host.dataset.inspected;if(mini&&tip.matches(':popover-open'))tip.hidePopover();tip.hidden=true;cursor.setAttribute('visibility','hidden');dot.setAttribute('visibility','hidden');}
  function show(at,mirrored=false){if(group&&!mirrored)doc.querySelectorAll('[data-sync-group]').forEach(peer=>{if(peer!==host&&peer.dataset.syncGroup===group)peer.inspectAt?.(at,true);});const p=nearest(at);host.dataset.inspected=String(at);const before=points.findLast(p=>p.at<=at),after=points.find(p=>p.at>=at),gap=before&&after?after.at-before.at>Math.max(interval,5)*3:Math.abs(p.at-at)>Math.max(interval,5)*3;const cx=x(group||gap?at:p.at);cursor.setAttribute('x1',cx);cursor.setAttribute('x2',cx);cursor.setAttribute('visibility','visible');dot.setAttribute('visibility',!gap&&p.value!=null?'visible':'hidden');if(p.value!=null){dot.setAttribute('cx',x(p.at));dot.setAttribute('cy',y(p.value));}tip.hidden=false;tip.replaceChildren();for(const[tag,text]of [['time',new Date((gap?at:p.at)*1000).toLocaleString()],['strong',gap?'No samples in this interval':p.value==null?'Unavailable / insufficient evidence':readout(p.value)],['span',label],['span',p.label?`Leading consumer: ${p.label}`:'Recorded observation']]){const e=doc.createElement(tag);e.textContent=text;tip.append(e);}if(mini&&host.isConnected){if(!tip.matches(':popover-open'))tip.showPopover();const rect=host.getBoundingClientRect();tip.style.left=Math.max(8,Math.min(rect.left,doc.documentElement.clientWidth-tip.offsetWidth-8))+'px';tip.style.top=Math.max(8,rect.top-tip.offsetHeight-8)+'px';}}
  function pointer(e){const matrix=svg.getScreenCTM();if(!matrix)return;const p=svg.createSVGPoint();p.x=e.clientX;p.y=e.clientY;show(Math.max(first,Math.min(last,first+(p.matrixTransform(matrix.inverse()).x-left)/width*(last-first))));}
  host.inspectAt=show;host.clearInspection=clear;
  host.onpointermove=pointer;host.onpointerdown=e=>{host.focus({preventScroll:true});pointer(e);};host.onpointerleave=e=>{if(e.pointerType!=='touch'&&doc.activeElement!==host)clear();};host.onblur=()=>clear();host.onfocus=()=>{if(!host.dataset.inspected)show(last);};
  host.onkeydown=e=>{if(!['ArrowLeft','ArrowRight','Home','End','Escape'].includes(e.key))return;e.preventDefault();if(e.key==='Escape'){clear();return;}const index=points.indexOf(nearest(Number(host.dataset.inspected||last)));const target=e.key==='Home'?0:e.key==='End'?points.length-1:Math.max(0,Math.min(points.length-1,index+(e.key==='ArrowLeft'?-1:1)));show(points[target].at);};
  if(host.dataset.inspected)show(Math.max(first,Math.min(last,Number(host.dataset.inspected))),true);
 }
 function direction(points,interval){const last=points.at(-1);if(last?.value==null)return null;let first=last;for(let i=points.length-2;i>=0;i--){const p=points[i];if(p.value==null||first.at-p.at>Math.max(interval,5)*3||last.at-p.at>60)break;first=p;}return first.at<last.at?{rate:(last.value-first.value)/(last.at-first.at),seconds:last.at-first.at}:null;}
 root.NatsuiTrends={series,draw,direction};
})(globalThis);
