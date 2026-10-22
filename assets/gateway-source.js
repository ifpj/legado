// Runs in Legado. The gateway fetches pages; the source only parses HTML.
// Text conversion uses Legado's built-in converter, never a local character map.
function cfChapterHeading(value,bookName) {
    function key(text){return String(text||'').replace(/[\uff01-\uff5e]/g,function(c){return String.fromCharCode(c.charCodeAt(0)-0xfee0);}).replace(/[\s\u200b\ufeff]/g,'').replace(/[·•…“”‘’"'「」『』]/g,'').replace(/\.{3,}/g,'').toLowerCase();}
    var text=key(value),book=key(bookName),bookPrefix=false;
    if(book&&text.indexOf(book)===0&&/^第[零〇一二三四五六七八九十百千万两\d]+[章回节卷集话]/.test(text.substring(book.length))){text=text.substring(book.length);bookPrefix=true;}
    text=text.replace(/^\d+[.、](?=\D)/,'');
    var heading=/^第([零〇一二三四五六七八九十百千万两\d]*)[章回节卷集话]/.exec(text),core=text;
    if(heading){
        core=text.substring(heading[0].length);
        core=core.replace(/^\d+[:、]/,'');
        if(core.indexOf(heading[0])===0)core=core.substring(heading[0].length);
    }
    core=core.replace(/\(([^()]{1,50})\)$/,function(note,inside){return /(?:\d|[一二三四五六七八九十])[\d.一二三四五六七八九十]*[kw万千字]|月票|推荐票|求票|加更|合一/.test(inside)?'':note;});
    return {key:text.replace(/[:、]+$/,''),core:core.replace(/[:、]+$/,'').replace(/\./g,''),numbered:!!heading,bookPrefix:bookPrefix};
}
function cfCleanChapterText(value,chapterTitle,pageHeading,bookName) {
    var text=String(value).replace(/[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f]/g,'');
    // This is a standalone boundary marker, not arbitrary author notes.
    text=text.replace(/(^|\n)[^\S\r\n]*[（(]本章完[）)][^\S\r\n]*(?=\r?\n|$)/g,'$1');
    text=text.replace(/[（(]本章完[）)]\s*$/,'');
    var targets=[],values=[chapterTitle,pageHeading];
    for(var i=0;i<values.length;i++){
        var target=cfChapterHeading(values[i],bookName);
        if(target.key&&!/^(半夏小说|69书吧|台湾小说网)$/.test(target.key))targets.push(target);
    }
    var lines=text.split('\n'),removed=0;
    // Full text is examined separately for site pollution. Only a leading,
    // standalone heading can be removed by this metadata comparison.
    while(lines.length&&removed<3){
        if(!lines[0].trim()){lines.shift();continue;}
        var candidate=cfChapterHeading(lines[0],bookName),same=false;
        for(var t=0;t<targets.length;t++){
            target=targets[t];
            if(candidate.key.length>=2&&candidate.key===target.key)same=true;
            else if(candidate.numbered&&target.numbered&&candidate.core.length>=1&&candidate.core===target.core)same=true;
            else if(candidate.bookPrefix&&candidate.numbered&&!candidate.core&&target.numbered)same=true;
        }
        if(!same||lines.slice(1).join('\n').trim().length<20)break;
        lines.shift();removed++;
    }
    return lines.join('\n').replace(/\n{3,}/g,'\n\n').trim();
}
function cfSession() {
    var raw=String(source.getVariable()||''),state;
    try{state=JSON.parse(raw);}catch(e){}
    var previous=state&&typeof state==='object'&&state.session||raw;
    var session=previous||'legado-'+CF_SITE_ID+'-'+Date.now()+'-'+Math.random().toString(16).slice(2);
    // Older Taiwan exports used a generic relay prefix. Preserve the unique
    // suffix and any saved pagination routes when adopting the site name.
    if(CF_SITE_ID==='twkan')session=session.replace(/^legado-relay-/,'legado-twkan-');
    if(session!==previous){
        if(!state||typeof state!=='object'||state instanceof Array)state={};
        state.session=session;source.setVariable(JSON.stringify(state));
    }
    return session;
}
function cfUrl(value) {
    var uri=new Packages.java.net.URI(CF_SITE+'/').resolve(String(value));
    var host=String(uri.getHost()||'').toLowerCase(), expected=String(new Packages.java.net.URI(CF_SITE).getHost());
    if(host!==expected)throw '书源链接离开了示例站点：'+host;
    return CF_SITE+String(uri.getRawPath()||'/')+(uri.getRawQuery()?'?'+uri.getRawQuery():'');
}
function cfRead(url) {
    url=cfUrl(url);
    // Finish on the gateway before Legado's 60-second read timeout. Otherwise
    // connect() may turn an IOException into a synthetic 200 text response.
    var endpoint=CF_BASE+'/fetch?url='+encodeURIComponent(url)+'&session='+encodeURIComponent(cfSession())+'&timeoutMs=45000';
    var headers=CF_TOKEN?{Authorization:'Bearer '+CF_TOKEN}:{};
    var r=java.connect(endpoint,JSON.stringify(headers),55000),body=String(r.body()),status=Number(r.code());
    if(status!==200){var error;try{error=JSON.parse(body);}catch(e){}throw error&&error.code?'CF 服务：'+error.code+' / '+error.error:'目标网站 HTTP '+status;}
    var type=String(r.headers().get('Content-Type')||'');
    if(!/text\/html/i.test(type))throw 'CF 服务未返回网页：'+body.slice(0,300);
    if(/window\._cf_chl_opt|<title>\s*Just a moment/i.test(body))throw '服务仍返回验证页';
    if(/id=["']cfts["']/.test(body)&&/verify_token/.test(body))throw '此页面需要网站独立的嵌入式验证，尚未取得实际搜索结果';
    return body;
}
function cfDocument(html){return Packages.org.jsoup.Jsoup.parse(String(html),CF_SITE+'/');}
function cfDoc(url){return Packages.org.jsoup.Jsoup.parse(cfRead(url),cfUrl(url));}
function cfText(node){return node?String(java.t2s(String(node.text()).trim())):'';}
function cfIntro(value){
    var text=String(value||'');
    // Metadata may contain both escaped HTML and literal tags. Decode in
    // bounded passes, preserving paragraphs before extracting plain text.
    for(var pass=0;pass<3;pass++){
        var html=text.replace(/<br\s*\/?\s*>/gi,'\n').replace(/<\/(?:p|div|li|section)>/gi,'\n');
        var doc=cfDocument(html);doc.select('script,style,noscript,iframe').remove();
        var cleaned=String(doc.wholeText());if(cleaned===text)break;text=cleaned;
    }
    text=String(java.t2s(text)).replace(/\r\n?/g,'\n').replace(/[\u00a0\u2003\u3000]/g,' ');
    var lines=text.split('\n'),out=[];
    for(var i=0;i<lines.length;i++){
        var line=lines[i].replace(/[ \t]+/g,' ').trim();
        if(/^[.。…·\s]{2,}$/.test(line))continue;
        if(/^(?:《[^》]{1,80}》)?\s*(?:[一二三四五六七八九十0-9号新老官方总VIP ]*)?(?:书友群|读者群|粉丝群|QQ群|群号)[^\n]{0,100}$/i.test(line)&&/[0-9]{5,12}/.test(line))continue;
        out.push(line);
    }
    return out.join('\n').replace(/\n{3,}/g,'\n\n').trim();
}
function cfCover(url){
    url=String(url||'');if(!url)return '';
    if(url.indexOf(CF_BASE+'/image?')===0)return url;
    url=url.replace(/^http:/,'https:');
    return CF_BASE+'/image?url='+encodeURIComponent(url)+'&session='+encodeURIComponent(cfSession())+'&timeoutMs=45000'+(CF_TOKEN?'&token='+encodeURIComponent(CF_TOKEN):'');
}
function cfImage(item){var img=item.selectFirst('img');if(!img)return '';for(var i=0,attrs=['data-original','data-src','src'];i<attrs.length;i++){if(img.hasAttr(attrs[i]))return cfCover(String(img.absUrl(attrs[i])));}return '';}
function cfSimple(value){var keys=['name','author','kind','latestChapterTitle','title','wordCount'];if(value instanceof Array){for(var i=0;i<value.length;i++)cfSimple(value[i]);}else if(value){for(var j=0;j<keys.length;j++)if(value[keys[j]])value[keys[j]]=java.t2s(String(value[keys[j]]));if(value.intro)value.intro=cfIntro(value.intro);if(value.coverUrl)value.coverUrl=cfCover(value.coverUrl);}return value;}
function cfNextPage(doc,href,page){
    var links=doc.select('a[href]');for(var i=0;i<links.size();i++){var a=links.get(i),text=String(a.text()).trim();if(/^(下一頁|下一页|下頁|下页|下一页>|>|›|»)$/.test(text))return String(a.absUrl('href'));}
    return '';
}
function cfPage(path,page){
    if(page<=1)return path;
    if(path==='/')return '';
    if(CF_SITE_ID==='banxia')return path.replace(/_\d+\.html$/, '_'+page+'.html');
    if(/_\d+\.(?:html|htm)$/.test(path))return path.replace(/_\d+\.(html|htm)$/, '_'+page+'.$1');
    return path+(path.indexOf('?')>=0?'&':'?')+'page='+page;
}
function cfDiscovery(){
    var list=[{title:'首页推荐',url:'/'}],seen={'/':true};
    function add(title,path){if(!path||seen[path])return;seen[path]=true;list.push({title:java.t2s(String(title)),url:path});}
    if(CF_SITE_ID==='banxia'){
        var home=cfDoc(CF_SITE+'/'),links=home.select('#main-nav a[href^="/list/"]');
        for(var i=0;i<links.size();i++)add(links.get(i).text(),String(links.get(i).attr('href')));
        return list;
    }
    var suffix=CF_SITE_ID==='twkan'?'html':'htm';
    add('最新更新',CF_SITE_ID==='twkan'?'/last':'/last.html');add('完本小说','/novels/full');add('全部小说','/all.html');
    var category=cfDoc(CF_SITE+(CF_SITE_ID==='twkan'?'/novels/class':'/novels/class/0.htm'));
    var links=category.select('a[href]'),expr=CF_SITE_ID==='twkan'?/\/novels\/class\/\d+_1\.html$/:/\/novels\/class\/\d+\.htm$/;
    for(var i=0;i<links.size();i++){var a=links.get(i),path=String(new Packages.java.net.URI(String(a.absUrl('href'))).getRawPath());if(expr.test(path))add(a.text(),path);}
    var rank=cfDoc(CF_SITE+'/novels/hot'),ranks=rank.select('a[href]');
    var rankExpr=CF_SITE_ID==='twkan'?/\/novels\/(?:weekvisit|allvote|newhot)_0_0_1\.html$/:/\/novels\/(?:monthvisit|allvote|newhot)_0_0_1\.htm$/;
    for(var j=0;j<ranks.size();j++){var a=ranks.get(j),p=String(new Packages.java.net.URI(String(a.absUrl('href'))).getRawPath()),title=String(a.text()).trim();if(rankExpr.test(p)&&!/^(不限|連載|连载|全部|全部分類)$/.test(title))add(title,p);}
    return list;
}

// Keep the site's titles and URLs; only arrange the reader's discovery chips.
function cfDiscoveryLayout(list){
    var out=[],navigation=[],categories=[],ranks=[];
    function full(title,url){out.push({title:title,url:url,style:{layout_flexBasisPercent:1,layout_wrapBefore:true}});}
    function grid(items){for(var i=0;i<items.length;i++){out.push({title:items[i].title,url:items[i].url,style:{layout_flexBasisPercent:0.29,layout_flexGrow:0,layout_wrapBefore:i%3===0}});}}
    for(var i=0;i<list.length;i++){
        var item=list[i],path=String(item.url||'');
        if(path==='/')full(item.title,item.url);
        else if(/^\/(?:novels\/class\/|list\/)/.test(path))categories.push(item);
        else if(/^\/novels\/(?:weekvisit|monthvisit|allvote|newhot)/.test(path))ranks.push(item);
        else navigation.push(item);
    }
    grid(navigation);
    if(categories.length){full('小说分类',null);grid(categories);}
    if(ranks.length){full('排行榜',null);grid(ranks);}
    return out;
}


// Store only the next-page URL observed in the current page, never its HTML.
// Non-paginated website sections stop immediately instead of repeating page 1.
function cfExplore(url,page,parse){
    var first=cfUrl(url),number=Number(page)||1,state={},raw=String(source.getVariable()||'');
    try{state=raw?JSON.parse(raw):{};}catch(e){}
    if(!state.session){cfSession();state=JSON.parse(String(source.getVariable()));}
    var routes=state.routes||{},route=routes[first],target=first;
    if(number>1){
        if(!route||route.page!==number||!route.next)return [];
        target=route.next;
    }
    var doc=cfDoc(target),next=cfNextPage(doc),books=parse(doc);
    state=JSON.parse(String(source.getVariable()));routes=state.routes||{};
    routes[first]={page:number+1,next:next?cfUrl(next):''};
    var keys=Object.keys(routes);while(keys.length>24){delete routes[keys.shift()];}
    state.routes=routes;source.setVariable(JSON.stringify(state));return books;
}
