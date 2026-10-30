'use strict';
const assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path'),vm=require('node:vm');
const context=vm.createContext({Packages:{java:{text:{Normalizer:{Form:{NFKD:'NFKD'},normalize:(text,form)=>String(text).normalize(form)}},lang:{Character:{digit:(point)=>{for(const start of [0x30,0x660,0x6f0,0x966,0xe50,0xff10])if(point>=start&&point<start+10)return point-start;return -1;}}}}}});
vm.runInContext(fs.readFileSync(path.join(__dirname,'../../assets/sources/gateway-source.js'),'utf8'),context);
vm.runInContext(fs.readFileSync(path.join(__dirname,'../../assets/sources/twkan-parser.js'),'utf8'),context);
const clean=context.cfCleanTwkanContent;
const ads=[
 '无一错一首一发一内一容一在一一看！',
 '無一錯一首一發一內一容一在一一看！',
 '无\u200b一错\u2060一首一发一内一容一在一一看！',
 '【记住全网最快小说站 就来 ，????????????.??????超方便 】',
 '【記住全網最快小說站 新口号，example.invalid】',
 '˜”*°•.˜”*°• 69shux.com •°*”˜.•°*”˜',
 '“”“”“首”“发”“”“”“”“”“”“”“.”“”“”“”',
 '{什么？你还不知道|阅读ＣＯＭ,无错章节阅读|赶紧google一下吧}',
 '【得】【奇】【小】【说】【网】【首】【发】“d”“e”“q”“i”“x”“s”“.”“o”“r”“g”',
 '[德]“旗”【小】[說]“網”【首】【發】[d][e][q][i][x][s][.][o][r][g]',
 '🐤🍟 ➅➈ⓢнυ𝓧.℃ⓞ๓ 💘💜',
 '.o0×X×0o. ➅９ŜĦᑌ𝔁.ᑕ𝕠𝐦 .o0×X×0o.',
 '“”“首”“发”“d”“e”“q”“i”“x”“s”“.”“o”“r”“g”',
 '‘6’‘9’‘s’‘h’‘u’‘x’‘.’‘c’‘o’‘m’',
 '请记住“”小”说”唯一址”：“w”w”w”.”d”e”q”i”x”s”.”o”r”g，站“长有且只“有这一个网“站，其它网“站随“时断“更。',
 '“”“首”“发”【d】【e】【q】【i】【x】【s】【.】【o】【r】【g】',
 '记住首发网站域名??????????.??????',
 '记住首发网站域名unknown.example.org',
 '我们郑重向您推荐本书：《任意新书》，阅读地址。',
 '下一章更精彩：第197章 任意标题，期待您的光临。',
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
 assert.equal(clean('𠮷说：“café，继续。”'+ad+'她回头👩🏽‍🚀，轻声作答。'),ad.includes('台湾小说网')&&!/^[【（(\[]/.test(ad)?'':'𠮷说：“café，继续。”她回头👩🏽‍🚀，轻声作答。','site-name paragraphs are removed; explicitly bracketed inline advertisements retain surrounding prose');
 assert.equal(clean(clean(ad)),clean(ad),'idempotent');
}
assert.equal(clean('他一字一句地说：“内容无错，第一版在书店看。”'),'他一字一句地说：“内容无错，第一版在书店看。”','preserve ordinary ones and separate publication words');
assert.equal(clean('无错首发内容\n\n在看星星。'),'无错首发内容\n\n在看星星。','do not match the watermark across paragraph boundaries');
assert.equal(clean('【他想建立全网最快小说站】\n\n“记住，先把故事写好。”'),'【他想建立全网最快小说站】\n\n“记住，先把故事写好。”','preserve story references without the bracketed promotion marker');
assert.equal(clean('她写下“˜”*°•.˜”*°•”作为暗号。'),'她写下“˜”*°•.˜”*°•”作为暗号。','preserve decorative story symbols without a verified domain');
assert.equal(clean('他说：“先走。”'+'“”“”“首”“发”“”“”“”“”“”“”“.”“”“”“”'+'“再见。”'),'他说：“先走。”“再见。”','preserve both adjacent dialogue quote boundaries');
assert.equal(clean('她选择了{继续|退出}，并在google上搜索故事。'),'她选择了{继续|退出}，并在google上搜索故事。','preserve ordinary braced choices and search mentions');
assert.equal(clean('【普通】【剧情】【首】【发】'),'【普通】【剧情】【首】【发】','split publication words alone do not identify an advertisement');
assert.equal(clean('他说：“先走。”'+'【得】【奇】【小】【说】【网】【首】【发】“d”“e”“q”“i”“x”“s”“.”“o”“r”“g”'+'“再见。”'),'他说：“先走。”“再见。”','split site marker preserves adjacent dialogue boundaries');
const paragraphs=[
 '书库广，????????????????n????.c????o????m????超省心',
 '藏书多，??tw??.??com超方便',
 '记住这个名字：。记住这个域名：。好书不迷路。',
 '免费读全本第337章 共同富裕，链接：。',
 '强力推荐《一本新书》！点击直达故事世界。',
 '在“人人书库”APP上可阅读《一本新书》无广告的最新更新章节，超一百万书籍全部免费阅读。即可访问APP官网',
 '无论何时何地，可乐小说（）都是您最忠实的阅读伴侣。',
 '收藏，随时随地继续阅读《完全不同的书名》。',
 '点击，打开《另一部作品》的奇妙旅程。',
 '最新更新，已在上线，等待您的解读。',
 '预告：即将更新，请密切关注！',
 '无错版本在读！首发本小说。',
 '小主，这个章节后面还有哦，请点击下一页继续，后面更精彩！',
 '本章未完，请点击下一页继续阅读。',
 '本小章还未完，请点击下一页继续阅读后面精彩内容！',
 '请“点击” 下 一 页继续，查看更多内容。',
 '探索都市小说分类，总有一本适合你。',
 '阅读盛宴：海量图书、极致体验，。',
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
 assert.equal(clean('她推开门。\n\n'+ad+'\n\n他抬起头。').replace(/\n{3,}/g,'\n\n'),'她推开门。\n\n他抬起头。','keep both neighbouring paragraphs');
 assert.equal(clean(clean(ad)),clean(ad),'paragraph cleanup is idempotent');
}
const keep=[
 '书库广，里面有他需要的资料。','她写下“书库广，???.???超省心”作为广告分析例子。','书库广，example.com超省心','书库广，??没有域名??超省心',
 '她点击下一页继续读小说，直到窗外天亮。',
 '他打开 Google 搜索天气，稍后搜索 TWKAN 的含义。','GOOGLE搜索天气。','TWKAN 是他写下的五个字母。',
 '她提到这个网站名字，随后转身离开。','本书首发于出版社，后附作者感言。',
 '【记住本站域名的人忽然回头】','（请记住我的话，明天再见。）',
 '1\n97\n😲✌\nno.1。','café ẗẅḳäṅ naïve e\u0301 𠮷 👩🏽‍🚀。',
 '她记下 https://example.com/a 和 https://69shux.com.example.org/ 。',
 '这个变量叫169shux.comparison，另一个是69shux.comx。','作者的话：明天请假，感谢支持和月票。',
 '台湾文学作品在图书馆的书架上。','故事里的网站是69news.com，另一段文字是69fօօx.com。'
];
for(const text of keep)assert.equal(clean(text),text,'preserve narrative: '+text);
assert.equal(clean('“你知道她吗？”\u2003\u2003本书由𝕥𝕨𝕜𝕒𝕟.𝕔𝕠𝕞全网首发'),'“你知道她吗？”','remove only the whitespace adjoining the verified publication watermark');
assert.equal(clean(ads[0]+'甲'+ads[1]+'乙'+ads[2]),'甲乙');
assert.equal(clean('甲'+ads[0]+'\n\n乙'),'甲\n\n乙');
for(const marker of ['♬💢  💘♧','🐠😳  🐸🐨','♙♕  😲✌','😾🐺  ൠ✌','ஜ۩۞۩ஜ ஜ۩۞۩ஜ'])assert.equal(clean(marker),'','empty generated emoji ornament line');
assert.equal(clean('【我已经在路上了，还要十几分钟，到了联系你。😲🏆  ☝🐨】'),'【我已经在路上了，还要十几分钟，到了联系你。😲🏆  ☝🐨】','preserve emoji inside a real chat message');
assert.equal(clean('水露亚纳听完，表情认真的叫了一声。🍧☠  🐊♥'),'水露亚纳听完，表情认真的叫了一声。','remove a generated emoji envelope glued to prose');
for(const left of [",-*' ^ '~*-.,_,.-*~",'~*._.-^--^-.',"|'~*__*~'|"]){
 const envelope=left+'\u2003'+left.split('').reverse().join('');
 assert.equal(clean('甲'+envelope+'乙'),'甲乙','empty mirrored generated ornament');
 assert.equal(clean('***\n\n甲'),'***\n\n甲','ordinary scene separators remain');
}
for(const separator of ['', '.', ')', ' ', '+', '”', '\u200b']){
 for(const message of ['请访问看最新章节！地址：dEqixs.oRg','请进 www.deqixs.org 网站查看当前小说最新章节！']){
  const ad=message.split('').join(separator);
  assert.equal(clean(ad),'','promotion with generated separators: '+separator);
  assert.equal(clean('她推开门。'+ad+'她抬起头。'),'她推开门。她抬起头。','preserve prose sharing the promotion line');
  assert.equal(clean('她请到的客人来了。'+ad+'她抬起头。'),'她请到的客人来了。她抬起头。','do not start a promotion span inside preceding narrative');
 }
}
for(const fragment of ['地址：\\\\\\.\\\\','地址: / / . /','：',':：','最后一批邀请码1900人发放，官方反馈群1104270100','邀请码27人发放，官方反馈群123456']){
 assert.equal(clean(fragment),'','remove empty address or site invitation notice');
 assert.equal(clean('她推开门。\n'+fragment+'\n她抬起头。'),'她推开门。\n她抬起头。','keep adjacent story paragraphs');
}
for(const text of ['地址：北京市东城区。','他介绍地址：http://example.com/，请明天来。','官方反馈群里的人讨论了小说的剧情。','【静音，纲手】','……','***'])assert.equal(clean(text),text,'preserve real addresses, notes and scene breaks');
assert.equal(clean('甲loadAdv(11,0);乙loadAdv (2, 7);丙'),'甲乙丙','remove inert imported advertising calls');
assert.equal(clean('喜欢新书：另一段故事请大家收藏：()新书：另一段故事'),'','remove imported repeated-title collection footer');
assert.equal(clean('作者的话：喜欢这本书请大家收藏，感谢支持！'),'作者的话：喜欢这本书请大家收藏，感谢支持！','preserve author collection requests');
assert.equal(clean('【学贷的信封。不要打开！不要打开！不要打开！】'),'【学贷的信封。不要打开！不要打开！不要打开！】','preserve story illustration captions');
for(const ad of ['uuɡu.首发更新，无错字。看不到地址输入的拼音后缀.即可进入首发更新站点。','首发更新，无错字。看不到地址输入的拼音后缀即可进入首发更新站点。','new.example首发更新，随机新文案，不再依赖网址。即可进入首发更新站点。']){
 assert.equal(clean(ad),'','recognize promotion boundaries even when its domain is damaged');
 assert.equal(clean('她推开门。'+ad+'她抬起头。'),'她推开门。她抬起头。','preserve both sides of a damaged promotion');
}
for(const engine of ['必应','百度','Google'])for(const keyword of ['嘚齐','任意新关键词','newbook']){
 const ad=engine+'搜索“'+keyword+'”可看本书最新更新章节！';
 assert.equal(clean(ad),'','search promotion with a generated keyword');
 assert.equal(clean('甲'+ad+'乙'),'甲乙','preserve narrative around search promotion');
}
assert.equal(clean('前往“必”应”搜“索“德”旗””可查看最“新章“节！'),'','search wording and injected quotes vary');
for(const mark of ['\u0301','\u034e','\u1ab0','\u1dc0'])assert.equal(clean('甲'+Array.from('69shux.com').map(c=>c+mark).join('')+'乙'),'甲乙','combining marks must also trigger the normalization path');
assert.equal(clean('正常正文。\n\n本章想法读者A：评论内容，今天14:13-上海0\n\n上APP，和更多书友互动'),'正常正文。\n','remove imported comment footer');
assert.equal(clean('作者的话：本章想法很多，下次再写。'),'作者的话：本章想法很多，下次再写。');
for(const ending of ['>\n\n©','\n>\n®','\n]>'])assert.equal(clean('剧情完整。\n'+ending).trim(),'剧情完整。','remove only a trailing broken markup/copyright pair');
assert.equal(clean('正文中提到 © 2026 版权所有。'),'正文中提到 © 2026 版权所有。','preserve copyright mentioned in the story');
assert.equal(clean('剧情完整。\n\n报名生物课程'),'剧情完整。\n','remove unrelated course signup footer');
assert.equal(clean('他明天准备报名生物课程。'),'他明天准备报名生物课程。','keep course signups in narrative');
assert.equal(clean('剧情完整。\n\n阅读心理书籍'),'剧情完整。\n','remove unrelated reading recommendation footer');
assert.equal(clean('她准备阅读心理书籍。'),'她准备阅读心理书籍。','preserve reading described in narrative');
assert.equal(clean('作者的话：建议阅读心理书籍'),'作者的话：建议阅读心理书籍','preserve author recommendations');
assert.equal(clean('当晚的故事。\n\nSES S E n\n\n第二天早上。'),'当晚的故事。\n\n\n第二天早上。','remove verified standalone Latin noise marker');
assert.equal(clean('他在纸上写下 SES S E n。'),'他在纸上写下 SES S E n。','keep words quoted within narrative');
assert.equal(clean('眼眶都<i class="icon icon-uniE0D3"></i><i class="icon icon-uniE0D2"></i>了。'),'眼眶都了。','remove empty escaped source icon tags without inventing missing characters');
assert.equal(clean('</ins>\n正文\n</ins>'),'\n正文\n','remove malformed literal ad closing tags');
assert.equal(clean('加载中...　　还有，'),'还有，','remove leading loading marker');
assert.equal(clean('上文。当&前@章#节$内%容^不&完*整！要~查!看-完_整|章;节)请(退&出%阅#读|模*式！加载中...　　下文。'),'上文。下文。','remove an injected incomplete-chapter template without removing surrounding prose');
assert.equal(clean('阿妙宝宝。加载中...　　这时，光头兄弟走了出来。'),'阿妙宝宝。这时，光头兄弟走了出来。','remove a loading marker glued inside story prose');
assert.equal(clean('他摆了摆手：加载中...　　他顿了顿。'),'他摆了摆手：他顿了顿。','remove the verified indented loading marker after a colon');
assert.equal(clean('他说：“页面关了。加载中...　　稍等。”'),'他说：“页面关了。加载中...　　稍等。”','keep a loading status inside quoted dialogue');
for(const story of ['【记忆回溯·加载中......】','【门关了。加载中...　　请等待。】','屏幕上显示“加载中......”。','加载中......'])assert.equal(clean(story),story,'preserve narrative loading text without the verified injected template');
assert.equal(clean('【引力蚀刻】1v3\n\nnaaa里里【引力蚀刻】被提升到I3已经有一个多月'),'【引力蚀刻】1v3\n\n【引力蚀刻】被提升到I3已经有一个多月','remove a narrow damaged import prefix before a bracketed skill');
assert.equal(clean('书友都在讨论区，畅聊都市小说小说的魅力。'),'','remove imported discussion-area promotion');
assert.equal(clean('最新章节《》剧情高能！快来！'),'','remove empty-title update promotion');
assert.equal(clean('好在观众的眼睛是雪亮的，最新章节《》已更新，速来可乐小说追更！真的惊艳的演出不可能被埋没。'),'好在观众的眼睛是雪亮的，真的惊艳的演出不可能被埋没。','remove a glued update advert while retaining both story clauses');
for(const ad of ['书荒？来看看都市小说小说推荐吧！','这不是广告，是宝藏书籍《任意书名》的安利：。','独行特立的猪的铁粉们，《任意书名》最新章节已发布！','最新章节已就位！书迷速归。'])assert.equal(clean(ad),'','remove generated book/update promotion: '+ad);
assert.equal(clean('前文，，，畅读《任意书名》等万千好书。但后文继续。'),'前文但后文继续。','remove a glued book promotion and retain surrounding story');
assert.equal(clean('前文，您的一站式小说阅读港湾。后文。'),'前文后文。','remove a reading-harbour slogan glued to prose');
assert.equal(clean('前文。独行特立的猪笔下的世界，尽在《任意书名》。后文。'),'前文。后文。','remove an author-and-book promotion glued to prose');
assert.equal(clean('前文。《任意书名》 - 文笔惊艳，情节跌宕起伏！后文。'),'前文。后文。','remove a book praise promotion glued to prose');
assert.equal(clean('这绝对是第一次，阅读盛宴：海量图书、极致体验，。这么喜欢这个称呼'),'这绝对是第一次，这么喜欢这个称呼','remove a reading-feast slogan glued to prose');
for(const ad of ['——您的私人掌上图书馆，随时访问。','本章第709章 神仙打架有惊喜，点我立即解锁。'])assert.equal(clean(ad),'','remove reader-platform promotion: '+ad);
for(const ad of ['专业的小说网站，提供最舒适的阅读体验，。','立即阅读第357章 金曲奖也来凑热闹：，打开今日精彩。'])assert.equal(clean(ad),'','remove generic reading navigation: '+ad);
for(const ad of ['最新剧情：，点击追更。','欢迎来到都市小说的奇幻大陆，入口在此：。'])assert.equal(clean(ad),'','remove generated entry promotion: '+ad);
assert.equal(clean('前文，读《任意书名》，享受阅读时光。后文'),'前文后文','remove a read-this-book promotion glued to prose');
assert.equal(clean('啊？独行特立的猪力作《任意书名》，点击立即阅读！她愣住了。'),'啊？她愣住了。','remove an author promotion inside dialogue');
assert.equal(clean('前文。独家！某作者专访及《任意书名》创作幕后，仅限。后文。'),'前文。后文。','remove an exclusive-interview promotion glued to prose');
assert.equal(clean('“我怎么知道。░▒▓█►─═  ═─◄█▓▒░”'),'“我怎么知道。”','remove a mirrored block ornament inside dialogue');
assert.equal(clean('她并不吃惊。×º°”˜\`”°º× ×º°”˜\`”°º×\n\n剧情继续。'),'她并不吃惊。\n\n剧情继续。','remove a mirrored Latin-extended ornament glued to prose');
assert.equal(clean('【暗号×º°”˜\`”°º× ×º°”˜\`”°º×】'),'【暗号×º°”˜\`”°º× ×º°”˜\`”°º×】','preserve mirrored symbols inside a bracketed story message');
for(const ad of ['69ᏕᏂᏬጀ.ፈᎧᎷ','69§hµx.¢ðm'])assert.equal(clean('前文。'+ad),'前文。','remove a new non-ASCII 69 watermark font');
assert.equal(clean('地址是69news.com，请记住。'),'地址是69news.com，请记住。','keep an unrelated ASCII domain');
assert.equal(clean("清晨。,-*' ^ '~*-"),'清晨。','remove dangling watermark ornament after prose');
assert.equal(clean('九尾、强大、野性......'),'九尾、强大、野性......','preserve narrative ellipsis');
assert.equal(clean('李茶看向旁边的女仆小第4章 “今天午饭做得不好。”'),'李茶看向旁边的女仆小4 “今天午饭做得不好。”','repair an injected chapter marker inside a numbered character name');
assert.equal(clean('女仆小第4章 (*?.?*)?'),'女仆小4 (*?.?*)?','repair the same injected chapter marker before an emoticon');
assert.equal(clean('一套房子，最新章节《》已更新，速来追更！让她成了大家羡慕的对象。'),'一套房子，让她成了大家羡慕的对象。','remove an empty-title update advert without an optional site name');
assert.equal(clean('作者写下最后一句。\n\n——番茄小说网参与“年度人气作者”活动，为作者助力，有机会赢惊喜好礼！'),'作者写下最后一句。\n','remove a platform activity injection bearing its site name');
assert.equal(clean('甲。\n>←\n乙。'),'甲。\n乙。','remove verified standalone broken arrow marker');
assert.equal(clean('八点，您收到了一个新的章节更新：《第707章 任意章名》，阅读链接。歌手总决赛准时开始。'),'八点，歌手总决赛准时开始。','retain both ends of prose around a glued update promotion');
const bookPromo='《另一本新书》\n\n简介：\n\n一个全新故事的简介。\n\n标签：【奇幻】【冒险】';
assert.equal(clean('原有剧情完整。\n\n'+bookPromo+'\n\n(本章完)').trim(),'原有剧情完整。','remove a separate structured book-promotion footer');
assert.equal(clean('他翻开一本书。\n\n'+bookPromo+'\n\n他合上了书，继续赶路。'),'他翻开一本书。\n\n'+bookPromo+'\n\n他合上了书，继续赶路。','do not remove book descriptions inside continuing narrative');
for(const ad of ['（请记住 台湾小说网体验佳，随机字体域名超给力 网站，观看最快的章节更新）','【台湾小说网书海量，任你挑】','[台湾小说网换成全新随机口号]']){
 assert.equal(clean('不过咱想的就是自污名声，达到退婚的目的，没必要撒谎。'+ad),'不过咱想的就是自污名声，达到退婚的目的，没必要撒谎。','protect story text joined to a bracketed promotion');
}
assert.equal(clean('正常叙事。６９ｓｈｕｘ．ｃｏｍ　（谒演 ）'),'正常叙事。','remove a padded generated label attached to a watermark');
assert.equal(clean('正常叙事。69shux.com（人物在这里停顿。）'),'正常叙事。（人物在这里停顿。）','keep a real narrative parenthesis after the watermark');
assert.equal(clean('正常叙事。Ϭ९ʂհմ×.çօʍ'),'正常叙事。','mixed decimal scripts and unknown font slots share the known domain shape');
// Use the actual production content callback; cleanup must follow the native
// converter, since Traditional Chinese brand text is recognized after t2s.
context.cfRead=()=>'<html>fixture</html>';
context.cfDocument=html=>String(html)==='fixture body'?{wholeText:()=> '甲GOOGLE搜索TWKAN乙'}:{selectFirst:selector=>selector==='h1'?null:({select:()=>({size:()=>0,remove:()=>{}}),html:()=> 'fixture body'})};
context.java={t2s:value=>value};
assert.equal(context.getContent({url:'fixture'}), '甲乙');
console.log('Verified Taiwan cleanup: whole-paragraph site-name removal, simplified/traditional and invisible spacing variants, independent of random slogans/domains; other watermarks, neighbouring paragraphs and Unicode preserved.');
