
function cfMeta(doc,name){var e=doc.selectFirst('meta[property="'+name+'"]');return e?String(e.attr('content')):'';}
function sbBook(doc,url){
    var count=cfText(doc.selectFirst('.booknav2')).match(/[\d.]+万字/);
    return cfSimple({bookUrl:cfUrl(url||cfMeta(doc,'og:novel:read_url')),name:cfMeta(doc,'og:title'),author:cfMeta(doc,'og:novel:author'),kind:cfMeta(doc,'og:novel:category')+' / '+cfMeta(doc,'og:novel:status')+' / '+cfMeta(doc,'og:novel:update_time'),intro:cfText(doc.selectFirst('.navtxt'))||cfMeta(doc,'og:description'),coverUrl:cfMeta(doc,'og:image'),wordCount:count?count[0]:'',latestChapterTitle:cfMeta(doc,'og:novel:latest_chapter_name'),tocUrl:CF_SITE+'/book/'+String(url||cfMeta(doc,'og:novel:read_url')).match(/\/book\/(\d+)/)[1]+'/'});
}
function sbBooks(doc){
    if(cfMeta(doc,'og:type')==='novel')return [sbBook(doc)];
    var error=doc.selectFirst('.mybox .mytitle');
    if(error&&cfText(error)==='出现错误'){
        var message=cfText(error.parent());
        if(/没有找到|未找到|没有搜索到|无相关/.test(message))return [];
        throw '69书吧：'+message.replace(/^出现错误\s*/, '');
    }
    var out=[],seen={},items=doc.select('#article_list_content > li,.newbox > ul > li,.newbook li,.booklist li,.bookcase li,.ranking li');
    for(var i=0;i<items.size();i++){var item=items.get(i),a=item.selectFirst('h3 a[href*="/book/"],h4 a[href*="/book/"],a[href*="/book/"]');if(!a)continue;var url=cfUrl(a.absUrl('href'));if(seen[url]||!/\/book\/\d+\.htm$/.test(url))continue;seen[url]=true;var h=item.selectFirst('h3,h4'),labels=item.select('.labelbox label');out.push({bookUrl:url,name:h?cfText(h):cfText(a),author:labels.size()?cfText(labels.get(0)):'',kind:labels.size()>1?cfText(labels.get(1)):'',intro:cfText(item.selectFirst('ol')),latestChapterTitle:cfText(item.selectFirst('.zxzj')),coverUrl:cfImage(item)});}
    if(!out.length){var links=doc.select('a[href*="/book/"]');for(var j=0;j<links.size();j++){var a=links.get(j),url=cfUrl(a.absUrl('href')),title=cfText(a);if(!title||seen[url]||!/\/book\/\d+\.htm$/.test(url))continue;seen[url]=true;out.push({bookUrl:url,name:title,coverUrl:cfImage(a)});}}
    return cfSimple(out);
}
// The gateway replays this site's GBK search form after embedded verification.
function search(key,page){
    if(/^https?:\/\//.test(String(key)))return [sbBook(cfDoc(cfUrl(key)),cfUrl(key))];
    var keyword=String(java.t2s(String(key))).trim();if(!keyword)return [];
    var encoded=String(Packages.java.net.URLEncoder.encode(keyword,'GBK'));
    return cfExplore(CF_SITE+'/modules/article/search.php?searchkey='+encoded+'&searchtype=all',page,sbBooks);
}
function explore(url,page){return cfExplore(String(url),page,sbBooks);}
function getBookInfo(book){return sbBook(cfDoc(book.bookUrl),book.bookUrl);}
function getChapters(book){
    var url=book.tocUrl||CF_SITE+'/book/'+String(book.bookUrl).match(/\/book\/(\d+)/)[1]+'/',doc=cfDoc(url),links=doc.select('#catalog li a[href*="/txt/"],.catalog li a[href*="/txt/"]'),out=[],seen={};
    for(var i=0;i<links.size();i++){var a=links.get(i),href=cfUrl(a.absUrl('href'));if(seen[href])continue;seen[href]=true;out.push({title:cfText(a),url:href,index:Number(String(a.parent().attr('data-num')))||i});}
    if(!out.length)throw '完整目录为空';out.sort(function(a,b){return a.index-b.index;});return out;
}
function getContent(chapter,book,nextChapterUrl){
    var doc=cfDoc(chapter.url),heading=cfText(doc.selectFirst('h1')),el=doc.selectFirst('.txtnav,#txtcontent');if(!el)throw '正文容器不存在';
    el.select('script,style,h1,.txtinfo,.bottom-ad,.txtad,.txtcenter,ins').remove();
    var text=String(cfDocument(String(el.html()).replace(/<br\s*\/?\s*>/gi,'\n')).wholeText()).replace(/\n[ \t\u3000]*/g,'\n').replace(/\n{3,}/g,'\n\n').trim();
    if(!text)throw '正文为空';return cfCleanChapterText(String(java.t2s(text)),chapter.title,heading,book&&book.name);
}
