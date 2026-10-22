// Runs in Legado. The gateway fetches pages; the source only parses HTML.
// Text conversion uses Legado's built-in converter, never a local character map.
function cfSession() {
    var state=String(source.getVariable()||'');
    if(state){try{return JSON.parse(state).session||state;}catch(e){return state;}}
    var session='legado-'+CF_SITE_ID+'-'+Date.now()+'-'+Math.random().toString(16).slice(2);
    source.setVariable(JSON.stringify({session:session}));return session;
}
function cfUrl(value) {
    var uri=new Packages.java.net.URI(CF_SITE+'/').resolve(String(value));
    var host=String(uri.getHost()||'').toLowerCase(), expected=String(new Packages.java.net.URI(CF_SITE).getHost());
    if(host!==expected)throw '书源链接离开了示例站点：'+host;
    return CF_SITE+String(uri.getRawPath()||'/')+(uri.getRawQuery()?'?'+uri.getRawQuery():'');
}
function cfRead(url) {
    url=cfUrl(url);
    var endpoint=CF_BASE+'/fetch?url='+encodeURIComponent(url)+'&session='+encodeURIComponent(cfSession())+'&timeoutMs=90000';
    var headers=CF_TOKEN?{Authorization:'Bearer '+CF_TOKEN}:{};
    var r=java.connect(endpoint,JSON.stringify(headers),100000),body=String(r.body()),status=Number(r.code());
    if(status!==200){var error;try{error=JSON.parse(body);}catch(e){}throw error&&error.code?'CF 服务：'+error.code+' / '+error.error:'目标网站 HTTP '+status;}
    if(/window\._cf_chl_opt|<title>\s*Just a moment/i.test(body))throw '服务仍返回验证页';
    if(/id=["']cfts["']/.test(body)&&/verify_token/.test(body))throw '此页面需要网站独立的嵌入式验证，尚未取得实际搜索结果';
    return body;
}
function cfDocument(html){return Packages.org.jsoup.Jsoup.parse(String(html),CF_SITE+'/');}
function cfDoc(url){return Packages.org.jsoup.Jsoup.parse(cfRead(url),cfUrl(url));}
function cfText(node){return node?String(java.t2s(String(node.text()).trim())):'';}
function cfCover(url){
    url=String(url||'').replace(/^http:/,'https:');if(!url)return '';
    if(url.indexOf(CF_BASE+'/image?')===0)return url;
    return CF_BASE+'/image?url='+encodeURIComponent(url)+'&session='+encodeURIComponent(cfSession())+(CF_TOKEN?'&token='+encodeURIComponent(CF_TOKEN):'');
}
function cfImage(item){var img=item.selectFirst('img');if(!img)return '';for(var i=0,attrs=['data-original','data-src','src'];i<attrs.length;i++){if(img.hasAttr(attrs[i]))return cfCover(String(img.absUrl(attrs[i])));}return '';}
function cfSimple(value){var keys=['name','author','kind','intro','latestChapterTitle','title','wordCount'];if(value instanceof Array){for(var i=0;i<value.length;i++)cfSimple(value[i]);}else if(value){for(var j=0;j<keys.length;j++)if(value[keys[j]])value[keys[j]]=java.t2s(String(value[keys[j]]));if(value.coverUrl)value.coverUrl=cfCover(value.coverUrl);}return value;}
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
