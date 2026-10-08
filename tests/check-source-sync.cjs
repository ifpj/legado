// Run with node. Simulate native callbacks, initialization races, and batch gaps.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const path = require('node:path');
const saved = new Map(), memory = new Map(), requests = [], cached = [];
class IdentityHashMap {
    constructor(){this.values=new Map();}
    get(k){return this.values.get(k);}
    put(k,v){this.values.set(k,v);}
    size(){return this.values.size;}
    clear(){this.values.clear();}
}
const context = {
    Packages: {java:{util:{IdentityHashMap}}},
    sourceApi: {getLoginInfo: () => JSON.stringify({uid:'test',ttToken:'test'})},
    cache: {get:k=>saved.get(k),put:(k,v)=>saved.set(k,v),getFromMemory:k=>memory.get(k),putMemory:(k,v)=>memory.set(k,v)},
    java: {lock:(_k,f)=>f(),toast:()=>{},log:()=>{},cacheContent:(c,s)=>cached.push([c.url,s])},
};
vm.createContext(context);
vm.runInContext(fs.readFileSync(path.resolve(__dirname,'../source/fanqie.js'),'utf8'),context);
let cloud = {itemId:'103',chapterIndex:3,chapterTitle:'three',timestampMs:1000};
let fail = false;
let failGet = false;
let delayed = false;
let accepted = null;
context.relayCall = (operation,args) => {
    requests.push({operation,args});
    if(operation==='progress_get'){if(failGet)throw Error('network failure');return JSON.stringify({position:cloud});}
    if(operation==='progress_put'){
        if(fail)throw Error('network failure');
        if(delayed){
            accepted={itemId:args.chapter.url.split('/').pop(),chapterIndex:args.chapterIndex,timestampMs:args.timestampMs};
            return JSON.stringify({status:'accepted',position:accepted,verification:{status:'pending'}});
        }
        cloud={itemId:args.chapter.url.split('/').pop(),chapterIndex:args.chapterIndex,timestampMs:args.timestampMs};
        return JSON.stringify({status:'uploaded',position:cloud});
    }
    if(operation==='content_batch')return JSON.stringify({chapters:[{url:args.chapters[0].url,content:'one'},{url:args.chapters[2].url,content:'three'}],missing:[args.chapters[1]]});
    throw Error('unexpected operation');
};
let dbIndex=-1;
const makeBook=index=>({bookUrl:'https://fanqienovel.com/page/1',variable:'{}',durChapterIndex:index,totalChapterNum:20,save(){dbIndex=this.durChapterIndex;}});
let book=makeBook(0);
const chapter=index=>({index,url:'https://fanqienovel.com/reader/'+(100+index)});
const event=(name,index,time=Date.now()+10)=>context.relayOnEvent(name,book,chapter(index),String(time));
event('saveRead',0);
assert.equal(requests.filter(r=>r.operation==='progress_put').length,0,'initialization must not upload');
assert.equal(dbIndex,3,'first SAVE_READ must pull when START_READ is absent');
event('startRead',0);
assert.equal(requests.length,1,'late START_READ must not repeat initialization');
assert.equal(dbIndex,3,'cloud chapter persisted');
book.durChapterIndex=0;
event('saveRead',0,1);
assert.equal(dbIndex,3,'late initialization save must not overwrite cloud');
event('endRead',0);
book.durChapterIndex=0;
event('saveRead',0);
assert.equal(dbIndex,3,'save after END_READ must preserve pending position');
const previousBook=book;
book=makeBook(3);
event('saveRead',3);
const active=JSON.parse([...memory.entries()].find(([k])=>k.endsWith('.active'))[1]);
assert.equal(active.open,true,'same-book reentry must open a session without START_READ');
event('startRead',3);
event('saveRead',3);
assert.equal(requests.filter(r=>r.operation==='progress_put').length,0,'remote application must not echo');
event('saveRead',4);
assert.equal(cloud.chapterIndex,4,'genuine chapter change uploaded');
fail=true;
event('saveRead',5);
assert.ok(JSON.parse([...saved.values()][0]).outbox,'failed upload retained as outbox');
fail=false;
const failedBook=book;
book=makeBook(5);
event('startRead',5);
assert.equal(cloud.chapterIndex,5,'outbox retried on next open');
const countBeforeOld=requests.length;
context.relayOnEvent('saveRead',previousBook,chapter(9),String(Date.now()+10));
context.relayOnEvent('startRead',failedBook,chapter(5),null);
assert.equal(requests.length,countBeforeOld,'callbacks from older Book objects must not reopen or upload');
event('endRead',6);
assert.equal(cloud.chapterIndex,6,'END_READ must upload a last chapter change before a delayed SAVE_READ');
book=makeBook(6);
event('startRead',6);
failGet=true;
event('endRead',6);
const writesBefore=requests.filter(r=>r.operation==='progress_put').length;
event('saveRead',7);
assert.equal(requests.filter(r=>r.operation==='progress_put').length,writesBefore,'failed END_READ must still close the active reader');
failGet=false;
book=makeBook(6);
event('startRead',6);
delayed=true;
event('saveRead',7);
const state=()=>JSON.parse(saved.get(context.relaySyncKey({uid:'test'},book)));
assert.equal(state().outbox,null,'accepted writes must not remain in the failed-write outbox');
assert.equal(state().confirmation.itemId,'107','keep a receipt until the cloud read catches up');
const pendingTimestamp=state().baseTimestampMs;
const pendingWrites=requests.filter(r=>r.operation==='progress_put').length;
event('endRead',7);
assert.equal(state().baseTimestampMs,pendingTimestamp,'stale reads must not lower the accepted base');
assert.equal(state().pending,null,'stale cloud position must not rewind the reader');
assert.equal(requests.filter(r=>r.operation==='progress_put').length,pendingWrites,'exit must not retry accepted writes');
book=makeBook(7);
event('startRead',7);
assert.equal(state().confirmation.itemId,'107','reopening must retain an unconfirmed receipt');
assert.equal(state().pending,null,'reopening must not apply stale cloud position');
cloud=accepted;
event('saveRead',8,pendingTimestamp+1);
assert.equal(requests.at(-1).args.baseTimestampMs,pendingTimestamp,'next chapter uses the accepted write as its base');
cloud=accepted;
event('endRead',8);
assert.equal(state().confirmation,null,'a fresh read confirms the accepted write');
assert.equal(state().pending,null);
book=makeBook(8);
event('startRead',8);
event('saveRead',9,pendingTimestamp+2);
const receiptTime=state().confirmation.timestampMs;
cloud={itemId:'110',chapterIndex:10,timestampMs:receiptTime+1};
event('endRead',9);
assert.equal(state().confirmation,null,'a genuinely newer cloud change supersedes the receipt');
assert.equal(state().pending.chapterIndex,10,'other-device updates remain applicable');
delayed=false;
// An accepted first write may also be followed by an empty stale read.
cloud=null;
book={...makeBook(0),bookUrl:'https://fanqienovel.com/page/2'};
event('startRead',0);
delayed=true;
event('saveRead',1);
const emptyWrites=requests.filter(r=>r.operation==='progress_put').length;
event('endRead',1);
assert.equal(requests.filter(r=>r.operation==='progress_put').length,emptyWrites,'empty read must not resend an accepted first write');
assert.ok(state().confirmation);
const list=[chapter(1),chapter(2),chapter(3)];
context.getContentBatch({size:()=>list.length,get:i=>list[i]},book);
assert.equal(requests.at(-1).args.native,true,'batch requests must avoid duplicate encrypted metadata in Rhino');
assert.deepEqual(cached,list.filter((_,i)=>i!==1).map((c,i)=>[c.url,i===0?'one':'three']),'only returned chapters may be cached');
console.log('Source checks passed: initialization, stale callbacks, close race, outbox, delayed confirmation, empty reads, newer cloud updates, partial batch.');
