'use strict';
const assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path'),vm=require('node:vm');
const script=fs.readFileSync(path.join(__dirname,'../../assets/sources/gateway-source.js'),'utf8');
// Existing source variables survive an import; upgrade only the legacy Taiwan
// prefix, preserving the per-client suffix and discovery pagination metadata.
const legacy='legado-relay-1700000000000-ab12cd34ef56';
function sourceSession(site,value){let stored=value,writes=0;const ctx=vm.createContext({CF_SITE_ID:site,source:{getVariable:()=>stored,setVariable:value=>{stored=value;writes++;}}});vm.runInContext(script,ctx);return {context:ctx,get stored(){return stored;},get writes(){return writes;}};}
const routes={'https://twkan.com/last':{page:2,next:'https://twkan.com/last?page=2'}};
const migrated=sourceSession('twkan',JSON.stringify({session:legacy,routes}));
assert.equal(migrated.context.cfSession(),legacy.replace('legado-relay-','legado-twkan-'));
assert.deepEqual(JSON.parse(migrated.stored),{session:legacy.replace('legado-relay-','legado-twkan-'),routes});
assert.equal(migrated.context.cfSession(),JSON.parse(migrated.stored).session);assert.equal(migrated.writes,1);
const rawLegacy=sourceSession('twkan',legacy);assert.equal(rawLegacy.context.cfSession(),legacy.replace('legado-relay-','legado-twkan-'));assert.equal(JSON.parse(rawLegacy.stored).session,rawLegacy.context.cfSession());
for(const site of ['twkan','69shuba','banxia']){const fresh=sourceSession(site,'');assert.match(fresh.context.cfSession(),new RegExp('^legado-'+site+'-\\d+-[0-9a-f]+$'));assert.equal(fresh.context.cfSession(),JSON.parse(fresh.stored).session);assert.equal(fresh.writes,1);}
for(const [site,session] of [['twkan','legado-twkan-123-abc'],['69shuba','legado-69shuba-123-abc'],['banxia','legado-banxia-123-abc'],['twkan','my-custom-session'],['banxia',legacy]]){const existing=sourceSession(site,JSON.stringify({session,routes}));assert.equal(existing.context.cfSession(),session);assert.equal(existing.writes,0);}
let status=200,type='text/html; charset=utf-8',body='<html>actual page</html>',call;
const context=vm.createContext({CF_BASE:'http://gateway.example:19686',CF_TOKEN:'',java:{connect:(url,headers,timeout)=>{call={url,headers,timeout};return {code:()=>status,body:()=>body,headers:()=>({get:()=>type})};}}});
vm.runInContext(script,context);
context.cfUrl=url=>url;context.cfSession=()=>'reader';
assert.equal(context.cfRead('https://example.org/book'),body);
assert.equal(new URL(call.url).searchParams.get('timeoutMs'),'45000');
assert.ok(call.timeout<60000);
type=null;body='java.net.SocketTimeoutException: timeout';
assert.throws(()=>context.cfRead('https://example.org/book'),/未返回网页.*SocketTimeoutException/);
status=504;type='application/json';body=JSON.stringify({code:'TIMEOUT',error:'请求总时限已用完'});
assert.throws(()=>context.cfRead('https://example.org/book'),/TIMEOUT/);
assert.equal(context.cfCover(''),'');
const cover=context.cfCover('https://example.org/actual.jpg');
assert.equal(new URL(cover).searchParams.get('url'),'https://example.org/actual.jpg');
assert.equal(new URL(cover).searchParams.get('timeoutMs'),'45000');
assert.equal(context.cfCover(cover),cover);
console.log('Verified source contracts: site-specific session names, legacy Taiwan migration with preserved metadata, real HTML, synthetic-200 transport failure, explicit gateway timeout, exact cover URL and bounded image deadline.');
