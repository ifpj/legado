// CI only: actual container, local upstream, no live websites.
'use strict';
const assert=require('node:assert/strict'),http=require('node:http');
const base=process.env.CF_TEST_BASE||'http://127.0.0.1:19686',token=process.env.CF_TEST_TOKEN||'';
const headers=token?{Authorization:'Bearer '+token}:{};
const server=http.createServer((req,res)=>{
  res.writeHead(req.url==='/missing'?404:200,{'Content-Type':'text/html; charset=utf-8'});
  res.end('<html><title>本地验收</title><body>'+ (req.url==='/missing'?'网站实际的 404 页面':'每次取页 '+Date.now())+'</body></html>');
});
(async()=>{
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
  const upstream='http://127.0.0.1:'+server.address().port;
  for(let i=0;;i++){
    try{const r=await fetch(base+'/v1/status',{headers});assert.equal(r.status,200);break;}
    catch(e){if(i===29)throw e;await new Promise(r=>setTimeout(r,500));}
  }
  const index=await fetch(base+'/');assert.equal(index.status,200);assert.match(await index.text(),/接入示例/);
  assert.equal((await fetch(base+'/logo.png')).headers.get('content-type'),'image/png');
  const status=await (await fetch(base+'/v1/status',{headers})).json();
  assert.equal(status.runtime.language,'Rust');assert.equal(status.runtime.python,null);assert.ok(status.runtime.rssBytes>0);
  if(token){assert.equal((await fetch(base+'/v1/status')).status,401);assert.equal((await fetch(base+'/v1/history?token='+token)).status,401);}
  for(const site of ['twkan','69shuba','banxia']){
    const q=new URLSearchParams({site,base:'https://reader.example:8443/gateway',...(token?{token}:{})});
    const r=await fetch(base+'/source.js?'+q);assert.equal(r.status,200);
    const script=await r.text();assert.match(script,/var CF_BASE = "https:\/\/reader.example:8443\/gateway"/);
    assert.match(script,/java.t2s/);assert.match(script,/cfDiscovery/);
  }
  const value=await (await fetch(base+'/v1/fetch',{method:'POST',headers:{...headers,'Content-Type':'application/json'},body:JSON.stringify({url:upstream+'/fresh?test=1',session:'container-smoke'})})).json();
  assert.equal(value.ok,true);assert.match(value.body,/每次取页/);assert.equal(value.contentCached,false);
  const raw=await fetch(base+'/fetch?'+new URLSearchParams({url:upstream+'/missing'}),{headers});assert.equal(raw.status,404);assert.match(await raw.text(),/网站实际/);
  const history=await (await fetch(base+'/v1/history',{headers})).json();
  assert.ok(history.items.some(x=>x.status===404));assert.ok(history.items.some(x=>x.request.url===upstream+'/fresh?test=1'));assert.equal(history.persistent,false);
  console.log('Verified server: Web, logo, Rust runtime, source exports, auth, local fetch, HTTP status and replay history.');
})().catch(e=>{console.error(e);process.exitCode=1;}).finally(()=>server.close());
