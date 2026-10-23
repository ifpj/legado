'use strict';
const assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path'),vm=require('node:vm');
const common=fs.readFileSync(path.join(__dirname,'../assets/gateway-source.js'),'utf8');
const ctx=vm.createContext({});vm.runInContext(common,ctx);
const body='痛！\n\n好痛！\n\n他睁开眼睛，看见窗外的晨光照着书页。随后继续了昨天的故事。';
const cases=[
 ['第1章 绯红','1.第1章 绯红','1.第1章 绯红',''],
 ['第102章 苦尽又甘来','第一百零一章 苦尽又甘来','',''],
 ['第１２章　归来','第12章 归来','',''],
 ['第127章 127：归来','第127章 归来','',''],
 ['示例小说 第721章','第671章 归来','','示例小说'],
 ['001：','001','半夏小说',''],
 ['番外（三）','番外（三）','半夏小说',''],
 ['完结感言·新书预告','完结感言新书预告','完结感言新书预告','']
 ,['第章 症','第55章 症','半夏小说','']
 ,['第373章 一剑……只要一剑','第373章 一剑只要一剑','','']
 ,['第259章 先斩紫苑再灭镜王 (六千四求月票)','第259章 先斩“紫苑”再灭镜王 (六千四求月','第259章 先斩“紫苑”再灭镜王 (六千四求月票)','']
 ,['第442章 440：R.A.B','第442章 RAB','','']
 ,['第231章 斩，艾莉！（5.1K）','第231章 斩，艾莉！','','']
 ,['第104章 瑶瑶，周砚给你表白了吗？（第二更！）','第104章 瑶瑶，周砚给你表白了吗？（第二更','第104章 瑶瑶，周砚给你表白了吗？（第二更','']
 ,['第108章 出名要趁早啊！（为盟主一笑天晴加更14）','第108章 出名要趁早啊！（为盟主一笑天晴加','第108章 出名要趁早啊！（为盟主一笑天晴加','']
 ,['第290章 哇哦~好漂酿！（6k二合一）','第290章 哇哦好漂酿！（6k二合一）','','']
 ,['第430章 解决吞食恶龟，帝具香格里拉','第430章 解决吞食恶龟，帝具香格拉','','']
 ,['第232章 【一份相当不错的芽菜回锅肉】（一笑天晴白银大盟110）','第232章 【一份相当不错的芽菜回锅肉】（一','第232章 【一份相当不错的芽菜回锅肉】（一','']
 ,['第372章 羡慕→嫉妒→恨！（6.3k二合一）','第372章 羡慕嫉妒恨！（63k二合一）','','']
 ,['我替他给你道个歉','第101章 我替他给你道个歉','第101章 我替他给你道个歉','']
];
for(const [line,catalog,heading,book] of cases){
 const cleaned=ctx.cfCleanChapterText(line+'\n\n'+body,catalog,heading,book);
 assert.equal(cleaned,body,'metadata-equivalent heading '+line);
 assert.equal(ctx.cfCleanChapterText(cleaned,catalog,heading,book),cleaned,'idempotent');
}
assert.equal(ctx.cfCleanChapterText('第1章 绯红\n第1章 绯红\n'+body,'1.第1章 绯红','',''),body);
assert.equal(ctx.cfCleanChapterText('异香妙用！\n\n'+body,'第186章 异香妙用！','第186章 异香妙用！',''),body,'remove unnumbered repeated heading with punctuation');
const openingSpeech='什么破任务！！！\n\n陆绣看着前方魔力乱飙的两人，忍不住骂了一句。\n\n还下午茶！';
const openingSubject='哥布林，\n\n蟑螂般臭名昭著的它们，几乎分布在每一个奇幻位面当中。';
assert.equal(ctx.cfCleanChapterText(openingSubject,'第3章 哥布林','',''),openingSubject,'keep an opening subject with continuation punctuation');
assert.equal(ctx.cfCleanChapterText('万事俱备，冲锋号吹起（求月票）\n\n'+body,'第139章 万事俱备，冲锋号吹起（求月票）','',''),body,'normalize title notes with or without chapter number');
assert.equal(ctx.cfCleanChapterText('【银脉长弓】\n\n种类：弓\n\n效果：射击','第522章 【银脉长弓】','',''),'【银脉长弓】\n\n种类：弓\n\n效果：射击','keep a same-name item heading when it starts an attribute panel');
assert.equal(ctx.cfCleanChapterText('第260章 什么破任务\n\n'+openingSpeech,'第260章 什么破任务','',''),openingSpeech,'keep opening exclamation when the following paragraph attributes speech');
const internal=body+'\n\n他提到“第1章 绯红”，又翻过一页。\n\n第1章 绯红\n\n这行是角色正在读的书中内容。';
assert.equal(ctx.cfCleanChapterText(internal,'第1章 绯红','',''),internal,'do not globally erase title references');
assert.equal(ctx.cfCleanChapterText('第1章 绯红'+body,'第1章 绯红','',''),'第1章 绯红'+body,'whole title line only');
assert.equal(ctx.cfCleanChapterText('第1章 绯红','第1章 绯红','',''),'第1章 绯红','never return an empty chapter');
assert.equal(ctx.cfCleanChapterText(body+'\n\n(本章完)','第1章','',''),body);
assert.equal(ctx.cfCleanChapterText(body+'\n\n\u2003(本章完)','第1章','',''),body);
assert.equal(ctx.cfCleanChapterText(body+'(本章完)','第1章','',''),body);
assert.equal(ctx.cfCleanChapterText(body+'第11章绝对禁止这些行为！','第11章 绝对禁止这些行为！','',''),body,'remove an exact numbered heading glued after the final sentence');
assert.equal(ctx.cfCleanChapterText(body+'第１２章归来','第12章 归来','',''),body,'normalize fullwidth characters in a trailing heading');
for(const ending of ['他念道：“第11章绝对禁止这些行为！”','第12章绝对禁止这些行为！','第11章绝对禁止这些行为！他喊道。','他最后写下：第11章绝对禁止这些行为！','他说：“这是正文。第11章绝对禁止这些行为！']){
 const text=body+ending;
 assert.equal(ctx.cfCleanChapterText(text,'第11章 绝对禁止这些行为！','',''),text,'preserve quoted, contextual or nonmatching trailing title mentions');
}
assert.equal(ctx.cfCleanChapterText('甲\b乙\t丙\n丁\r\n戊','','',''),'甲乙\t丙\n丁\r\n戊','remove controls without deleting words');
assert.equal(ctx.cfCleanChapterText('半夏小说\n\n'+body,'','半夏小说',''),'半夏小说\n\n'+body,'ignore site h1 as chapter metadata');
assert.equal(ctx.cfCleanChapterText('7.5K\n\n'+body,'第1章','',''),'7.5K\n\n'+body,'retain word-count notes');
for(const line of ['——不如让欧尼谈恋爱试试？','自己的墓为什么不能去看？','“我替他给你道个歉。”'])assert.equal(ctx.cfCleanChapterText(line+'\n\n'+body,'第17章 '+line.replace(/^——|[“”]/g,''),'',''),line+'\n\n'+body,'preserve opening dialogue and questions even if reused as a title');
assert.equal(ctx.cfCleanChapterText('第431章 解决吞食恶龟，帝具香格里拉\n\n'+body,'第430章 解决吞食恶龟，帝具香格拉','',''),'第431章 解决吞食恶龟，帝具香格里拉\n\n'+body,'near titles require the same chapter number');
vm.runInContext(fs.readFileSync(path.join(__dirname,'../assets/banxia-parser.js'),'utf8'),ctx);
assert.equal(ctx.bxCleanContent(body+'\n\n半夏小说，快乐很多'),body+'\n\n');
assert.equal(ctx.bxCleanContent('甲nbsp;nbsp;。乙'),'甲。乙','entity remnants only; preserve punctuation');
assert.equal(ctx.bxCleanContent('她说“半夏小说，快乐很多”，随后打开书页。'),'她说“半夏小说，快乐很多”，随后打开书页。','only standalone slogan');
assert.equal(ctx.bxCleanContent('浏♂览♂器♂搜索\n\n\\（书名\\）.+?\\{♂,,♂\\} 可/以快速找到你在看的/书！\n\n'+body),'\n\n'+body);
assert.equal(ctx.bxCleanContent(body+'\n\n♂章节前往章节前往♂.+?\\{?/?♂\\}♂输\\入\\网\\址：.{\\ＷＷＷ··}\n\n/♂\n\n半夏小说，快乐很多'),body+'\n\n');
vm.runInContext(fs.readFileSync(path.join(__dirname,'../assets/69shuba-parser.js'),'utf8'),ctx);
ctx.cfDoc=()=>({selectFirst:selector=>selector==='h1'?'第1章 归来':{select:()=>({remove:()=>{}}),html:()=> 'fixture'}});
ctx.cfText=value=>String(value||'');
ctx.cfDocument=()=>({wholeText:()=> '　　第1章 归来\n\n\u2003\u2003第一段，保留 内部 空格。\n\n \t\u00a0第二段，继续保留正文。\n\n\u2002\u2004\u2009第三段，继续保持换段。'});
ctx.java={t2s:value=>String(value)};
assert.equal(ctx.getContent({url:'fixture',title:'第1章 归来'},{name:'测试书籍'}),'第一段，保留 内部 空格。\n\n第二段，继续保留正文。\n\n第三段，继续保持换段。','69 production callback removes Unicode paragraph indentation without touching interior spaces');
console.log('Verified common content cleanup: differing heading counters/prefixes, running book headers, numeric chapters, interior narrative protection, controls, footer boundaries and exact Banxia notices.');
