
function cfMeta(doc,name){var e=doc.selectFirst('meta[property="'+name+'"]');return e?String(e.attr('content')):'';}
function sbBook(doc,url){
    var count=cfText(doc.selectFirst('.booknav2')).match(/[\d.]+万字/);
    return cfSimple({bookUrl:cfUrl(url||cfMeta(doc,'og:novel:read_url')),name:cfMeta(doc,'og:title'),author:cfMeta(doc,'og:novel:author'),kind:cfMeta(doc,'og:novel:category')+' / '+cfMeta(doc,'og:novel:status')+' / '+cfMeta(doc,'og:novel:update_time'),intro:cfText(doc.selectFirst('.navtxt'))||cfMeta(doc,'og:description'),coverUrl:cfMeta(doc,'og:image'),wordCount:count?count[0]:'',latestChapterTitle:cfMeta(doc,'og:novel:latest_chapter_name'),tocUrl:CF_SITE+'/book/'+String(url||cfMeta(doc,'og:novel:read_url')).match(/\/book\/(\d+)/)[1]+'/'});
}
function sbBooks(doc){
    if(cfMeta(doc,'og:type')==='novel')return [sbBook(doc)];
    var out=[],seen={},items=doc.select('#article_list_content > li,.newbook li,.booklist li,.bookcase li,.ranking li');
    for(var i=0;i<items.size();i++){var item=items.get(i),a=item.selectFirst('h3 a[href*="/book/"],h4 a[href*="/book/"],a[href*="/book/"]');if(!a)continue;var url=cfUrl(a.absUrl('href'));if(seen[url]||!/\/book\/\d+\.htm$/.test(url))continue;seen[url]=true;var h=item.selectFirst('h3,h4'),labels=item.select('.labelbox label');out.push({bookUrl:url,name:h?cfText(h):cfText(a),author:labels.size()?cfText(labels.get(0)):'',kind:labels.size()>1?cfText(labels.get(1)):'',intro:cfText(item.selectFirst('ol')),latestChapterTitle:cfText(item.selectFirst('.zxzj')),coverUrl:cfImage(item)});}
    if(!out.length){var links=doc.select('a[href*="/book/"]');for(var j=0;j<links.size();j++){var a=links.get(j),url=cfUrl(a.absUrl('href')),title=cfText(a);if(!title||seen[url]||!/\/book\/\d+\.htm$/.test(url))continue;seen[url]=true;out.push({bookUrl:url,name:title});}}
    return out;
}
// The site's embedded search verification is separate from managed CF challenges.
// URL lookup remains available without pretending the verification page is a result.
function search(key,page){
    if(/^https?:\/\//.test(String(key)))return [sbBook(cfDoc(cfUrl(key)),cfUrl(key))];
    throw '69书吧站内搜索需要独立嵌入式验证；此接入示例请使用发现页，或输入完整书籍链接取书';
}
function explore(url,page){return cfExplore(String(url),page,sbBooks);}
function getBookInfo(book){return sbBook(cfDoc(book.bookUrl),book.bookUrl);}
function getChapters(book){
    var url=book.tocUrl||CF_SITE+'/book/'+String(book.bookUrl).match(/\/book\/(\d+)/)[1]+'/',doc=cfDoc(url),links=doc.select('#catalog li a[href*="/txt/"],.catalog li a[href*="/txt/"]'),out=[],seen={};
    for(var i=0;i<links.size();i++){var a=links.get(i),href=cfUrl(a.absUrl('href'));if(seen[href])continue;seen[href]=true;out.push({title:cfText(a),url:href,index:Number(String(a.parent().attr('data-num')))||i});}
    if(!out.length)throw '完整目录为空';out.sort(function(a,b){return a.index-b.index;});return out;
}
function getContent(chapter,book,nextChapterUrl){
    var doc=cfDoc(chapter.url),el=doc.selectFirst('.txtnav,#txtcontent');if(!el)throw '正文容器不存在';
    el.select('script,style,h1,.txtinfo,.bottom-ad,.txtad,.txtcenter,ins').remove();
    var text=String(cfDocument(String(el.html()).replace(/<br\s*\/?\s*>/gi,'\n')).wholeText()).replace(/\n[ \t\u3000]*/g,'\n').replace(/\n{3,}/g,'\n\n').trim();
    if(!text)throw '正文为空';return java.t2s(text);
}
