var CF_READER_KEY='jieqi2026abcd12';
// Delete site-name paragraphs regardless of the random slogan or domain font.
// Other known watermarks use a detection copy without rewriting surrounding text.
function cfCleanTwkanContent(value) {
    var symbol='(?:[ஜ۩۞ඏൠ\\u2190-\\u2bff]|[\\ud83c-\\ud83e][\\udc00-\\udfff])(?:\\ufe0f)?';
    // A bracketed promotion can share a line with the end of a story paragraph.
    // Remove that explicit wrapper first; unwrapped site-name paragraphs below
    // are still discarded regardless of their generated slogan or domain.
    value=String(value).replace(/[（(][^（）()\r\n]*[台臺][湾灣]小[说說][网網][^（）()\r\n]*[）)]|【[^【】\r\n]*[台臺][湾灣]小[说說][网網][^【】\r\n]*】|\[[^\[\]\r\n]*[台臺][湾灣]小[说說][网網][^\[\]\r\n]*\]/g,'');
    var lines=String(value).replace(/(?:^|\n)本章想法[\s\S]*上APP[，,]和更多书友互动\s*$/i,'').replace(/(?:^|\n)报名[^\n。！？!?]{1,30}(?:课程|培训)\s*$/,'').replace(/loadAdv\s*\(\s*\d+\s*,\s*\d+\s*\)\s*;/gi,'').replace(new RegExp('(?:'+symbol+'){2}\\u2003{2}(?:'+symbol+'){2}','g'),'').replace(new RegExp('((?:'+symbol+'){2,})\\u2003+\\1','g'),'').replace(/(^|\n)[^\S\r\n]*(?:\]>|>)\s*(?:[©®]\s*)?$/,'$1').split('\n');
    // Empty generated watermarks keep the mirrored ornament but lose the
    // domain. Require the EM SPACE centre and exact reversed punctuation.
    for(var empty=0;empty<lines.length;empty++)lines[empty]=lines[empty].replace(/([,.*'"^~_`|!+\- ]{6,})\u2003+([,.*'"^~_`|!+\- ]{6,})/g,function(all,left,right){return left.split('').reverse().join('')===right?'':all;});
    // The site name is the deletion marker. Random slogans and domain fonts
    // do not change the decision; the entire marked paragraph is discarded.
    lines=lines.filter(function(line){var marker=line.replace(/[\s\u200b-\u200d\u2060\ufeff“”"'⊥]/g,'');return !/[台臺][湾灣]小[说說][网網]|记住更新地址不迷路|本网站分享到(?:Facebook|脸书)|最新小说在.*首发[!！]|邀请码.*官方反馈群\d+/i.test(marker)&&!/^小说网(?:首发|手打更新)[.a-z0-9]*$|^前往必(?:应)?搜索$|^(?:原文|(?:更多|最新|热门).*小说)在看[!！。]?$/i.test(marker)&&!/^喜欢(.+?)请大家收藏[:：][\s\S]*\1\s*$/.test(line)&&!/^\s*(?:[:：]+|地址[:：][\\/.\s]+)\s*$/.test(line);});
    var suspect=/google|搜[“”]*索|记住首发网站域名|本书由|sudu|首发更新|首[“”]*发|请|“德”“旗”|吧书69新|mo[ɔc][.．]|6[】\]]|69[^\r\n]{0,30}[.．]|[\u00a4-\u1fff\u2070-\u24ff\u2776-\u2788\ud835\ud83c\uff10-\uff19\uff21-\uff3a\uff41-\uff5a]/i;
    // All templates are confined to one line. Avoid per-character mapping of
    // ordinary narrative paragraphs, which is expensive inside Rhino.
    for(var line=0;line<lines.length;line++)if(suspect.test(lines[line]))lines[line]=cfCleanTwkanLine(lines[line]);
    return lines.join('\n');
}
function cfCleanTwkanLine(value) {
    var original=String(value),key='',starts=[],ends=[],ranges=[];
    var confusable={},glyphs={'6':'ϭ',s:'ѕร丂ƨʂᔕꜱꞩꙅ',h:'ħнᕼ卄ⱨ♄ʜңһђ',u:'ᑌยυㄩцʊ',x:'χж乂᙭хҳ✘〤×',c:'¢ςᑕ匚ƈϲɔ',o:'๏øσοоㄖ口ᗝ',m:'мᗰ爪μɱ♏๓ϻ',g:'ɡ'};
    for(var letter in glyphs)for(var glyph=0;glyph<glyphs[letter].length;glyph++)confusable[glyphs[letter].charAt(glyph)]=letter;
    for(var i=0;i<original.length;){
        var begin=i,point=original.charCodeAt(i++),piece=original.charAt(begin);
        if(point>=0xd800&&point<=0xdbff&&i<original.length){
            var low=original.charCodeAt(i);
            if(low>=0xdc00&&low<=0xdfff){point=0x10000+((point-0xd800)<<10)+low-0xdc00;piece=original.substring(begin,++i);}
        }
        if(point>=0x1f130&&point<=0x1f149)piece=String.fromCharCode(97+point-0x1f130);
        else if(Packages.java.lang.Character.digit(point,10)>=0)piece=String(Packages.java.lang.Character.digit(point,10));
        else if(point>=0x1f150&&point<=0x1f169)piece=String.fromCharCode(97+point-0x1f150);
        else if(point>=0x1f170&&point<=0x1f189)piece=String.fromCharCode(97+point-0x1f170);
        else if(point>=0x2776&&point<=0x277e)piece=String(point-0x2775);
        else if(point>=0x2780&&point<=0x2788)piece=String(point-0x277f);
        else if((point>=0xc0&&point<=0x52f)||(point>=0x1d00&&point<=0x1dbf)||(point>=0x1e00&&point<=0x1eff)||(point>=0x2070&&point<=0x24ff)||(point>=0xff00&&point<=0xffef)||(point>=0x1d400&&point<=0x1d7ff)){
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
    match(/google[ \t]*搜索[ \t]*twkan/g);
    match(/搜索[a-z0-9.]+(?:com|cc|org)阅读[◆]?/g);
    match(/记住首发网站域名[ \t]*twkan\.com/g);
    match(/本书由[ \t]*twkan\.com[ \t]*全网首发/g);
    match(/“德”“旗”“小”“说”“网”“手打”“更新”|吧书69新/g);
    match(/[a-z0-9.]*首发更新[^\r\n]*?即可进入首发更新站点[。.]?/g);
    function quoted(text){return text.split('').join('[“”" ]*');}
    match(new RegExp('(?:'+quoted('请收藏')+'|'+quoted('请记住')+')[^\\r\\n]*?'+quoted('其它网站随时断更')+'[“”。]*','g'),new RegExp(quoted('唯一网址')));
    // Keep a positional index while discarding ornament separators. Chinese
    // narrative remains a boundary; matching the fixed marker needs no wildcards.
    var domainChars=[],positions=[],fontChars=[],fontPositions=[];
    for(var k=0;k<key.length;k++){
        if(/[a-z0-9\u3400-\u9fff]/.test(key.charAt(k))){domainChars.push(key.charAt(k));positions.push(k);}
        if(/[a-z0-9.\u00c0-\u1fff\u3400-\u9fff]/.test(key.charAt(k))){fontChars.push(key.charAt(k));fontPositions.push(k);}
    }
    var domainKey=domainChars.join(''),domain=/69shuxco?m|mocxuhs[69]{1,2}/g,found,domains=[];
    // The inserted separators and letter case are random. Match the promotion
    // in the compact index, then remove only its original span: some ads share
    // a line with the next story paragraph.
    var notice=/请(?:访问|进入?|到|前往)[a-z0-9看读阅最当前新章节址地网站小说]*?deqixsorg(?:网站[\u3400-\u9fff]*?最新章节)?|(?:前往)?(?:必应|百度|google)搜索[a-z0-9\u3400-\u9fff]*?可?(?:看|阅读|查看)(?:本书|小说)?最新(?:更新)?章节/g;
    while((found=notice.exec(domainKey))!==null){
        var noticeEnd=positions[found.index+found[0].length-1]+1;
        var noticeTail=key.substring(noticeEnd).match(/^[^a-z0-9\u3400-\u9fff\r\n]*[!！]/);
        if(noticeTail)noticeEnd+=noticeTail[0].length;
        else while(/[.．)）]/.test(key.charAt(noticeEnd)))noticeEnd++;
        add(positions[found.index],noticeEnd);
    }
    while((found=domain.exec(domainKey))!==null)domains.push({from:positions[found.index],to:positions[found.index+found[0].length-1]+1});
    // Unknown non-ASCII font glyphs are allowed in their fixed slots; already
    // restored ASCII letters must agree, so unrelated 69xxxx domains stay intact.
    var fontKey=fontChars.join(''),fontDomain=/69([a-z\u00c0-\u1fff]{4})\.([a-z\u00c0-\u1fff]{3})/g;
    while((found=fontDomain.exec(fontKey))!==null){
        var letters=found[1]+found[2],expected='shuxcom',valid=/[^a-z]/.test(letters);
        for(var f=0;f<letters.length;f++)if(/[a-z]/.test(letters.charAt(f))&&letters.charAt(f)!==expected.charAt(f))valid=false;
        if(valid)domains.push({from:fontPositions[found.index],to:fontPositions[found.index+found[0].length-1]+1});
    }
    var decoration=/[\sღ‿⁀⁔ஜ۩۞ඏൠ\u00b8\u00f8\u00a4\u00ba\u00b0\u00a7\u00a8\u00af\u00b4\u00d7\u00f7`,'~!*_|.\-«»<>()v【】\[\]\u200b-\u200f\u2060\ufe0e\ufe0f\u1d00-\u1dbf\u2070-\u209f\u2190-\u2bff\u2022\u00b7\ud83c-\ud83e\udc00-\udfff]/;
    for(var d=0;d<domains.length;d++){
        var from=domains[d].from,to=domains[d].to,left=from,right=to;
        if(/[a-z0-9.-]/.test(key.charAt(from-1))||/[a-z0-9.-]/.test(key.charAt(to)))continue;
        while(left>0&&decoration.test(original.substring(starts[left-1],ends[left-1])))left--;
        while(right<key.length&&decoration.test(original.substring(starts[right],ends[right])))right++;
        if(/-漫[~*'¨¯·]*舞[~*'¨¯· ]*$/.test(original.substring(0,starts[from]))&&/^[~*'¨¯· ]*舞[~*'¨¯·]*漫-/.test(original.substring(ends[to-1]))){
            while(left>0&&(/[漫舞]/.test(key.charAt(left-1))||decoration.test(original.substring(starts[left-1],ends[left-1]))))left--;
            while(right<key.length&&(/[漫舞]/.test(key.charAt(right))||decoration.test(original.substring(starts[right],ends[right]))))right++;
        }
        // Decorative envelopes are stripped only when present on both sides of
        // this known watermark, never by deleting arbitrary emoji-only lines.
        if(key.substring(left,from).replace(/\s/g,'')&&key.substring(to,right).replace(/\s/g,'')){from=left;to=right;}
        // Some watermark generators append a short, padded Chinese label.
        var label=original.substring(ends[to-1]).match(/^[ \u3000]+（[\u3400-\u9fff]{1,4}[ \u3000]+）/);
        if(label){var labelEnd=ends[to-1]+label[0].length;while(to<key.length&&starts[to]<labelEnd)to++;}
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
