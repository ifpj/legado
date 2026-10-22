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
 '请记住“””小”说”唯一“网”址”：“”””.”””””””.”””，站“长有且只“有这一个网“站，其它网“站随“时断“更。'
];
for(const ad of ads){
 assert.equal(clean(ad),'','complete watermark: '+ad);
 assert.equal(clean('𠮷说：“café，继续。”'+ad+'她回头👩🏽‍🚀，轻声作答。'),'𠮷说：“café，继续。”她回头👩🏽‍🚀，轻声作答。','only remove the injected span');
 assert.equal(clean(clean(ad)),clean(ad),'idempotent');
}
const keep=[
 '他打开 Google 搜索天气，稍后搜索 TWKAN 的含义。','GOOGLE搜索天气。','TWKAN 是他写下的五个字母。',
 '她提到台湾小说网这个名字，随后转身离开。','本书首发于出版社，后附作者感言。',
 '【记住本站域名的人忽然回头】','（请记住我的话，明天再见。）',
 '1\n97\n😲✌\nno.1。','café ẗẅḳäṅ naïve e\u0301 𠮷 👩🏽‍🚀。',
 '她记下 https://example.com/a 和 https://69shux.com.example.org/ 。',
 '这个变量叫169shux.comparison，另一个是69shux.comx。','作者的话：明天请假，感谢支持和月票。'
];
for(const text of keep)assert.equal(clean(text),text,'preserve narrative: '+text);
assert.equal(clean(ads[0]+'甲'+ads[1]+'乙'+ads[2]),'甲乙');
assert.equal(clean('甲'+ads[0]+'\n\n乙'),'甲\n\n乙');
assert.equal(clean('【记住本站域名 台湾小说网'+('甲'.repeat(300))+'】'),'【记住本站域名 台湾小说网'+('甲'.repeat(300))+'】','bounded malformed notices');
// Use the actual production content callback; cleanup must follow the native
// converter, since Traditional Chinese brand text is recognized after t2s.
context.cfRead=()=>'<html>fixture</html>';
context.cfDocument=html=>String(html)==='fixture body'?{wholeText:()=> '甲GOOGLE搜索TWKAN乙'}:{selectFirst:selector=>selector==='h1'?null:({select:()=>({size:()=>0,remove:()=>{}}),html:()=> 'fixture body'})};
context.java={t2s:value=>value};
assert.equal(context.getContent({url:'fixture'}), '甲乙');
console.log('Verified Taiwan cleanup: 23 observed/variant advertisements, exact surrounding narrative and Unicode preservation, bounded matching, idempotence and production content callback.');
