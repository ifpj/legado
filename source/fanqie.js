/* SPDX-License-Identifier: GPL-3.0-only
 * Fanqie source for the Rust relay. Download from its web console to set the host.
 * This template is embedded in the service; import its generated source.json.
 */
var config = {
    bookSourceUrl: 'http://127.0.0.1:19670/fanqie',
    bookSourceName: '番茄小说（Rust 中转）',
    bookSourceType: 0,
    maxBatchSize: 30,
    customButton: true,
    enabledCookieJar: false,
    bookSourceGroup: 'APP/Rust',
    bookSourceComment: '连接 Rust 中转服务；在服务 Web 台导入配置好的书源。实时请求，不缓存接口响应，支持官方网页登录、书架和历史。',
    exploreUrl: '@js:eval(String(source.mainJs));relayExploreMenu();'
};
var RELAY_URL = 'http://127.0.0.1:19670';
var RELAY_TOKEN = '';
var RELAY_STATE_KEY = 'fanqie.rust.device.v1';
var RELAY_LAST_METRICS = null;
function relayAccount() {
    var raw=sourceApi.getLoginInfo(), account;
    try { account=raw?JSON.parse(String(raw)):null; } catch(e) { return null; }
    return account && (/\b(?:sessionid|sid_guard|sessionid_ss)=/.test(String(account.sessionCookie||''))||account.ttToken)?account:null;
}
function relayBook(book) { return {bookUrl:String(book.bookUrl||book.tocUrl||''),variable:String(book.variable||'{}')}; }
function relayCall(operation,args,account) {
    var state=cache.get(RELAY_STATE_KEY)||cache.get('fanqie.independent.v3.state'), device;
    try { device=state?JSON.parse(String(state)):null; } catch(e) { device=null; }
    if(!device) device={deviceType:String(Packages.android.os.Build.MODEL),deviceBrand:String(Packages.android.os.Build.BRAND)};
    if(account===undefined)account=relayAccount();
    var payload=JSON.stringify({operation:operation,args:args,device:device,account:account,osVersion:String(Packages.android.os.Build.VERSION.RELEASE)});
    var client=cache.getFromMemory('fanqie.rust.http.client.v2');
    if(!client){client=new Packages.okhttp3.OkHttpClient.Builder().callTimeout(120,Packages.java.util.concurrent.TimeUnit.SECONDS).build();cache.putMemory('fanqie.rust.http.client.v2',client);}
    var request=new Packages.okhttp3.Request.Builder().url(RELAY_URL+'/v1/call').header('X-Relay-Client','Legado').post(Packages.okhttp3.RequestBody.create(Packages.okhttp3.MediaType.parse('application/json; charset=utf-8'),java.strToBytes(payload)));
    var token=RELAY_TOKEN||cache.get('fanqie.rust.service.token');if(token)request.header('Authorization','Bearer '+String(token));
    var response=client.newCall(request.build()).execute();
    try {
        var next=response.header('x-fanqie-state','');
        if(next)cache.put(RELAY_STATE_KEY,String(java.bytesToStr(java.base64DecodeToByteArray(String(next)))));
        RELAY_LAST_METRICS=String(response.header('x-fanqie-metrics','{}'));
        var raw=String(response.body().string());
        if(response.code()!==200){var error;try{error=JSON.parse(raw).error;}catch(e){}throw error||'Rust 中转 HTTP '+response.code();}
        // JSON arrays/objects are returned as strings so the existing engine can
        // pass them straight to its Kotlin/Gson model parser, without Rhino parsing.
        return raw;
    } finally { response.close(); }
}
function search(key,page){return relayCall('search',{key:String(key),page:Number(page)||1});}
function relayPreferences(){return relayReadState('fanqie.rust.preferences.v1.'+RELAY_URL);}
function relayExploreMenu(){return relayCall('discovery_menu',{preferences:relayPreferences(),native:true});}
function explore(url,page){return relayCall('explore',{url:String(url),page:Number(page)||1,preferences:relayPreferences()});}
function getBookInfo(book){return relayCall('detail',{book:relayBook(book)});}
function getChapters(book){return relayCall('chapters',{book:relayBook(book)});}
function getContent(chapter,book,nextChapterUrl){return relayCall('content',{book:relayBook(book),chapter:{url:String(chapter.url)}});}
function getContentBatch(chapters,book){
    var args=[],byUrl={},i;
    for(i=0;i<chapters.size();i++){var chapter=chapters.get(i),url=String(chapter.url);args.push({url:url});byUrl[url]=chapter;}
    var data=JSON.parse(relayCall('content_batch',{book:relayBook(book),chapters:args,native:true}));
    for(i=0;i<data.chapters.length;i++){var item=data.chapters[i];if(byUrl[item.url])java.cacheContent(byUrl[item.url],String(item.content));}
    // Missing/invalid chapters stay uncached; Legado's scheduler fetches them singly.
}
function relaySyncKey(account,book){return 'fanqie.rust.progress.v1.'+RELAY_URL+'.'+String(account.uid)+'.'+String(book.bookUrl);}
function relayReadState(key){try{return JSON.parse(String(cache.get(key)||'{}'));}catch(e){return {};}}
function relayApplyPosition(book,position){
    if(!position||position.chapterIndex===undefined)return false;
    book.durChapterIndex=Number(position.chapterIndex);book.durChapterPos=0;
    book.durChapterTitle=String(position.chapterTitle||'');book.durChapterTime=Number(position.timestampMs)||0;
    book.save();return true;
}
function relayOnEvent(event,book,chapter,result){
    if(event==='clickCustomButton'){
        var id=String(book.bookUrl).replace(/.*\/page\//,'');
        var cid=chapter?String(chapter.url).replace(/.*\/reader\//,''):'';
        java.showBrowser(RELAY_URL+'/community?bookId='+encodeURIComponent(id)+'&chapterId='+encodeURIComponent(cid)+'&title='+encodeURIComponent(String(book.name||'')),null,null,JSON.stringify({title:'番茄书评与讨论'}));return true;
    }
    if(event==='clickBookLabel'&&result&&!/^(评分|书架分组|置顶|完结|连载|[0-9])/.test(String(result))){java.searchBook(String(result),String(source.bookSourceName)+'::'+String(source.bookSourceUrl));return true;}
    if(event==='addBookShelf'||event==='delBookShelf'){
        var mode=relayPreferences().shelfSync||'off',action=event==='addBookShelf'?'add':'remove';
        if(mode==='both'||mode===action){
            var account=relayAccount();if(!account){java.toast('书架联动需要先登录番茄账号');return;}
            try{relayCall('shelf_change',{book:relayBook(book),action:action},account);}catch(e){java.toast('番茄书架联动失败：'+String(e.message||e));}
        }return;
    }
    if(!book||['startRead','saveRead','endRead'].indexOf(event)<0)return;
    var account=relayAccount();if(!account||!account.uid)return;
    var key=relaySyncKey(account,book);
    java.lock(key,function(){
        var saved=relayReadState(key),active;
        try{active=JSON.parse(String(cache.getFromMemory(key+'.active')||'null'));}catch(e){}
        var index=chapter?Number(chapter.index):Number(book.durChapterIndex);
        // Reopening an already loaded book can omit START_READ. The reader
        // creates a new Book object on entry, so its first SAVE_READ is also
        // a safe initialization signal. Book.equals compares URLs; use Java
        // identity to distinguish a new entry from a delayed old callback.
        var identities=cache.getFromMemory(key+'.books');
        if(!identities)identities=new Packages.java.util.IdentityHashMap();
        var generation=Number(identities.get(book)||0),first=!generation;
        if(first){
            generation=Number(active&&active.generation||0)+1;
            if(identities.size()>=16)identities.clear();
            identities.put(book,generation);cache.putMemory(key+'.books',identities);
        }
        if(active&&generation<Number(active.generation||0))return;
        function persist(){cache.put(key,JSON.stringify(saved));if(active)cache.putMemory(key+'.active',JSON.stringify(active));}
        function apply(position){if(position&&position.chapterIndex!==undefined&&Number(position.chapterIndex)!==index){saved.pending=position;relayApplyPosition(book,position);}saved.baseTimestampMs=Number(position&&position.timestampMs)||0;}
        function observe(position){
            // This is a receipt for our submitted write, not cached cloud data.
            // A stale/empty read must not rewind the reader or lower its base.
            if(saved.confirmation&&(!position||Number(position.timestampMs)<Number(saved.confirmation.timestampMs)))return;
            saved.confirmation=null;saved.pending=null;apply(position);
        }
        function upload(outbox){
            var response=JSON.parse(relayCall('progress_put',outbox,account));
            saved.outbox=null;saved.baseTimestampMs=Number(response.position&&response.position.timestampMs)||0;
            saved.confirmation=response.status==='accepted'?response.position:null;
            if(response.status==='conflict')apply(response.position);else saved.pending=null;
        }
        try{
            if(event==='startRead'||event==='saveRead'&&first){
                if(!first&&active&&active.generation===generation){if(saved.pending)relayApplyPosition(book,saved.pending);return;}
                // The entry SAVE_READ only pulls, even when START_READ is
                // omitted or arrives later. It never uploads a chapter.
                active={index:index,startedMs:event==='saveRead'?Number(result)||Date.now():Date.now(),open:true,generation:generation};
                if(saved.outbox)upload(saved.outbox);
                var remote=JSON.parse(relayCall('progress_get',{book:relayBook(book)},account));
                observe(remote.position);
                if(saved.pending)java.toast('番茄章节已同步，请退出后重新进入书籍');
            }else if(event==='saveRead'){
                if(!active||!active.open||Number(result)<active.startedMs){if(saved.pending)relayApplyPosition(book,saved.pending);return;}
                if(saved.pending&&index===Number(saved.pending.chapterIndex)){
                    saved.pending=null;active.index=index;
                }else if(index!==active.index){
                    saved.outbox={book:relayBook(book),chapter:{url:String(chapter.url),title:String(chapter.title||'')},chapterIndex:index,chapterCount:Number(book.totalChapterNum)||0,timestampMs:Number(result)||Date.now(),baseTimestampMs:Number(saved.baseTimestampMs)||0};
                    active.index=index;persist();upload(saved.outbox);
                }
                if(saved.pending)relayApplyPosition(book,saved.pending);
            }else{
                if(active&&active.open&&chapter&&index!==active.index&&!(saved.pending&&index===Number(saved.pending.chapterIndex))){
                    saved.outbox={book:relayBook(book),chapter:{url:String(chapter.url),title:String(chapter.title||'')},chapterIndex:index,chapterCount:Number(book.totalChapterNum)||0,timestampMs:Date.now(),baseTimestampMs:Number(saved.baseTimestampMs)||0};
                    active.index=index;persist();
                }
                if(saved.outbox)upload(saved.outbox);
                if(active&&active.open){
                    var latest=JSON.parse(relayCall('progress_get',{book:relayBook(book)},account));
                    observe(latest.position);
                    if(!latest.position&&!saved.confirmation&&chapter){
                        saved.outbox={book:relayBook(book),chapter:{url:String(chapter.url),title:String(chapter.title||'')},chapterIndex:index,chapterCount:Number(book.totalChapterNum)||0,timestampMs:Date.now(),baseTimestampMs:0};
                        persist();upload(saved.outbox);
                    }
                }
                // Reapply after the reader's final local save so the next open
                // starts at the cloud chapter. No reflection or Web service.
                if(saved.pending)relayApplyPosition(book,saved.pending);
            }
            saved.error=null;
        }catch(e){saved.error=String(e.message||e);java.log('番茄章节同步：'+saved.error);}
        finally{if(event==='endRead'&&active)active.open=false;persist();}
    },60000);
}
function relayWebLogin(){
    var options={initialPage:1,needRedirect:true,redirectURLs:{login:'https://fanqienovel.com/'}};
    var url='https://fanqienovel.com/main/writer/login?_login_data='+encodeURIComponent(JSON.stringify(options));
    url+=','+JSON.stringify({headers:{'User-Agent':'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36'}});
    java.startBrowserAwait(url,'番茄官方登录：完成后点击右上角勾号',false);
    var cookie=String(java.getCookie('https://fanqienovel.com')||'');
    if(!/\b(?:sessionid|sid_guard|sessionid_ss)=[^;]+/.test(cookie))throw '尚未取得网页登录会话，请完成登录后点击右上角勾号';
    return JSON.parse(relayCall('web_login',{cookie:cookie},null));
}
function loginUi(state){
    var account=relayAccount(),prefs=relayPreferences(),rows=[];
    if(account)rows.push({name:'已登录：'+(account.nickname||'番茄账号'),type:'label'},{name:'章节双向同步自动开启；云端章节在重新进入书籍时应用。',type:'label'},{name:'退出登录',type:'button',action:'logout'});
    else rows.push({name:'在官方网页登录，返回时点右上角勾号。',type:'label'},{name:'网页登录',type:'button',action:'webLogin'});
    rows.push({name:'发现分类',key:'gender',type:'select',options:['男生','女生'],value:prefs.gender==='0'?'女生':'男生'});
    var choices=relayFilterChoices();
    ['words','status','sort'].forEach(function(k){var options=choices[k],picked=options[0][0];options.forEach(function(v){if(v[1]===prefs[k])picked=v[0];});rows.push({name:{words:'字数筛选',status:'更新与完结筛选',sort:'分类排序'}[k],key:k,type:'select',options:options.map(function(v){return v[0];}),value:picked});});
    rows.push({name:'显示原生段评数量（每章额外请求一次）',key:'nativeReviews',type:'toggle',value:prefs.nativeReviews?'true':'false'},
        {name:'番茄书架联动',key:'shelfSync',type:'select',options:['关闭','仅加入','仅移除','加入和移除'],value:{off:'关闭',add:'仅加入',remove:'仅移除',both:'加入和移除'}[prefs.shelfSync||'off']},
        {name:'阅读菜单的自定义按钮可按需查看书评和章节讨论。更改段评开关后退出并重新进入书籍；分类设置保存后刷新发现入口。',type:'label'},
        {name:'保存设置',type:'button',action:'preferences'});
    return {rows:rows};
}
function loginAction(action,state,form){
    if(action==='logout'){sourceApi.removeLoginInfo();sourceApi.removeLoginHeader();return {state:{}};}
    if(action==='webLogin'){try{return relayWebLogin();}catch(e){return {error:{webLogin:String(e.message||e)}};}}
    if(action==='preferences'){
        var prefs={gender:form.gender==='女生'?'0':'1',nativeReviews:String(form.nativeReviews)==='true',shelfSync:{'关闭':'off','仅加入':'add','仅移除':'remove','加入和移除':'both'}[String(form.shelfSync)]||'off'},choices=relayFilterChoices();
        ['words','status','sort'].forEach(function(k){prefs[k]=choices[k][0][1];choices[k].forEach(function(v){if(v[0]===String(form[k]))prefs[k]=v[1];});});
        cache.put('fanqie.rust.preferences.v1.'+RELAY_URL,JSON.stringify(prefs));java.toast('设置已保存，请刷新发现入口');return {state:{}};
    }
    return {error:{webLogin:'请使用网页登录'}};
}
function relayFilterChoices(){return {
    words:[['不限','word_num_default'],['10万以内','word_num_lte10'],['30万以内','word_num_lte30'],['50万以内','word_num_lte50'],['30万以上','word_num_gte30'],['50万以上','word_num_gte50'],['100万以上','word_num_gte100'],['500万以上','word_num_gte500']],
    status:[['不限','creation_status_default'],['完结','creation_status_end'],['连载','creation_status_loading'],['半年内完结','creation_status_half_year_end'],['3日内更新','creation_status_3day_update'],['7日内更新','creation_status_7day_update'],['1月内更新','creation_status_1month_update']],
    sort:[['推荐','sort_default'],['最新','sort_new_book'],['高分','sort_score']]};}
function relayReviewArgs(chapter,book,paraData,page){
    var para;try{para=JSON.parse(String(paraData||'{}'));}catch(e){para={};}
    return {book:relayBook(book),chapter:{url:String(chapter.url)},scope:'paragraph',paraIndex:Number(para.paraIndex)||0,version:String(para.version||'1'),page:Number(page)||1,native:true};
}
function getReviewSummary(chapter,book){
    if(!relayPreferences().nativeReviews)return '[]';
    return relayCall('review_summary',{book:relayBook(book),chapter:{url:String(chapter.url)},native:true});
}
function getReviewDetail(chapter,book,paraIndex,paraData,page){return relayCall('reviews',relayReviewArgs(chapter,book,paraData,page));}
function getReviewReplies(chapter,book,paraIndex,paraData,reviewId,page){var args=relayReviewArgs(chapter,book,paraData,page);args.reviewId=String(reviewId);return relayCall('review_replies',args);}
