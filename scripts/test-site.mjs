import assert from 'node:assert/strict';
import {readFile,stat} from 'node:fs/promises';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import './build-site.mjs';
const root=path.resolve(fileURLToPath(new URL('../dist-site/',import.meta.url)));
for(const name of ['index.html','demo/index.html']){
 const file=path.join(root,name),html=await readFile(file,'utf8');
 for(const [,url] of html.matchAll(/(?:src|href)="([^"]+)"/g)){
  if(/^(https?:|#)/.test(url))continue;
  assert.ok(!url.startsWith('/'),`${name}: absolute asset path would escape the Pages project`);
  let target=path.resolve(path.dirname(file),url.split('#')[0]);if(url.endsWith('/'))target=path.join(target,'index.html');
  assert.ok((await stat(target)).isFile(),`${name}: missing ${url}`);
 }
}
const html=await readFile(path.join(root,'demo/index.html'),'utf8');
assert.ok(html.indexOf('runtime.js')<html.indexOf('demo.js'));
assert.ok(html.includes("connect-src 'none'"));
const compose=await readFile(path.join(root,'try/compose.yaml'),'utf8');
assert.ok(!/^\s+build:/m.test(compose));assert.ok(!compose.includes('docker.sock'));assert.ok(compose.includes('127.0.0.1:'));
globalThis.fetch=()=>{throw new Error('The simulation must not use a network request');};
await import('../web/subjects.js');await import('../web/demo.js');
for(const uri of ['/api/snapshot','/api/history','/api/incidents','/api/activity','/api/records/ORDERS','/api/latest/ORDERS'])assert.ok(await globalThis.NatsuiDemo.api(uri));
console.log('Pages subpath links, embedded assets, static network isolation and demo API routes passed.');
