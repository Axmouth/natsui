'use strict';
(() => {
 const labels={max_msgs:['Maximum retained messages','-1 means unlimited. Lower limits can remove retained records.'],max_bytes:['Maximum stored bytes','-1 means unlimited. Values are exact bytes.'],max_age:['Maximum message age, seconds','0 disables age expiry. Fractional seconds are supported.'],max_msg_size:['Maximum message size, bytes','-1 means unlimited; payload and headers count toward this limit.'],subjects:['Captured subjects','Comma-separated subjects, for example orders.* or events.>'],discard:['At the storage limit','old removes oldest records; new rejects incoming records.'],duplicate_window:['Duplicate detection window, seconds','Positive duration for recognizing repeated message IDs.'],num_replicas:['Stream replicas','1 to 5. Cluster capacity and placement rules still apply.'],ack_wait:['Acknowledgment timeout, seconds','Positive duration. A nonempty backoff schedule overrides this value.'],max_ack_pending:['Maximum outstanding acknowledgments','-1 means unlimited. Increasing this can increase worker pressure.'],max_deliver:['Maximum delivery attempts','-1 means unlimited. Reaching the limit does not create a dead-letter queue.'],backoff:['Retry backoff, seconds','Comma-separated positive delays, for example 1, 5, 30. Empty disables backoff. Applies to acknowledgment timeouts.']};
 let capability=null, loaded=null, preview=null, generation=0, working=false, resources=[];
 const status=text=>$('editing-status').textContent=text;
 function discard(){preview=null;$('editing-review').hidden=true;$('editing-accept').checked=false;}
 function reset(){generation++;loaded=null;discard();$('editing-form').hidden=true;status('');}
 function options(){
  const kind=$('editing-kind').value;
  resources=(kind==='stream'?data?.snapshot?.streams:data?.snapshot?.consumers)||[];
  const select=$('editing-resource');select.replaceChildren();
  resources.forEach((r,i)=>{const o=element('option',kind==='stream'?r.config.name:`${r.stream_name} / ${r.name}`);o.value=String(i);select.append(o);});
  $('editing-load').disabled=working||!resources.length||staticDemo||data?.snapshot?.demo===true;
 }
 async function capabilities(){
  if(staticDemo){capability={enabled:false,demo:true};}else{capability=await api('/api/editing');}
  $('editing-access').textContent=capability.demo?'Simulation: configuration editing is available only with a real NATS connection.':capability.enabled?`Write access enabled for ${capability.profile}. Each update requires a fresh preview; native NATS permissions still apply.`:'Read-only connection. To enable reviewed edits, start the dashboard with NATSUI_ALLOW_WRITES=1 and a NATS identity permitted to update the selected resources.';
  if(capability.enabled)$('footer-scope').textContent='Local workspace / reviewed broker edits enabled';
 }
 function busy(value){working=value;['editing-kind','editing-resource','editing-load','editing-preview','editing-apply','editing-cancel'].forEach(id=>$(id).disabled=value);$('editing-fields').querySelectorAll('input,select').forEach(input=>input.disabled=value||!capability?.enabled||!loaded?.supported);}
 async function load(){
  reset(); const seq=generation; const kind=$('editing-kind').value;const r=resources[Number($('editing-resource').value)];if(!r)return;
  const target={kind,stream:kind==='stream'?r.config.name:r.stream_name,...(kind==='consumer'?{consumer:r.name}:{})};
  busy(true);status('Reading current configuration from NATS...');
  try{
   await capabilities(); const result=await api('/api/config?'+new URLSearchParams(target));if(seq!==generation)return;
   loaded={...result,target};const fields=$('editing-fields');fields.replaceChildren();
   Object.keys(labels).filter(key=>key in result.fields).map(key=>[key,result.fields[key]]).forEach(([key,value])=>{const label=element('label');label.append(element('span',labels[key][0]));let input;
    if(key==='discard'){input=element('select');['old','new'].forEach(v=>{const o=element('option',v==='old'?'Remove oldest records':'Reject incoming records');o.value=v;input.append(o);});}
    else{input=element('input');input.type='text';input.autocomplete='off';input.maxLength=8192;}
    input.name=key;input.value=value;input.disabled=!capability.enabled||!result.supported;label.append(input,element('small',labels[key][1]));fields.append(label);
   });
   $('editing-form').hidden=false;status(result.supported?`Current settings loaded from NATS ${result.version}. Only changed fields will be submitted.`:`NATS ${result.version}: editing requires version 2.11 or later in the 2.x series.`);
  }catch(e){status(e.message);}finally{busy(false);$('editing-preview').disabled=!loaded?.supported||!capability?.enabled;}
 }
 $('editing-kind').onchange=()=>{reset();options();};$('editing-resource').onchange=reset;$('editing-load').onclick=load;
 $('editing-form').oninput=discard;
 $('editing-form').onsubmit=async e=>{
  e.preventDefault();if(!loaded||working)return;discard();const changes={};
  for(const [key,value] of new FormData(e.currentTarget))if(value.trim()!==loaded.fields[key])changes[key]=value.trim();
  if(!Object.keys(changes).length){status('No changes to review.');return;}
  busy(true);status('Preparing preview...');
  try{preview=await api('/api/config/preview',{method:'POST',headers:{'Content-Type':'application/json','X-Natsui-Request':'1'},body:JSON.stringify({target:loaded.target,revision:loaded.revision,changes})});
   const table=element('table');const head=element('thead');const hr=element('tr');['Setting','Current','Proposed'].forEach(t=>hr.append(element('th',t)));head.append(hr);const body=element('tbody');
   for(const [key,change] of Object.entries(preview.changes)){const row=element('tr');[labels[key][0],change.before||'(empty)',change.after||'(empty)'].forEach(t=>row.append(element('td',t)));body.append(row);}table.append(head,body);$('editing-diff').replaceChildren(table);
   $('editing-target').textContent=`${loaded.target.stream}${loaded.target.consumer?' / '+loaded.target.consumer:''} - preview valid for 2 minutes.`;
   $('editing-warnings').replaceChildren(...preview.warnings.map(w=>element('p',w,'editing-warning')));$('editing-accept').closest('label').hidden=!preview.warnings.length;
   $('editing-review').hidden=false;status('Review the changes before applying.');$('editing-review').scrollIntoView({block:'nearest',behavior:'smooth'});
  }catch(e){status(e.message);}finally{busy(false);}
 };
 $('editing-cancel').onclick=()=>{discard();status('Preview cancelled. No update sent.');};
 $('editing-apply').onclick=async()=>{
  if(!preview||working)return;if(preview.warnings.length&&!$('editing-accept').checked){status('Acknowledge the listed effects before applying.');return;}
  const token=preview.token;busy(true);status('Applying and reading back from NATS...');
  try{const result=await api('/api/config/apply',{method:'POST',headers:{'Content-Type':'application/json','X-Natsui-Request':'1'},body:JSON.stringify({token,accept_warnings:$('editing-accept').checked})});
   reset();status(`${result.outcome}.${result.audit_saved?' Recorded in Activity.':' Outcome could not be saved; the attempted change remains recorded.'} Load current settings before making another change.`);
  }catch(e){reset();status(`${e.message} Load current settings before retrying; no automatic retry was sent.`);}finally{busy(false);}
 };
 async function enter(){if(currentPage()!=='settings')return;if(!working&&!loaded)options();try{await capabilities();}catch(e){status(e.message);}}
 window.addEventListener('hashchange',enter);
 // Resource choices are refreshed only before loading a form, preserving unsaved edits during polling.
 const ready=setInterval(()=>{if(!data)return;clearInterval(ready);enter();},250);
 enter();
})();
