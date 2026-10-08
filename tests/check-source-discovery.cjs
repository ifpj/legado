const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm'),path=require('node:path');
const saved=new Map(),calls=[];
const context={sourceApi:{getLoginInfo:()=>JSON.stringify({uid:'test',ttToken:'test'})},source:{bookSourceName:'test',bookSourceUrl:'test'},cache:{get:k=>saved.get(k),put:(k,v)=>saved.set(k,v)},java:{toast(){},log(){},showBrowser(...args){calls.push({browser:args});},searchBook(...args){calls.push({search:args});}}};
vm.createContext(context);vm.runInContext(fs.readFileSync(path.resolve(__dirname,'../source/fanqie.js'),'utf8'),context);
context.relayCall=(operation,args)=>{calls.push({operation,args});return JSON.stringify(operation==='review_summary'?[{paraIndex:1,count:2}]:operation==='discovery_menu'?[{title:'test',url:'fanqie://home'}]:{items:[]});};
const book={bookUrl:'https://fanqienovel.com/page/123',name:'<test>'},chapter={url:'https://fanqienovel.com/reader/456'};
assert.equal(context.getReviewSummary(chapter,book),'[]');assert.equal(calls.length,0,'disabled reviews add no per-chapter requests');
context.relayOnEvent('addBookShelf',book,chapter);assert.equal(calls.length,0,'shelf syncing defaults off');
context.loginAction('preferences',{}, {gender:'女生',words:'100万以上',status:'完结',sort:'高分',nativeReviews:'true',shelfSync:'仅加入'});
const prefs=JSON.parse([...saved.values()][0]);assert.equal(prefs.gender,'0');assert.equal(prefs.words,'word_num_gte100');assert.equal(prefs.shelfSync,'add');
assert.equal(JSON.parse(context.getReviewSummary(chapter,book))[0].paraIndex,1);
context.getReviewDetail(chapter,book,-1,'{"paraIndex":0,"version":"7"}',2);assert.equal(calls.at(-1).args.version,'7');assert.equal(calls.at(-1).args.paraIndex,0);
context.relayOnEvent('delBookShelf',book,chapter);assert.notEqual(calls.at(-1).operation,'shelf_change');
context.relayOnEvent('addBookShelf',book,chapter);assert.equal(calls.at(-1).args.action,'add');
assert.equal(context.relayOnEvent('clickCustomButton',book,chapter),true);assert.equal(calls.at(-1).browser[1],null);assert.match(calls.at(-1).browser[0],/bookId=123&chapterId=456/);
assert.equal(calls.at(-1).browser[2],'window.fanqieRun=run;','BottomWebViewDialog only injects its run bridge when preloadJs is supplied');
console.log('Discovery source checks passed: preferences, no default extra HTTP, shelf modes, paragraph/version mapping, native browser URL.');

// Run the self-contained button against an older mainJs: users should not need
// to update their source before they can use the new update entry.
const updateAction=fs.readFileSync(path.resolve(__dirname,'../source/update-source.js'),'utf8');
const legacyMainJs=fs.readFileSync(path.resolve(__dirname,'../source/fanqie.js'),'utf8');
const pages=[];
context.java.startBrowser=(url,title)=>pages.push({url,title});
context.java.downloadFile=()=>{throw new Error('Update must only open the web console');};
context.java.openUrl=()=>{throw new Error('Import is initiated by the user on the web page');};
for(const [base,embeddedToken,cachedToken] of [
    ['http://relay.lan:122','',''],
    ['https://books.example:8443','embedded-token','unused-cache-token'],
    ['http://[2001:db8::1]:19670','','cache-token']
]){
    saved.set('fanqie.rust.service.token',cachedToken);
    context.source.mainJs=legacyMainJs+'\nRELAY_URL='+JSON.stringify(base)+';RELAY_TOKEN='+JSON.stringify(embeddedToken)+';';
    const before=[...saved.entries()];
    vm.runInContext(updateAction,context);
    const {url:address,title}=pages.at(-1),url=new URL(address);
    assert.equal(url.origin,base);
    assert.equal(url.pathname,'/');assert.equal(url.hash,'#connect');
    assert.equal(url.search,'','credentials are not passed in the page URL');
    assert.equal(title,'更新番茄书源');
    assert.deepEqual([...saved.entries()],before,'opening the page preserves login/device/preferences state');
}
assert.equal(pages.length,3);
console.log('Source update checks passed: old mainJs, host/HTTPS/IPv6, direct web import page, no file download/import, preserved state.');
