'use strict';
const assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path'),vm=require('node:vm');
const context=vm.createContext({Packages:{java:{text:{Normalizer:{Form:{NFKD:'NFKD'},normalize:(text,form)=>String(text).normalize(form)}}}}});
vm.runInContext(fs.readFileSync(path.join(__dirname,'../assets/gateway-source.js'),'utf8'),context);
vm.runInContext(fs.readFileSync(path.join(__dirname,'../assets/twkan-parser.js'),'utf8'),context);
const clean=context.cfCleanTwkanContent;
const ads=[
 'GOOGLE搜索TWKAN','ＧＯＯＧＬＥ搜索ＴＷＫＡＮ','Google 搜索 TwKan',
 '记住首发网站域名𝕥𝕨𝕜𝕒𝕟.𝕔𝕠𝕞','本书由𝕥𝕨𝕜𝕒𝕟.𝕔𝕠𝕞全网首发',
 '【写到这里我希望读者记一下我们域名 台湾小说网书库全，𝕥𝕨𝕜𝕒𝕟.𝕔𝕠𝕞任你选 】',
 '【写到这里我希望读者记一下我们域名台湾小说网→🅣🅦🅚🅐🅝.🅒🅞🅜】',
 '【记住本站域名 追台湾小说就上台湾小说网，🆃🆆🅺🅰🅽.🅲🅾🅼超实用 】',
 '（请记住 台湾小说网体验佳，𝔱𝔴𝔨𝔞𝔫.𝔠𝔬𝔪超给力 网站，观看最快的章节更新）',
 '本书首发 台湾小说网体验棒，ẗẅḳäṅ.ċöṁ超赞 ,提供给你无错章节，无乱序章节的阅读体验',
 '本书首发 台湾小说网藏书全，𝗍𝗐𝗄𝖺𝗇.𝖼𝗈𝗆随时享 ,提供给你无错章节，无乱序章节的阅读体验',
 '台湾小说网→𝐭𝐰𝐤𝐚𝐧.𝐜𝐨𝐦','台湾小说网藏书多，🅣🅦🅚🅐🅝.🅒🅞🅜超方便',
 '找台湾好书上台湾小说网，t͎͎w͎͎k͎͎a͎͎n͎͎.c͎͎o͎͎m͎͎超方便',
 '𝟨𝟫𝒔𝒉𝒖𝒙.𝒄𝒐𝒎','𝟨𝟫𝐬𝐡𝐮𝐱.𝐜𝐨𝐦',
 '¸,ø¤º°`°º¤ø,¸ ➅９𝕊ＨＵⓍ.Ｃ๏ᗰ ¸,ø¤º°`°º¤ø,¸',
 '◦•●◉✿ ６❾𝐬ʰ𝕦𝓍.¢Ⓞм ✿◉●•◦','♔🎄 ➅➈ѕ𝕙ᑌχ.𝓒𝓞Ｍ ✎☠',
 '♟👌 ❻❾ˢ𝓗Ⓤx.Ćᵒ𝓂 👺👤','💋♩ ➅➈𝔰Ĥ𝓊𝔵.𝒸𝐨𝓂 💣👍',
 'ｓuduɡu.ｃｃ首发更新，无错字。看不到地址输入的拼音后缀.ｃｃ即可进入首发更新站点。',
 '请记住“””小”说”唯一“网”址”：“”””.”””””””.”””，站“长有且只“有这一个网“站，其它网“站随“时断“更。',
 '【6】【9】【s】【h】【u】【x】【.】【c】【o】【m】','69🅂🄷🅄🅇.🄲🄾🄼',
 '👍☺ ❻❾𝕤ĤｕЖ.𝓒Øм 🌷★','💔🐉 ➅❾ˢⓗⓤＸ.Ćσ𝐦 💔🍓',
 '|!¤*\'~``~\'*¤!| ６❾𝔰ħ𝓾x.𝕔Ⓞⓜ |!¤*\'~``~\'*¤!|',
 '✊👊 ➅➈ŜĤǗˣ.ς𝓞𝓂 ♟🐳','§.•´¨\'°÷•..× ➅９𝓼Ⓗยχ.𝕔ｏ𝐦 ×,.•´¨\'°÷•..§',
 '¸,ø¤º°`°º¤ø,¸ ➅９𝓼𝒽𝐔乂.ᑕ๏𝓂 ¸,ø¤º°`°º¤ø,¸',
 '🍓 ⋆ 🍦 🎀 𝟨𝟫𝓈𝒽𝓊𝓍.𝒸🌸𝓂 🎀 🍦 ⋆ 🍓',
 '“德”“旗”“小”“说”“网”“手打”“更新”','吧书69新',
 '«-(¯`v´¯)-« ６➈丂𝕙Ǘ乂.ςⓄⓜ »-(¯`v´¯)-»',
 '🎀♠ ➅❾𝐒нỮ𝔁.匚๏𝐦 👌♨','⁶⁹ˢʰᵘˣ.ᶜᵒᵐ',
 '°°°·.°·..·°¯°·._.· ６➈𝓈𝔥υЖ.Čσ𝕞 ·._.·°¯°·.·° .·°°°',
 '👺✊ ➅❾ŞＨᑌ᙭.ᑕό𝓜 🎀👌','👹☝ ６❾丂ℍù𝔵.ᶜỖ𝓜 🐧🍮',
 '🍑 ⋆ 🍭 🎀 𝟨𝟫𝓈𝒽𝓊𝓍.𝒸☯𝓂 🎀 🍭 ⋆ 🍑',
 '请收“藏“”“”小”说”唯”一“网”址”：““””.””“””””.”“””，“站”“长”只“有这一个网“站，其它网“站随“时断“更。”',
 '请收“藏“得”“奇”小”说”唯”一“网”址”：“w“w”w”.”d”e“q”i”x”s”.”“o”r”g，“站”“长”只“有这一个网“站，其它网“站随“时断“更。”',
 '▁ ▂ ▄ ▅ ▆ ▇ █ ６➈shux.com █ ▇ ▆ ▅ ▄ ▂ ▁',
 '╰☆☆ ❻➈𝕊𝐇𝐮ⓧ.𝓬ｏ爪 ☆☆╮','💜🍫 ➅９ѕᕼᵘχ.Ć๏ｍ ♞♨',
 '✌👣 ６９ˢ𝓗Ǘ𝓧.ᶜᗝΜ 💛🍭','🐠😳 ⑥⑨ⓢⓗⓤⓧ   .⃝ⓒⓞⓜ 🐸🐨',
 '🐚♠ ❻９ˢнυｘ.ᑕ𝕠๓ ♥😂','↤↤↤↤↤ ➅➈Ｓᕼ𝓾𝕏.匚๏ϻ ↦↦↦↦↦',
 '¤¸¸.•´¯`•¸¸.•..>> 69shux.com <<..•.¸¸•´¯`•.¸¸¤',
 '-漫~*\'¨¯¨\'*·舞69shux.com舞*\'¨¯¨\'*·~漫-',
 '-漫~*\'¨¯¨\'*·舞~ ➅❾𝓢нᵘ᙭.𝓒𝐎𝓶 ~舞*\'¨¯¨\'*·~漫-',
 'moɔ.xuʜꙅ9','搜索𝐬𝐭𝐨𝟓𝟐𝟎𝐜𝐨𝐦阅读◆',
 '∙∙·▫▫ᵒᴼᵒ▫ₒₒ▫ᵒᴼᵒ▫ₒₒ▫ᵒᴼᵒ 69shux.com ᵒᴼᵒ▫ₒₒ▫ᵒᴼᵒ▫ₒₒ▫ᵒᴼᵒ▫▫·∙∙',
 'ඏ69shux.comඏ','≋6≋9≋s≋h≋u≋x≋.≋c≋o≋m≋','╰☆☆ ６❾𝕊ђᵘЖ.Ｃᵒм ☆☆╮',
 '69ֆɦʊӼ.ƈօʍ','♨🐚 ➅❾𝐒ʰ𝓤א.𝐂𝕆𝐦 🐝♗','ღ(¯`◕‿69shux.com‿◕´¯)ღ'
];
for(const ad of ads){
 assert.equal(clean(ad),'','complete watermark: '+ad);
 assert.equal(clean('𠮷说：“café，继续。”'+ad+'她回头👩🏽‍🚀，轻声作答。'),ad.includes('台湾小说网')?'':'𠮷说：“café，继续。”她回头👩🏽‍🚀，轻声作答。','site-name paragraphs are removed; other inline watermarks retain surrounding prose');
 assert.equal(clean(clean(ad)),clean(ad),'idempotent');
}
const paragraphs=[
 '台湾小说网书海量，₮₩₭₳₦.₵Ø₥任你挑',
 '台湾小说网书海量，TWKAN.COM任你挑',
 '台湾小说网书海量，T̶W̶K̶A̶N̶.C̶O̶M̶任你挑',
 '台湾小说网书海浩瀚，𝓽𝔀𝓴𝓪𝓷.𝓬𝓸𝓶任君畅读',
 '台湾小说网藏书丰富，₮₩₭₳₦.₵Ø₥等你发现',
 '台湾小说网海量好书，任你随心挑选',
 '台湾小说网书库辽阔，尽享阅读乐趣',
 '台湾小说网阅读体验出色，╬Ꞩꟻ.𐌂𐌏𐌌随时享受',
 '台湾小说网好书不断，T W K A N . C O M 等你探索',
 '看好书就来台湾小说网，₮₩₭₳₦.₵Ø₥随时畅读',
 '更多精彩尽在台湾小说网，₮₩₭₳₦.₵Ø₥任你挑',
 '台湾小说网书海浩瀚，₮₩₭₳₦.₵Ø₥',
 '台湾小说网→₮₩₭₳₦.₵Ø₥',
 '【台湾小说网书海无边，₮₩₭₳₦.₵Ø₥任你读】',
 '　台湾小说网海量好书，任你选择！　',
 '台灣小說網書海量，₮₩₭₳₦.₵Ø₥任你挑',
 '臺灣小說網：任意新的随机修饰词，不需要网址',
 '台\u200b湾 小 说 网：随机字体、随机前后缀都不参与判断',
 '完全未知的前缀，台湾小说网，完全未知的后缀',
 '台湾小说网的藏书里，有他一直在找的线索。',
 '台湾小说网（https://twkan.com）是案件的关键线索。',
 '台湾小说网更新了网站公告，消息很快传到他耳中。',
 '他说：“台湾小说网书海量，₮₩₭₳₦.₵Ø₥任你挑。”随后合上电脑。',
 '她在台湾小说网上读完了这本书，终于明白那些更新日期的意义。',
 '请到台湾小说网查找这本书的作者，不必购买。',
 '【记住本站域名 台湾小说网'+('甲'.repeat(300))+'】',
 '记住更新地址不迷路：???.、.、.、.、??.?或者必应搜索：',
 '【麻烦您动动手指，把本网站分享到Facebook脸书，这样我们能坚持运营下去】',
 '最⊥新⊥小⊥说⊥在⊥⊥⊥首⊥发！',
 '“”“”“小”“说”“网”“首”“发”“”“”“”“”“”“”“.”“”“”“”','前往必搜索','原文在看！','””“小”“说”“网”“手打”“更新”','更多最新热门小说在看！'
];
for(const ad of paragraphs){
 assert.equal(clean(ad),'','delete every paragraph bearing the site name: '+ad);
 assert.equal(clean('她推开门。\n\n'+ad+'\n\n他抬起头。'),'她推开门。\n\n\n他抬起头。','keep both neighbouring paragraphs');
 assert.equal(clean(clean(ad)),clean(ad),'paragraph cleanup is idempotent');
}
const keep=[
 '他打开 Google 搜索天气，稍后搜索 TWKAN 的含义。','GOOGLE搜索天气。','TWKAN 是他写下的五个字母。',
 '她提到这个网站名字，随后转身离开。','本书首发于出版社，后附作者感言。',
 '【记住本站域名的人忽然回头】','（请记住我的话，明天再见。）',
 '1\n97\n😲✌\nno.1。','café ẗẅḳäṅ naïve e\u0301 𠮷 👩🏽‍🚀。',
 '她记下 https://example.com/a 和 https://69shux.com.example.org/ 。',
 '这个变量叫169shux.comparison，另一个是69shux.comx。','作者的话：明天请假，感谢支持和月票。',
 '台湾文学作品在图书馆的书架上。','故事里的网站是69news.com，另一段文字是69fօօx.com。'
];
for(const text of keep)assert.equal(clean(text),text,'preserve narrative: '+text);
assert.equal(clean(ads[0]+'甲'+ads[1]+'乙'+ads[2]),'甲乙');
assert.equal(clean('甲'+ads[0]+'\n\n乙'),'甲\n\n乙');
for(const marker of ['♬💢  💘♧','🐠😳  🐸🐨','♙♕  😲✌','😾🐺  ൠ✌','ஜ۩۞۩ஜ ஜ۩۞۩ஜ'])assert.equal(clean('甲'+marker+'乙'),'甲乙','empty generated emoji envelope');
assert.equal(clean('正常正文。\n\n本章想法读者A：评论内容，今天14:13-上海0\n\n上APP，和更多书友互动'),'正常正文。\n','remove imported comment footer');
assert.equal(clean('作者的话：本章想法很多，下次再写。'),'作者的话：本章想法很多，下次再写。');
// Use the actual production content callback; cleanup must follow the native
// converter, since Traditional Chinese brand text is recognized after t2s.
context.cfRead=()=>'<html>fixture</html>';
context.cfDocument=html=>String(html)==='fixture body'?{wholeText:()=> '甲GOOGLE搜索TWKAN乙'}:{selectFirst:selector=>selector==='h1'?null:({select:()=>({size:()=>0,remove:()=>{}}),html:()=> 'fixture body'})};
context.java={t2s:value=>value};
assert.equal(context.getContent({url:'fixture'}), '甲乙');
console.log('Verified Taiwan cleanup: whole-paragraph site-name removal, simplified/traditional and invisible spacing variants, independent of random slogans/domains; other watermarks, neighbouring paragraphs and Unicode preserved.');
