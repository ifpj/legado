var CF_READER_KEY='jieqi2026abcd12';
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
    return java.t2s(text);
}
