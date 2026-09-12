/* Match literal subjects against configured NATS filters, not delivery state. */
(function(root){
 function valid(filter){const tokens=filter.split('.');return filter.length>0&&filter.length<=1024&&!/[\s\x00-\x1f\x7f]/.test(filter)&&tokens.every((t,i)=>t&&(t==='*'||(t==='>'&&i===tokens.length-1)||!/[*>]/.test(t)));}
 function matches(filter,subject){if(!valid(filter)||!subject||/[*>]/.test(subject))return false;const parts=subject.split('.'),tokens=filter.split('.');for(let i=0;i<tokens.length;i++){if(i>=parts.length)return false;if(tokens[i]==='>')return true;if(tokens[i]!=='*'&&tokens[i]!==parts[i])return false;}return tokens.length===parts.length;}
 function filters(consumer){return consumer.config?.filter_subjects?.length?consumer.config.filter_subjects:consumer.config?.filter_subject?[consumer.config.filter_subject]:['>'];}
 root.NatsuiSubjects={valid,matches,filters};
})(globalThis);
