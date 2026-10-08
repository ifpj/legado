const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm'),path=require('node:path');
const code=fs.readFileSync(path.resolve(__dirname,'../web/community.js'),'utf8');
function page(chapter='456'){
    const elements=new Map(),pending=[];
    function node(tag='div'){
        const n={tag,children:[],value:'',textContent:'',dataset:{},classList:{toggle(){}},append(...x){this.children.push(...x);},replaceChildren(...x){this.children=x;},click(){}};
        Object.defineProperty(n,'options',{get(){return this.children;}});return n;
    }
    const element=id=>{if(!elements.has(id))elements.set(id,node());return elements.get(id);};
    const tabs=['book','chapter','paragraph'].map(scope=>{const n=node('button');n.dataset.scope=scope;return n;});
    const sorts=['1','3'].map(sort=>{const n=node('button');n.dataset.sort=sort;return n;});
    element('sort').value='1';
    const context=vm.createContext({URL,URLSearchParams,location:new URL('http://relay/community'),performance:{now:()=>1},window:{},sessionStorage:{getItem(){}},document:{getElementById:element,querySelectorAll:s=>s==='[data-scope]'?tabs:sorts,createElement:node},setTimeout(){}});
    vm.runInContext(code,context);
    context.initialCall=vm.runInContext('call',context);
    context.mockCall=(operation,args)=>new Promise((resolve,reject)=>pending.push({operation,args:JSON.parse(JSON.stringify(args)),resolve,reject}));
    vm.runInContext('call=mockCall',context);element('book').value='123';element('chapter').value=chapter;
    vm.runInContext('availability()',context);
    const run=expression=>vm.runInContext(expression,context);
    return {element,tabs,sorts,pending,run,context};
}
const result=(text='comment',cursor='opaque',hasMore=true)=>({items:[{id:'comment:789',name:'reader',content:{text,replyCount:2}}],cursor,hasMore,raw:{untouched:true}});
const tick=()=>new Promise(resolve=>setImmediate(resolve));
function find(n,text){if(n.textContent===text)return n;for(const c of n.children||[]){const found=find(c,text);if(found)return found;}}
(async()=>{
    const ui=page();let task=ui.run('load(true)');
    assert.equal(ui.pending[0].args.sort,1);assert.equal('cursor' in ui.pending[0].args,false);
    ui.pending[0].resolve(result());await task;
    task=ui.element('next').onclick();assert.equal(ui.pending[1].args.page,2);assert.equal(ui.pending[1].args.cursor,'opaque');
    ui.pending[1].reject(Error('temporary failure'));await task;
    assert.equal(ui.run('state.page'),1);assert.equal(ui.element('next').disabled,false);
    task=ui.element('next').onclick();assert.equal(ui.pending[2].args.page,2);ui.pending[2].resolve(result('page two','next'));await task;
    assert.equal(ui.run('state.page'),2);

    const old=ui.run('load(true)');const latest=ui.sorts[1].onclick();
    assert.equal(ui.pending[4].args.sort,3);
    ui.pending[4].resolve(result('latest','new'));await latest;
    ui.pending[3].resolve(result('stale','old'));await old;
    assert.equal(ui.run('state.result.items[0].content.text'),'latest');assert.equal(ui.run('state.cursor'),'new');
    assert.equal(ui.element('refresh').disabled,false);

    const reply=find(ui.element('items'),'查看回复');ui.element('book').value='999';
    task=reply.onclick();assert.equal(ui.pending[5].args.book.bookUrl,'123');assert.equal(ui.pending[5].args.sort,3);
    assert.equal('cursor' in ui.pending[5].args,false);ui.pending[5].resolve({...result('reply','reply-next'),items:[]});await task;
    task=reply.onclick();assert.equal(ui.pending[6].args.page,2);assert.equal(ui.pending[6].args.cursor,'reply-next');
    ui.pending[6].reject(Error('retry replies'));await task;
    task=reply.onclick();assert.equal(ui.pending[7].args.page,2);ui.pending[7].resolve({...result(),hasMore:false});await task;
    assert.equal(reply.hidden,true);

    task=ui.tabs[1].onclick();assert.equal(ui.element('items').children.length,0);assert.equal(ui.element('raw').textContent,'');
    assert.equal(ui.pending[8].args.scope,'chapter');ui.pending[8].reject(Error('chapter failure'));await task;
    assert.equal(ui.run('state.result'),null);assert.equal(ui.element('next').disabled,true);
    assert.equal(ui.element('download').disabled,true);

    const missing=page('');missing.run('availability()');assert.equal(missing.tabs[1].disabled,true);assert.equal(missing.tabs[2].disabled,true);
    assert.equal(missing.element('chapter-hint').hidden,false);
    const para=page();task=para.tabs[2].onclick();assert.equal(para.pending[0].operation,'review_summary');
    para.pending[0].resolve({items:[{paraData:'{"paraIndex":7,"version":"current"}',count:9}],raw:{newest_item_version:'current'}});await tick();
    assert.equal(para.pending[1].args.paraIndex,7);assert.equal(para.pending[1].args.version,'current');
    para.pending[1].resolve(result());await task;
    assert.equal(para.element('para').options[0].textContent,'第 7 段 · 9 条');
    assert.match(para.element('raw').textContent,/newest_item_version/);
    task=find(para.element('items'),'查看回复').onclick();assert.equal(para.pending[2].args.version,'current');
    para.pending[2].resolve({...result(),items:[],hasMore:false});await task;
    para.element('para-label').onclick();assert.equal(para.element('para').hidden,false);
    task=para.element('para').children[0].onclick();assert.equal(para.element('para').hidden,true);
    assert.equal(para.pending[3].operation,'review_summary');
    para.pending[3].resolve({items:[{paraData:'{"paraIndex":7,"version":"fresh"}',count:10}],raw:{newest_item_version:'fresh'}});await tick();
    assert.equal(para.pending[4].args.paraIndex,7);assert.equal(para.pending[4].args.version,'fresh');
    para.pending[4].resolve(result());await task;

    const race=page();const book=race.run('load(true)'),chapter=race.tabs[1].onclick();
    assert.equal(race.pending[1].args.scope,'chapter');race.pending[1].resolve(result('chapter'));await chapter;
    race.pending[0].resolve(result('book'));await book;assert.equal(race.run('state.result.items[0].content.text'),'chapter');
    const html=fs.readFileSync(path.resolve(__dirname,'../web/community.html'),'utf8');
    assert.match(html,/data-sort="1"[^>]*>热门<\/button>/);assert.match(html,/data-sort="3"[^>]*>最新<\/button>/);
    assert.doesNotMatch(html,/<select\b|关注/,'WebView native selects crash in the reading dialog; controls must stay in HTML');
    const native=page(),scripts=[];
    native.context.bridge=async code=>{scripts.push(code);return JSON.stringify(result('native'));};
    native.run('window.fanqieRun=bridge;call=initialCall;');
    const data=await native.run('call("reviews",{book:{bookUrl:"123"},scope:"book",sort:3})');
    assert.equal(data.items[0].content.text,'native');
    assert.match(scripts[0],/^var sourceApi=source;eval\(String\(source.mainJs\)\);relayCall\("reviews",/);
    assert.match(scripts[0],/"sort":3/);
    console.log('Community checks passed: official sorts, request races, page retries, reply context/cursors, chapter availability and paragraph versions.');
})().catch(e=>{console.error(e);process.exitCode=1;});
