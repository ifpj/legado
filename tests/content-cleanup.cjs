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
];
for(const [line,catalog,heading,book] of cases){
 const cleaned=ctx.cfCleanChapterText(line+'\n\n'+body,catalog,heading,book);
 assert.equal(cleaned,body,'metadata-equivalent heading '+line);
 assert.equal(ctx.cfCleanChapterText(cleaned,catalog,heading,book),cleaned,'idempotent');
}
assert.equal(ctx.cfCleanChapterText('第1章 绯红\n第1章 绯红\n'+body,'1.第1章 绯红','',''),body);
const internal=body+'\n\n他提到“第1章 绯红”，又翻过一页。\n\n第1章 绯红\n\n这行是角色正在读的书中内容。';
assert.equal(ctx.cfCleanChapterText(internal,'第1章 绯红','',''),internal,'do not globally erase title references');
assert.equal(ctx.cfCleanChapterText('第1章 绯红'+body,'第1章 绯红','',''),'第1章 绯红'+body,'whole title line only');
assert.equal(ctx.cfCleanChapterText('第1章 绯红','第1章 绯红','',''),'第1章 绯红','never return an empty chapter');
assert.equal(ctx.cfCleanChapterText(body+'\n\n(本章完)','第1章','',''),body);
assert.equal(ctx.cfCleanChapterText(body+'\n\n\u2003(本章完)','第1章','',''),body);
assert.equal(ctx.cfCleanChapterText(body+'(本章完)','第1章','',''),body);
assert.equal(ctx.cfCleanChapterText('甲\b乙\t丙\n丁\r\n戊','','',''),'甲乙\t丙\n丁\r\n戊','remove controls without deleting words');
assert.equal(ctx.cfCleanChapterText('半夏小说\n\n'+body,'','半夏小说',''),'半夏小说\n\n'+body,'ignore site h1 as chapter metadata');
assert.equal(ctx.cfCleanChapterText('7.5K\n\n'+body,'第1章','',''),'7.5K\n\n'+body,'retain word-count notes');
vm.runInContext(fs.readFileSync(path.join(__dirname,'../assets/banxia-parser.js'),'utf8'),ctx);
assert.equal(ctx.bxCleanContent(body+'\n\n半夏小说，快乐很多'),body+'\n\n');
assert.equal(ctx.bxCleanContent('甲nbsp;nbsp;。乙'),'甲。乙','entity remnants only; preserve punctuation');
assert.equal(ctx.bxCleanContent('她说“半夏小说，快乐很多”，随后打开书页。'),'她说“半夏小说，快乐很多”，随后打开书页。','only standalone slogan');
assert.equal(ctx.bxCleanContent('浏♂览♂器♂搜索\n\n\\（书名\\）.+?\\{♂,,♂\\} 可/以快速找到你在看的/书！\n\n'+body),'\n\n'+body);
assert.equal(ctx.bxCleanContent(body+'\n\n♂章节前往章节前往♂.+?\\{?/?♂\\}♂输\\入\\网\\址：.{\\ＷＷＷ··}\n\n/♂\n\n半夏小说，快乐很多'),body+'\n\n');
console.log('Verified common content cleanup: differing heading counters/prefixes, running book headers, numeric chapters, interior narrative protection, controls, footer boundaries and exact Banxia notices.');
