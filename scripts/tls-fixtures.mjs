import {mkdtempSync, writeFileSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {execFileSync} from 'node:child_process';

// Certificates and private keys exist only in a disposable test directory.
const dir=mkdtempSync(join(tmpdir(),'natsui-tls-'));
const openssl=process.env.NATSUI_OPENSSL||'openssl';
writeFileSync(join(dir,'openssl.cnf'),'[req]\ndistinguished_name=dn\n[dn]\n');
const run=(...args)=>execFileSync(openssl,args,{cwd:dir,env:{...process.env,OPENSSL_CONF:join(dir,'openssl.cnf')},stdio:['ignore','ignore','pipe']});
run('req','-x509','-newkey','rsa:2048','-nodes','-keyout','ca.key','-out','ca.pem','-days','2','-subj','/CN=Natsui disposable test CA','-addext','basicConstraints=critical,CA:TRUE','-addext','keyUsage=critical,keyCertSign,cRLSign');
for(const name of ['server','client']) {
 run('req','-newkey','rsa:2048','-nodes','-keyout',`${name}.key`,'-out',`${name}.csr`,'-subj',`/CN=${name==='server'?'localhost':'natsui-test'}`);
 writeFileSync(join(dir,`${name}.ext`),name==='server'?'subjectAltName=DNS:localhost,IP:127.0.0.1\nextendedKeyUsage=serverAuth\n':'extendedKeyUsage=clientAuth\n');
 run('x509','-req','-in',`${name}.csr`,'-CA','ca.pem','-CAkey','ca.key','-CAcreateserial','-out',`${name}.pem`,'-days','2','-extfile',`${name}.ext`);
}
console.log(dir);
