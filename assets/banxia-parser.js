
function bxBooks(doc){
    var items=doc.select('li.pop-book,li.pop-book2'),out=[],seen={};
    for(var i=0;i<items.size();i++){
        var item=items.get(i),a=item.selectFirst('a[href*="/books/"]'),heading=item.selectFirst('h2');
        if(!a||!heading)continue;var url=cfUrl(a.absUrl('href'));if(seen[url])continue;seen[url]=true;
        var author=item.selectFirst('.pop-intro');
        out.push({bookUrl:url,name:cfText(heading),author:author?java.t2s(String(author.attr('title')||author.text())):'',coverUrl:cfImage(item)});
    }return out;
}
function search(key,page){return bxBooks(cfDoc(CF_SITE+'/modules/article/search_t.php?searchkey='+encodeURIComponent(java.s2t(String(key)))+'&page='+(Number(page)||1)));}
function explore(url,page){return cfExplore(String(url),page,bxBooks);}
function getBookInfo(book){
    var doc=cfDoc(String(book.bookUrl)),info=doc.selectFirst('.book-describe');if(!info)throw '未找到书籍详情';
    var kind=[],paragraphs=info.select('> p'),author='',latest='';
    for(var i=0;i<paragraphs.size();i++){var p=paragraphs.get(i),text=cfText(p);
        if(/^作者/.test(text))author=text.replace(/^作者[︰：:]?\s*/,'');
        else if(/^(类型|状态|最近更新)/.test(text))kind.push(text);
        else if(/^最新章节/.test(text))latest=cfText(p.selectFirst('a'));
    }
    var intro=info.selectFirst('.describe-html');if(intro){intro.select('script,style,span[style],a.__cf_email__').remove();}
    return {bookUrl:cfUrl(book.bookUrl),name:cfText(info.selectFirst('h1')),author:author,kind:kind.join(' / '),intro:intro?cfText(intro):'',coverUrl:cfImage(doc.selectFirst('.book-img')),latestChapterTitle:latest,tocUrl:cfUrl(book.bookUrl)};
}
function getChapters(book){
    var doc=cfDoc(book.tocUrl||book.bookUrl),links=doc.select('.book-list li a[href]'),out=[],seen={};
    for(var i=0;i<links.size();i++){var a=links.get(i),url=cfUrl(a.absUrl('href'));if(!/\/books\/\d+\/\d+\.html$/.test(url)||seen[url])continue;seen[url]=true;out.push({title:cfText(a),url:url});}
    if(!out.length)throw '完整目录为空';return out;
}
function getContent(chapter,book,nextChapterUrl){
    var doc=cfDoc(chapter.url),el=doc.selectFirst('#nr1');if(!el)throw '正文容器不存在';
    el.select('script,style,ins,.ad,.ads').remove();
    var html=String(el.html()).replace(/<br\s*\/?\s*>/gi,'\n'),text=String(cfDocument(html).wholeText()).replace(/\n[ \t\u3000]*/g,'\n').replace(/\n{3,}/g,'\n\n').trim();
    if(!text)throw '正文为空';return java.t2s(text);
}
