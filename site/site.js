'use strict';
for(const button of document.querySelectorAll('[data-copy]'))button.addEventListener('click',async()=>{
 const text=document.getElementById(button.dataset.copy).textContent;
 try{await navigator.clipboard.writeText(text);button.textContent='Copied';document.getElementById('copy-status').textContent='Docker command copied.';setTimeout(()=>button.textContent='Copy command',2000);}
 catch{const range=document.createRange();range.selectNodeContents(document.getElementById(button.dataset.copy));const selection=window.getSelection();selection.removeAllRanges();selection.addRange(range);document.getElementById('copy-status').textContent='Command selected. Use the browser copy command.';}
});
