var CF_READER_KEY='jieqi2026abcd12';
// Match a normalized detection copy, then remove only the corresponding original
// spans. Novel text, accents, supplementary characters and punctuation stay intact.
function cfCleanTwkanContent(value) {
    var lines=String(value).split('♙♕\u2003\u2003😲✌').join('').replace(/(^|\n)[^\S\r\n]*(?:\]>|>)[^\S\r\n]*(?=\n*$)/,'$1').split('\n');
    var suspect=/台湾小说网|google|记住首发网站域名|本书由|sudu|首发更新|请记住“””|69[^\r\n]{0,30}[.．]|[\u00a4\u1d00-\u1dbf\u2100-\u24ff\u2776-\u2788\ud835\ud83c\uff10-\uff19\uff21-\uff3a\uff41-\uff5a]/i;
    // All templates are confined to one line. Avoid per-character mapping of
    // ordinary narrative paragraphs, which is expensive inside Rhino.
    for(var line=0;line<lines.length;line++)if(suspect.test(lines[line]))lines[line]=cfCleanTwkanLine(lines[line]);
    return lines.join('\n');
}
function cfCleanTwkanLine(value) {
    var original=String(value),key='',starts=[],ends=[],ranges=[];
    var confusable={'ѕ':'s','χ':'x','м':'m','ᗰ':'m','๏':'o','¢':'c','ᑌ':'u','ɡ':'g'};
    for(var i=0;i<original.length;){
        var begin=i,point=original.charCodeAt(i++),piece=original.charAt(begin);
        if(point>=0xd800&&point<=0xdbff&&i<original.length){
            var low=original.charCodeAt(i);
            if(low>=0xdc00&&low<=0xdfff){point=0x10000+((point-0xd800)<<10)+low-0xdc00;piece=original.substring(begin,++i);}
        }
        if(point>=0x1f150&&point<=0x1f169)piece=String.fromCharCode(97+point-0x1f150);
        else if(point>=0x1f170&&point<=0x1f189)piece=String.fromCharCode(97+point-0x1f170);
        else if(point>=0x2776&&point<=0x277e)piece=String(point-0x2775);
        else if(point>=0x2780&&point<=0x2788)piece=String(point-0x277f);
        else if((point>=0xc0&&point<=0x2ff)||(point>=0x1d00&&point<=0x1dbf)||(point>=0x1e00&&point<=0x1eff)||(point>=0x2100&&point<=0x24ff)||(point>=0xff00&&point<=0xffef)||(point>=0x1d400&&point<=0x1d7ff)){
            piece=String(Packages.java.text.Normalizer.normalize(piece,Packages.java.text.Normalizer.Form.NFKD));
        }
        piece=piece.toLowerCase();
        for(var j=0;j<piece.length;j++){
            var c=piece.charAt(j);
            if(/[\u0300-\u036f\u1ab0-\u1aff\u1dc0-\u1dff\u20d0-\u20ff\ufe20-\ufe2f\u200b-\u200d\u2060\ufeff]/.test(c)){
                if(ends.length)ends[ends.length-1]=i;
                continue;
            }
            key+=confusable[c]||c;starts.push(begin);ends.push(i);
        }
    }
    function add(from,to){if(to>from)ranges.push({start:starts[from],end:ends[to-1]});}
    function match(pattern,check){var found;while((found=pattern.exec(key))!==null){if(!check||check.test(found[0]))add(found.index,found.index+found[0].length);}}
    var site=/台湾小说网|twkan\.com/;
    match(/[【\[](?:写到这里我希望读者记一下我们域名|记住本站域名)[^】\]\r\n]{0,260}[】\]]/g,site);
    match(/\(请记住[^()\r\n]{0,260}网站,[ \t]*观看最快的章节更新\)/g,site);
    match(/本书首发[^\r\n]{0,260}?提供给你无错章节,[ \t]*无乱序章节的阅读体验/g,site);
    match(/google[ \t]*搜索[ \t]*twkan/g);
    match(/记住首发网站域名[ \t]*twkan\.com/g);
    match(/本书由[ \t]*twkan\.com[ \t]*全网首发/g);
    match(/(?:(?:找|读)台湾好书(?:上|选)|(?:看|追|读)台湾小说(?:就上|就来|就去|选))?台湾小说网(?:藏书[全多]|解书荒|伴你[闲读]|体验[棒佳]|超(?:贴心|便捷|顺畅))?[,→][ \t]*twkan\.com(?:超(?:方便|省心|实用|贴心|靠谱|流畅|给力|赞)|等你(?:读|寻)|任你选|随时享|轻松(?:读|看))?/g);
    match(/sudugu\.cc首发更新,无错字。看不到地址输入的拼音后缀\.cc即可进入首发更新站点。/g);
    // This observed damaged notice has no usable domain left; keep the match
    // exact rather than generalizing it to ordinary quotation marks or prose.
    match(/请记住“””小”说”唯一“网”址”:“”””\.”””””””\.”””,站“长有且只“有这一个网“站,其它网“站随“时断“更。/g);
    var domain=/69shux\.com/g,found;
    var decoration=/[ \t\u00a0\u2000-\u200a\u00b8\u00f8\u00a4\u00ba\u00b0`,\u2600-\u27ff\u25a0-\u25ff\u2022\u00b7\ud83c-\ud83e\udc00-\udfff]/;
    while((found=domain.exec(key))!==null){
        var from=found.index,to=from+found[0].length,left=from,right=to;
        if(/[a-z0-9.-]/.test(key.charAt(from-1))||/[a-z0-9.-]/.test(key.charAt(to)))continue;
        while(left>0&&from-left<64&&decoration.test(key.charAt(left-1)))left--;
        while(right<key.length&&right-to<64&&decoration.test(key.charAt(right)))right++;
        // Decorative envelopes are stripped only when present on both sides of
        // this known watermark, never by deleting arbitrary emoji-only lines.
        if(key.substring(left,from).replace(/[ \t\u00a0\u2000-\u200a]/g,'')&&key.substring(to,right).replace(/[ \t\u00a0\u2000-\u200a]/g,'')){from=left;to=right;}
        add(from,to);
    }
    if(!ranges.length)return original;
    ranges.sort(function(a,b){return a.start-b.start;});
    var output='',cursor=0;
    for(var r=0;r<ranges.length;r++){
        var range=ranges[r];
        if(range.start>cursor)output+=original.substring(cursor,range.start);
        if(range.end>cursor)cursor=range.end;
    }
    return output+original.substring(cursor);
}
function cfDocument(html) {
    return Packages.org.jsoup.Jsoup.parse(String(html), CF_SITE + '/');
}
function cfMeta(doc, name) {
    var element = doc.selectFirst('meta[property="' + name + '"]');
    return element ? String(element.attr('content')) : '';
}
function cfBook(doc, url) {
    var wordCount = String(doc.select('.booknav2 p').text()).match(/[\d.]+萬字/);
    return {bookUrl: url || cfMeta(doc, 'og:url'),
            name: cfMeta(doc, 'og:title'), author: cfMeta(doc, 'og:novel:author'),
            kind: cfMeta(doc, 'og:novel:category') + ' / ' + cfMeta(doc, 'og:novel:status'),
            intro: cfMeta(doc, 'og:description'), coverUrl: cfMeta(doc, 'og:image'),
            latestChapterTitle: cfMeta(doc, 'og:novel:latest_chapter_name'),
            wordCount: wordCount ? wordCount[0] : '',
            tocUrl: CF_SITE + '/ajax_novels/chapterlist/' + cfMeta(doc, 'og:book_id') + '.html'};
}
function cfBookList(doc) {
    if (cfMeta(doc, 'og:book_id')) return [cfSimple(cfBook(doc))];
    var items = doc.select('#article_list_content > li'), books = [], seen = {};
    if (items.isEmpty()) items = doc.select('.newbook li, .booklist li, .bookcase li');
    for (var i = 0; i < items.size(); i++) {
        var item = items.get(i), link = item.selectFirst('h3 a[href*="/book/"], h4 a[href*="/book/"]');
        if (!link) continue;
        var url = String(link.absUrl('href'));
        if (seen[url]) continue;
        seen[url] = true;
        var image = item.selectFirst('img'), labels = item.select('.labelbox label');
        books.push({bookUrl: url, name: cfText(item.selectFirst('h3,h4')),
                    author: labels.size() ? String(labels.get(0).text()) : cfText(item.selectFirst('h4')),
                    kind: labels.size() > 1 ? String(labels.get(1).text()) : '',
                    intro: String(item.select('ol').text()),
                    latestChapterTitle: String(item.select('.zxzj a').text()),
                    coverUrl: image ? String(image.hasAttr('data-src') ? image.absUrl('data-src') : image.absUrl('src')) : ''});
    }
    if (!books.length) {
        var links = doc.select('a[href*="/book/"]');
        for (var j = 0; j < links.size(); j++) {
            var candidate = links.get(j), target = String(candidate.absUrl('href')), name = String(candidate.text()).trim();
            if (!name || seen[target] || !/\/book\/\d+\.html$/.test(target)) continue;
            seen[target] = true;
            var heading=candidate.selectFirst('h3,h4');
            books.push({bookUrl:target,name:heading?cfText(heading):name,author:cfText(candidate.selectFirst('h4')),coverUrl:cfImage(candidate)});
        }
    }
    return cfSimple(books);
}
function search(key, page) {
    return cfBookList(cfDocument(cfRead(CF_SITE + '/search?searchkey=' + encodeURIComponent(java.s2t(String(key))) + '&searchtype=all&page=' + (Number(page) || 1))));
}
function explore(url,page){return cfExplore(String(url),page,cfBookList);}
function getBookInfo(book) {
    return cfSimple(cfBook(cfDocument(cfRead(String(book.bookUrl))), String(book.bookUrl)));
}
function getChapters(book) {
    var id = String(book.bookUrl).match(/\/book\/(\d+)/);
    if (!id) throw '无法识别书籍 ID';
    var doc = cfDocument(cfRead(CF_SITE + '/ajax_novels/chapterlist/' + id[1] + '.html'));
    var links = doc.select('li a[href*="/txt/"]'), chapters = [], seen = {};
    for (var i = 0; i < links.size(); i++) {
        var link = links.get(i), url = String(link.absUrl('href'));
        if (seen[url]) continue;
        seen[url] = true;
        chapters.push({title: String(link.text()).trim(), url: url});
    }
    if (!chapters.length) throw '完整目录为空';
    return cfSimple(chapters);
}
function getContent(chapter, book, nextChapterUrl) {
    var doc = cfDocument(cfRead(String(chapter.url))), content = doc.selectFirst('#txtcontent0');
    if (!content) throw '没有找到正文容器';
    var canvases = content.select('canvas.sec-last[data-c][data-v]');
    for (var i = 0; i < canvases.size(); i++) {
        var canvas = canvases.get(i), nonce = String(canvas.attr('data-v'));
        var bytes = java.base64DecodeToByteArray(String(canvas.attr('data-c')));
        for (var j = 0; j < bytes.length; j++) {
            var value = (bytes[j] & 255) ^ CF_READER_KEY.charCodeAt(j % CF_READER_KEY.length) ^ nonce.charCodeAt(j % nonce.length);
            bytes[j] = value > 127 ? value - 256 : value;
        }
        var decoded = String(java.bytesToStr(bytes, 'UTF-8'));
        if (decoded.indexOf('\ufffd') >= 0) throw '正文末段编码发生变化，停止返回不完整正文';
        canvas.replaceWith(new Packages.org.jsoup.nodes.TextNode(String(cfDocument(decoded).text())));
    }
    content.select('script,style,.txtad,.txtcenter,ins').remove();
    var html = String(content.html()).replace(/<br\s*\/?\s*>/gi, '\n');
    var text = String(cfDocument(html).wholeText()).replace(/\n[ \t\u2003\u3000]*/g, '\n').replace(/\n{3,}/g, '\n\n').trim();
    if (!text) throw '正文为空';
    return cfCleanChapterText(cfCleanTwkanContent(String(java.t2s(text))),chapter.title,cfText(doc.selectFirst('h1')),book&&book.name);
}
