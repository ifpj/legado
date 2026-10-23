var CF_READER_KEY='jieqi2026abcd12';
// Delete site-name paragraphs regardless of the random slogan or domain font.
// Other known watermarks use a detection copy without rewriting surrounding text.
function cfCleanTwkanContent(value) {
    // Match the fixed first-publication watermark even when its characters
    // are separated by inserted ones or invisible spacing. Keep adjacent prose.
    var splitNotice=['[无無]','[错錯]','首','[发發]','[内內]','容','在','看'].join('[ \\t\\u00a0\\u2000-\\u200d\\u2060\\u3000\\ufeff一]*');
    value=String(value).replace(new RegExp(splitNotice+'[！!。.]?','g'),'');
    // This bracketed promotion can lose both its site name and domain glyphs.
    value=value.replace(/【[ \t]*[记記]住全[网網]最快小[说說]站[^【】\r\n]*】/g,'');
    value=value.replace(/\{[^{}\r\n]{1,300}\}/g,function(block){return block.indexOf('|')>=0&&/(?:无错章节|無錯章節)/.test(block)&&/google/i.test(block)?'':block;});
    // Imported promotional spans can be glued into a normal story paragraph.
    value=String(value).replace(/(?:您|你)收到了(?:一个)?(?:新的)?章节更新[:：][^\r\n]*?阅读链接[。.]?/g,'').replace(/(?:我们)?郑重向[您你]推荐本书[:：][^\r\n]*?阅读地址[。.]?/g,'').replace(/下一章更精彩[:：][^\r\n]*?期待[您你]的(?:光临|阅读)[。.]?/g,'').replace(/[，,]{2,}\s*畅读《[^》\r\n]+》等万千好书[。.]?/g,'').replace(/[，,]?您的一站式小说阅读港湾[。.]?/g,'').replace(/[^，,。！？!?\r\n]{1,40}笔下的世界[，,]尽在《[^》\r\n]+》[。.]?/g,'').replace(/《[^》\r\n]+》\s*[-—]\s*文笔惊艳[，,]情节跌宕起伏[！!]?/g,'').replace(/阅读盛宴[:：]海量图书、极致体验[，,]?[。.]?/g,'').replace(/最新章节《[^》\r\n]*》已更新[，,]速来(?:可乐小说)?追更[！!]?/g,'').replace(/[，,]读《[^》\r\n]+》[，,]享受阅读时光[。.]?/g,'').replace(/[^，,。！？!?\r\n]{1,40}力作《[^》\r\n]+》[，,]点击立即阅读[！!]?/g,'').replace(/独家[！!][^。！？!?\r\n]{1,50}专访及《[^》\r\n]+》创作幕后[，,]仅限[。.]?/g,'').replace(/<\/?ins\b[^>]*>/gi,'');
    // The anti-reader template may be injected in the middle of a paragraph.
    // Remove only its fixed wording and the adjacent loading marker so that
    // story text before and after it remains intact.
    value=value.replace(/当[^\u3400-\u9fff\r\n]?前[^\u3400-\u9fff\r\n]?章[^\u3400-\u9fff\r\n]?节[^\u3400-\u9fff\r\n]?内[^\u3400-\u9fff\r\n]?容[^\u3400-\u9fff\r\n]?不[^\u3400-\u9fff\r\n]?完[^\u3400-\u9fff\r\n]?整[！!]?要[^\u3400-\u9fff\r\n]?查[^\u3400-\u9fff\r\n]?看[^\u3400-\u9fff\r\n]?完[^\u3400-\u9fff\r\n]?整[^\u3400-\u9fff\r\n]?章[^\u3400-\u9fff\r\n]?节[^\u3400-\u9fff\r\n]?请[^\u3400-\u9fff\r\n]?退[^\u3400-\u9fff\r\n]?出[^\u3400-\u9fff\r\n]?阅[^\u3400-\u9fff\r\n]?读[^\u3400-\u9fff\r\n]?模[^\u3400-\u9fff\r\n]?式[！!]?(?:加载中[.。…]+)?[ \t\u3000]*/g,'').replace(/(^|[。！？!?：:，,；;])加载中[.。…]+\u3000{2,}/g,function(all,before,at,whole){var prefix=whole.substring(whole.lastIndexOf('\n',at)+1,at),pairs=[['【','】'],['[',']'],['“','”'],['「','」']];for(var guard=0;guard<pairs.length;guard++)if(prefix.lastIndexOf(pairs[guard][0])>prefix.lastIndexOf(pairs[guard][1]))return all;return before;});
    // A complete, separate title/synopsis/tag block at the chapter end is a
    // book-promotion footer, not part of the preceding story paragraph.
    value=value.replace(/(?:^|\n)《[^》\r\n]+》\s*\n+\s*简介[:：]\s*\n[\s\S]+?\n\s*标签[:：](?:【[^】\n]+】)+\s*(?:\n+\s*[（(]本章完[）)])?\s*$/,'');
    value=String(value).replace(/<i\s+class=(["'])icon\s+icon-uni[0-9a-f]+\1\s*>\s*<\/i>/gi,'');
    value=String(value).replace(/(?:^|\n)[ \t]*SES[ \t]+S[ \t]+E[ \t]+n[ \t]*(?=\n|$)/g,'');
    var symbol='(?:[ஜ۩۞ඏൠ\\u2190-\\u2bff]|[\\ud83c-\\ud83e][\\udc00-\\udfff])(?:\\ufe0f)?';
    // The generated four-symbol envelope may be glued to story prose. Keep it
    // when it is inside a bracketed chat message; otherwise it is site debris.
    function stripEmojiEnvelope(text,pattern){return text.replace(pattern,function(){var all=arguments[0],offset=arguments[arguments.length-2],whole=arguments[arguments.length-1],line=whole.substring(whole.lastIndexOf('\n',offset)+1,offset),open=line.lastIndexOf('【'),close=line.lastIndexOf('】');return open>close?all:'';});}
    value=stripEmojiEnvelope(value,new RegExp('(?:'+symbol+'){2}\\u2003{2}(?:'+symbol+'){2}','g'));
    value=stripEmojiEnvelope(value,new RegExp('((?:'+symbol+'){2,})\\u2003+\\1','g'));
    value=value.replace(/[░▒▓█]+►─═\u2003+\s*═─◄[█▓▒░]+/g,'');
    // A bracketed promotion can share a line with the end of a story paragraph.
    // Remove that explicit wrapper first; unwrapped site-name paragraphs below
    // are still discarded regardless of their generated slogan or domain.
    value=String(value).replace(/[（(][^（）()\r\n]*[台臺][湾灣]小[说說][网網][^（）()\r\n]*[）)]|【[^【】\r\n]*[台臺][湾灣]小[说說][网網][^【】\r\n]*】|\[[^\[\]\r\n]*[台臺][湾灣]小[说說][网網][^\[\]\r\n]*\]/g,'');
    var lines=String(value).replace(/(?:^|\n)本章想法[\s\S]*上APP[，,]和更多书友互动\s*$/i,'').replace(/(?:^|\n)(?:报名[^\n。！？!?]{1,30}(?:课程|培训)|阅读[^\n。！？!?]{1,30}书籍)\s*$/,'').replace(/loadAdv\s*\(\s*\d+\s*,\s*\d+\s*\)\s*;/gi,'').replace(/(^|\n)[^\S\r\n]*(?:\]>|>)\s*(?:[©®]\s*)?$/,'$1').split('\n');
    // A fixed 69 + four glyphs + dot + three glyphs shape is the source
    // watermark when any glyph is non-ASCII. This covers newly generated
    // fonts without treating ordinary domains such as 69news.com as ads.
    for(var shaped=0;shaped<lines.length;shaped++)lines[shaped]=lines[shaped].replace(/69(....)\.(...)/gi,function(all,left,right){var letters=left+right,expected='shuxcom',nonAscii=false;for(var glyph=0;glyph<letters.length;glyph++){var code=letters.charCodeAt(glyph);if(code>=0x3400&&code<=0x9fff)return all;if(code>127)nonAscii=true;else if(!/[a-z]/i.test(letters.charAt(glyph))||letters.charAt(glyph).toLowerCase()!==expected.charAt(glyph))return all;}return nonAscii?'':all;});
    // A bare ornament line is generated site debris. The same four-symbol
    // shape can be a real emoji message inside brackets, so bracketed text is
    // deliberately left untouched.
    var emojiEnvelope=new RegExp('^(?:'+symbol+'){2}\\u2003{2}(?:'+symbol+'){2}$');
    var mirroredEmoji=new RegExp('^((?:'+symbol+'){2,})\\u2003+\\1$');
    for(var ornament=0;ornament<lines.length;ornament++)if(emojiEnvelope.test(lines[ornament])||mirroredEmoji.test(lines[ornament]))lines[ornament]='';
    // Empty generated watermarks keep the mirrored ornament but lose the
    // domain. Require the EM SPACE centre and exact reversed punctuation.
    for(var empty=0;empty<lines.length;empty++)lines[empty]=lines[empty].replace(/([,.*'"^~_`|!+\- ]{6,})\u2003+([,.*'"^~_`|!+\- ]{6,})/g,function(all,left,right){return left.split('').reverse().join('')===right?'':all;});
    // Some empty watermarks use Latin-extended punctuation instead of emoji.
    // Run this after the reversed-ASCII detector so a palindromic substring
    // cannot consume the centre of a larger reversed ornament.
    for(var paired=0;paired<lines.length;paired++)lines[paired]=stripEmojiEnvelope(lines[paired],/([^\u3400-\u9fffA-Za-z0-9\r\n]{4,24})\u2003+\1/g);
    for(var tail=0;tail<lines.length;tail++)lines[tail]=lines[tail].replace(/[,.*'"^~_`|!+\- ]{6,}$/,function(all){return /,/.test(all)&&/[-*'"^~_`|!+]/.test(all)?'':all;});
    lines=lines.filter(function(line){var marker=line.replace(/[\s\u200b-\u200d\u2060\ufeff“”"'‘’]/g,'');return !/可[乐樂]小[说說].*最忠实的阅读伴侣|人人[书書][库庫]|记住这个域名|免费读全本.*链接|强力推荐.*点击直达|APP.*最新更新章节.*访问APP官网|^(?:点击|收藏)[，,].*《[^》]+》|^最新(?:更新|章节)[，,].*(?:上线|解读|阅读)|^最新章节《[^》]*》剧情高能[！!].*快来[！!]|^最新章节已就位[！!].*书迷速归|^最新剧情[:：].*点击追更|^欢迎来到.*小说的奇幻大陆[，,]入口在此|^预告[:：]即将更新[，,].*关注|无错版本.*(?:在读|首发)|首发本小说|^专业的小说网站[，,]提供最舒适的阅读体验|^立即阅读第\d+章.+打开今日精彩|书荒[?？].*小说小说推荐吧|这不是广告[，,]是宝藏书籍《[^》]+》的安利|^[^，,。！？!?]{1,40}的铁粉们[，,]《[^》]+》最新章节已发布|(?:本章未完|本小章还未完|这个章节后面还有).*(?:下一页|继续阅读)|^请点击下一页继续|^探索.*小说分类|^阅读盛宴[:：]|^(?:——)?您的私人掌上图书馆[，,]随时访问|^本章第\d+章.+点我立即解锁|书友都在讨论区[，,]畅聊.*小说小说的魅力|^>[←→]$/i.test(marker);});
    // A damaged import occasionally prefixes a bracketed skill name with an
    // ASCII run and a duplicated CJK glyph. It is safe to remove only this
    // narrow prefix; the bracketed skill and following sentence are retained.
    for(var noise=0;noise<lines.length;noise++)lines[noise]=lines[noise].replace(/^[a-z]{3,}([\u3400-\u9fff])\1(?=【)/i,'');
    for(var inserted=0;inserted<lines.length;inserted++)lines[inserted]=lines[inserted].replace(/女仆小第(\d+)章/g,'女仆小$1');
    // The site name is the deletion marker. Random slogans and domain fonts
    // do not change the decision; the entire marked paragraph is discarded.
    lines=lines.filter(function(line){var marker=line.replace(/[\s\u200b-\u200d\u2060\ufeff“”"'⊥]/g,'');return !/[台臺][湾灣]小[说說][网網]|番茄小说网|记住更新地址不迷路|本网站分享到(?:Facebook|脸书)|最新小说在.*首发[!！]|邀请码.*官方反馈群\d+/i.test(marker)&&!/^小说网(?:首发|手打更新)[.a-z0-9]*$|^前往必(?:应)?搜索$|^(?:原文|(?:更多|最新|热门).*小说)在看[!！。]?$/i.test(marker)&&!/^喜欢(.+?)请大家收藏[:：][\s\S]*\1\s*$/.test(line)&&!/^\s*(?:[:：]+|地址[:：][\\/.\s]+)\s*$/.test(line);});
    // A missing site-name prefix can leave a catalogue slogan and a damaged
    // domain. Require all three slots, repeated replacement glyphs and a dot.
    lines=lines.filter(function(line){return !/^(?:书库|书海|藏书)[\u3400-\u9fff]{1,6}[，,](?=[a-z0-9?？.．]*[?？]{2})(?=[a-z0-9?？.．]*[.．])[a-z0-9?？.．]{3,200}(?:超省心|超方便|超实用|超给力|任你选|任你挑|随时享)[!！。]?$/i.test(line.trim());});
    var suspect=/google|搜|本书由|sudu|首|请|“德”“旗”|吧书69新|mo[ɔc][.．]|6|[\u00a4-\u1fff\u2070-\u24ff\u2776-\u2788\ud835\ud83c\uff10-\uff19\uff21-\uff3a\uff41-\uff5a]/i;
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
        if(confusable[piece])piece=confusable[piece];
        else if(point>=0x1f130&&point<=0x1f149)piece=String.fromCharCode(97+point-0x1f130);
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
    // Run after whole site-name paragraphs have been removed, and keep the
    // opening/closing quote boundaries of any adjacent story dialogue.
    match(/“[“”" ]*首[“”" ]+[发發][“”" ]{2,}[.．。][“”" ]*”/g);
    match(/google[ \t]*搜索[ \t]*twkan/g);
    match(/搜索[a-z0-9.]+(?:com|cc|org)阅读[◆]?/g);
    match(/记住首发网站域名[ \t]*twkan\.com/g);
    match(/记住首发网站域名[ \t]*[^ \t\r\n\u3400-\u9fff]*/g);
    match(/[ \t\u2003\u3000]*本书由[ \t]*twkan\.com[ \t]*全网首发/g);
    match(/“德”“旗”“小”“说”“网”“手打”“更新”|吧书69新/g);
    match(/[a-z0-9.]*首发更新[^\r\n]*?即可进入首发更新站点[。.]?/g);
    function quoted(text){return text.split('').join('[“”" ]*');}
    var splitDomain='[“”"\u2018\u2019【】\\[\\] ]*';
    var splitSite=['[得德]','[奇旗]','小','[说說]','[网網]'].join(splitDomain)+splitDomain;
    match(new RegExp('(?:“[“”"]*|【|\\[)?'+'(?:'+splitSite+')?'+'首发deqixs.org'.split('').map(function(c){return c==='.'?'\\.':c==='发'?'[发發]':c;}).join(splitDomain)+'[”"\u2019】\\]]*','g'));
    match(new RegExp('(?:'+quoted('请收藏')+'|'+quoted('请记住')+')[^\\r\\n]*?'+quoted('其它网站随时断更')+'[“”。]*','g'));
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
    // The generator sometimes wraps the domain in a short .o0×X×0o. envelope.
    // These characters are decoration only when adjacent to an already-known
    // domain; they are never removed as a standalone pattern.
    var decoration=/[\sღ‿⁀⁔ஜ۩۞ඏൠ\u00b8\u00f8\u00a4\u00ba\u00b0\u00a7\u00a8\u00af\u00b4\u00d7\u00f7`,'~!*_|.\-«»<>()vOoXx0【】\[\]\u200b-\u200f\u2060\ufe0e\ufe0f\u1d00-\u1dbf\u2070-\u209f\u2190-\u2bff\u2022\u00b7\ud83c-\ud83e\udc00-\udfff]/;
    var mirrorDecoration=/[ \t~˜°•.*'"“”‘’\u0060_^|!+\-]/;
    for(var d=0;d<domains.length;d++){
        var from=domains[d].from,to=domains[d].to,left=from,right=to;
        if(/[a-z0-9.-]/.test(key.charAt(from-1))||/[a-z0-9.-]/.test(key.charAt(to)))continue;
        // Curly quotes belong to this ornament only when both original spans
        // mirror each other around a verified domain. Do not consume quotes
        // from adjacent story dialogue by adding them to the generic trim set.
        var rawFrom=starts[from],rawTo=ends[to-1],mirrorStart=rawFrom,mirrorEnd=rawTo,mirrored=false;
        while(mirrorStart>0&&rawFrom-mirrorStart<80&&mirrorDecoration.test(original.charAt(mirrorStart-1)))mirrorStart--;
        while(mirrorEnd<original.length&&mirrorEnd-rawTo<80&&mirrorDecoration.test(original.charAt(mirrorEnd)))mirrorEnd++;
        for(var width=Math.min(rawFrom-mirrorStart,mirrorEnd-rawTo);width>=6;width--){
            var ornament=original.substring(rawFrom-width,rawFrom);
            if(ornament.trim().length>=6&&/[˜°•*]/.test(ornament)&&ornament.split('').reverse().join('')===original.substring(rawTo,rawTo+width)){
                ranges.push({start:rawFrom-width,end:rawTo+width});mirrored=true;break;
            }
        }
        if(mirrored)continue;
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
        if(key.charAt(from-1)==='‘'&&key.charAt(to)==='’'){from--;to++;}
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
