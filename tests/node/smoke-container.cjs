// CI only: actual container, local upstream, no live websites.
'use strict';
const assert=require('node:assert/strict'),http=require('node:http');
const base=process.env.CF_TEST_BASE||'http://127.0.0.1:19686',token=process.env.CF_TEST_TOKEN||'';
const headers=token?{Authorization:'Bearer '+token}:{};
const pixel=Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aU1sAAAAASUVORK5CYII=','base64');
const server=http.createServer((req,res)=>{
  if(req.url==='/cover'){res.writeHead(200,{'Content-Type':'image/png'});return res.end(pixel);}
  res.writeHead(req.url==='/missing'?404:200,{'Content-Type':'text/html; charset=utf-8'});
  res.end('<html><title>本地验收</title><body>'+ (req.url==='/missing'?'网站实际的 404 页面':'每次取页 '+Date.now())+'</body></html>');
});
(async()=>{
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
  const upstream='http://127.0.0.1:'+server.address().port;
  for(let i=0;;i++){
    try{const r=await fetch(base+'/v1/status',{headers});await r.arrayBuffer();assert.equal(r.status,200);break;}
    catch(e){if(i===29)throw e;await new Promise(r=>setTimeout(r,500));}
  }
  const index=await fetch(base+'/');assert.equal(index.status,200);const html=await index.text();assert.match(html,/接入示例/);
  const logo=await fetch(base+'/logo.png');assert.equal(logo.headers.get('content-type'),'image/png');await logo.arrayBuffer();
  const unknown=await fetch(base+'/nope');assert.equal(unknown.status,404);assert.equal((await unknown.json()).code,'NOT_FOUND');
  const status=await (await fetch(base+'/v1/status',{headers})).json();
  assert.equal(status.build,status.buildId);assert.ok(status.build.startsWith(status.version));assert.equal(status.dirty,false);assert.match(status.binaryHash,/^[a-f0-9]{12}$/);assert.ok(html.includes('>'+status.build+'<'));
  assert.equal(status.runtime.language,'Rust');assert.equal(status.runtime.python,null);assert.ok(status.runtime.rssBytes>0);
  assert.equal(status.runtime.networkMode,process.env.CF_TEST_NETWORK_MODE||'direct');assert.match(html,/id="network-mode"/);assert.equal(status.runtime.dnsMode,process.env.CF_TEST_DNS_MODE||'system');
  if(token){for(const path of ['/v1/status','/v1/history?token='+token]){const denied=await fetch(base+path);await denied.arrayBuffer();assert.equal(denied.status,401);}}
  for(const site of ['twkan','69shuba','banxia']){
    const q=new URLSearchParams({site,base:'https://reader.example:8443/gateway',...(token?{token}:{})});
    const r=await fetch(base+'/source.js?'+q);assert.equal(r.status,200);
    const script=await r.text();assert.match(script,/var CF_BASE = "https:\/\/reader.example:8443\/gateway"/);
    assert.match(script,/java.t2s/);assert.match(script,/cfDiscovery/);
  }
  const bundleQuery=new URLSearchParams({base:'https://reader.example:8443/gateway',...(token?{token}:{})});
  const bundleResponse=await fetch(base+'/sources.json?'+bundleQuery);assert.equal(bundleResponse.status,200);
  const bundle=await bundleResponse.json();assert.equal(bundle.length,3);assert.equal(new Set(bundle.map(s=>s.bookSourceUrl)).size,3);
  for(const source of bundle){assert.match(source.mainJs,/function cfIntro\(/);assert.match(source.mainJs,/var CF_BASE = "https:\/\/reader.example:8443\/gateway"/);}
  const value=await (await fetch(base+'/v1/fetch',{method:'POST',headers:{...headers,'Content-Type':'application/json'},body:JSON.stringify({url:upstream+'/fresh?test=1',session:'container-smoke'})})).json();
  assert.equal(value.ok,true);assert.match(value.body,/每次取页/);assert.equal(value.contentCached,false);
  const inventory=await (await fetch(base+'/v1/status',{headers})).json();
  const item=inventory.sessionDetails.find(s=>s.session==='container-smoke'&&s.origin===upstream);
  assert.ok(item);assert.equal(item.verified,true);assert.equal(item.hasProxy,false);assert.ok(item.lastUsedAtMs>0);
  assert.equal(inventory.sessionDetails.length,inventory.runtime.sessions);
  const raw=await fetch(base+'/fetch?'+new URLSearchParams({url:upstream+'/missing'}),{headers});assert.equal(raw.status,404);assert.match(await raw.text(),/网站实际/);
  const image=await fetch(base+'/image?'+new URLSearchParams({url:upstream+'/cover',...(token?{token}:{})}));assert.equal(image.status,200);assert.equal(image.headers.get('content-type'),'image/png');assert.deepEqual(Buffer.from(await image.arrayBuffer()),pixel);
  const history=await (await fetch(base+'/v1/history',{headers})).json();
  assert.ok(history.items.some(x=>x.status===404));assert.ok(history.items.some(x=>x.request.url===upstream+'/fresh?test=1'));assert.equal(history.persistent,false);
  console.log('Verified server: Web, logo, Rust runtime, source exports, auth, local fetch, HTTP status and replay history.');
})().catch(e=>{console.error(e);process.exitCode=1;}).finally(()=>{server.closeAllConnections();server.close();});
