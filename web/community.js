'use strict';
const $=id=>document.getElementById(id),query=new URLSearchParams(location.search);
const state={scope:'book',page:0,cursor:null,busy:false,result:null,device:null,revision:0};
$('sort').value='1';$('para').value='0';
$('book').value=query.get('bookId')||'';$('chapter').value=query.get('chapterId')||'';
if(query.get('title'))$('title').textContent=query.get('title')+' · 书评与讨论';
function node(tag,text,cls){const n=document.createElement(tag);if(text!==undefined)n.textContent=text;if(cls)n.className=cls;return n;}
function image(src,cls){try{const u=new URL(src);if(!['http:','https:'].includes(u.protocol))return null;const img=node('img',undefined,cls);img.src=u.href;img.loading='lazy';img.alt=cls==='avatar'?'用户头像':'评论图片';img.referrerPolicy='no-referrer';return img;}catch{return null;}}
async function call(operation,args){
    // Credentials remain in the book source and never appear in the URL or page.
    const nativeRun=window.fanqieRun||window.run;
    if(typeof nativeRun==='function'){
        // WebJsExtensions uses the regular rule context, whose source is also
        // the login API. JSON-source callbacks name that same object sourceApi.
        const value=await nativeRun('var sourceApi=source;eval(String(source.mainJs));relayCall('+JSON.stringify(operation)+','+JSON.stringify(args)+');');
        return typeof value==='string'?JSON.parse(value):value;
    }
    const token=$('token').value||sessionStorage.getItem('relay.token')||'';
    const r=await fetch('/v1/call',{method:'POST',cache:'no-store',headers:{'Content-Type':'application/json','X-Relay-Client':'Community',...(token?{Authorization:'Bearer '+token}:{})},body:JSON.stringify({operation,args,device:state.device||{deviceType:'Android',deviceBrand:'Generic'},osVersion:'13'})});
    const next=r.headers.get('x-fanqie-state');if(next)state.device=JSON.parse(new TextDecoder().decode(Uint8Array.from(atob(next),c=>c.charCodeAt(0))));
    const data=await r.json();if(!r.ok)throw new Error(data.error||'HTTP '+r.status);return data;
}
function args(){return {book:{bookUrl:$('book').value.trim()},chapter:{url:$('chapter').value.trim()},scope:state.scope,paraIndex:Number($('para').value)||0,sort:Number($('sort').value)};}
function availability(){
    const hasChapter=!!$('chapter').value.trim();
    document.querySelectorAll('[data-scope]').forEach(b=>{b.disabled=b.dataset.scope!=='book'&&!hasChapter;b.classList.toggle('active',b.dataset.scope===state.scope);});
    $('chapter-hint').hidden=hasChapter;
    $('paragraph-picker').hidden=state.scope!=='paragraph';$('sort').hidden=state.scope==='chapter';
    $('para-label').hidden=state.scope!=='paragraph';$('para').hidden=true;
    document.querySelectorAll('[data-sort]').forEach(b=>b.classList.toggle('active',b.dataset.sort===$('sort').value));
}
function card(item,context,reply=false){
    const a=node('article',undefined,reply?'reply':''),head=node('div',undefined,'identity'),avatar=image(item.avatar,'avatar');
    if(avatar)head.append(avatar);head.append(node('span',item.name||'读者'));
    for(const badge of item.badge||[])head.append(node('span',badge,'badge'));a.append(head);
    const c=item.content||{};a.append(node('div',c.text||'', 'body'));
    for(const src of item.images||[]){const img=image(src,'picture');if(img)a.append(img);}
    const meta=node('div',undefined,'meta');meta.append(node('span',c.time||''),node('span','赞 '+(c.likeCount||0)),node('span','回复 '+(c.replyCount||0)));
    if(c.replyToName)meta.append(node('span','回复 '+c.replyToName));
    if(!reply&&(c.replyCount||0)>0){
        const button=node('button','查看回复'),wrap=node('div'),status=node('small');let page=1,cursor=null,loading=false;
        button.onclick=async()=>{
            if(loading)return;loading=true;button.disabled=true;status.textContent='';
            try{
                // Use the original card's chapter, paragraph version and server cursor.
                const request={...context,reviewId:item.id,replyContext:item.replyContext,page};
                if(page>1)request.cursor=cursor;
                const data=await call('review_replies',request);
                for(const r of data.items||[])wrap.append(card(r,context,true));
                cursor=data.cursor;page++;
                button.textContent=data.hasMore?'更多回复':'回复已加载';button.hidden=!data.hasMore;
                if(!data.items?.length&&page===2)status.textContent='暂无可见回复';
            }catch(e){status.textContent=(e.message||String(e))+'，可重试';}
            finally{loading=false;button.disabled=false;}
        };
        meta.append(button);a.append(meta,status,wrap);
    }else a.append(meta);
    return a;
}
async function load(reset){
    if(!reset&&(state.busy||!state.result?.hasMore))return;
    const revision=++state.revision,context=args(),page=reset?1:state.page+1;
    const request={...context,page};if(!reset)request.cursor=state.cursor;
    if(reset){state.page=0;state.cursor=null;state.result=null;$('items').replaceChildren();$('raw').textContent='';$('page').textContent='第 1 页';}
    availability();state.busy=true;$('next').disabled=true;$('refresh').disabled=true;$('first').disabled=true;$('download').disabled=true;
    $('status').textContent='正在读取官方数据…';const begin=performance.now();
    try{
        if(!context.book.bookUrl)throw new Error('请填写书籍 ID');
        if(context.scope!=='book'&&!context.chapter.url)throw new Error('请从正文阅读菜单打开本章评论');
        let summary=null;
        if(context.scope==='paragraph'){
            summary=await call('review_summary',context);
            if(revision!==state.revision)return;
            const rows=summary.items||[];const previous=String(context.paraIndex);
            $('para').replaceChildren();
            for(const row of rows){
                let detail;try{detail=JSON.parse(row.paraData);}catch{continue;}
                const option=node('button',(detail.paraIndex===0?'本章段评':'第 '+detail.paraIndex+' 段')+' · '+row.count+' 条');
                option.value=String(detail.paraIndex);$('para').append(option);
                option.onclick=()=>{$('para').value=option.value;return load(true);};
            }
            if(!$('para').children.length){const option=node('button','本章暂无段评');option.value='0';option.disabled=true;$('para').append(option);}
            const picked=[...$('para').children].find(o=>o.value===previous)||$('para').children[0];
            $('para').value=picked.value;$('para-label').textContent=picked.textContent;
            context.paraIndex=Number($('para').value);context.version=String(summary.raw?.newest_item_version||'1');
            Object.assign(request,{paraIndex:context.paraIndex,version:context.version});
        }
        const result=await call('reviews',request);
        if(revision!==state.revision)return;
        if(summary)result.paragraphSummary=summary;
        state.result=result;state.page=page;state.cursor=result.cursor;
        const wrap=$('items');wrap.replaceChildren();for(const item of result.items||[])wrap.append(card(item,context));
        if(!result.items?.length)wrap.append(node('div','暂无评论','empty'));
        $('page').textContent='第 '+state.page+' 页';
        $('status').textContent=(result.total!=null?'官方总数 '+result.total+' 条 · ':'')+'本页显示 '+(result.items?.length||0)+' 条 · '+Math.round(performance.now()-begin)+' ms';
        $('raw').textContent=JSON.stringify(summary?{comments:result.raw,paragraphs:summary.raw}:result.raw,null,2);
    }catch(e){if(revision===state.revision)$('status').textContent=e.message||String(e);}
    finally{if(revision===state.revision){state.busy=false;$('refresh').disabled=false;$('first').disabled=false;$('next').disabled=!state.result?.hasMore;$('download').disabled=!state.result;}}
}
document.querySelectorAll('[data-scope]').forEach(b=>b.onclick=()=>{if(b.disabled)return;state.scope=b.dataset.scope;return load(true);});
$('refresh').onclick=()=>load(true);$('first').onclick=()=>load(true);$('next').onclick=()=>load(false);
$('sort').onchange=()=>load(true);$('para').onchange=()=>load(true);
$('para-label').onclick=()=>{$('para').hidden=!$('para').hidden;};
document.querySelectorAll('[data-sort]').forEach(b=>b.onclick=()=>{$('sort').value=b.dataset.sort;return load(true);});
for(const id of ['book','chapter'])$(id).onchange=()=>{if(!$('chapter').value.trim())state.scope='book';load(true);};
$('download').onclick=()=>{if(!state.result)return;const blob=new Blob([JSON.stringify(state.result,null,2)],{type:'application/json'}),u=URL.createObjectURL(blob),a=node('a');a.href=u;a.download='fanqie-comments-'+state.page+'.json';a.click();setTimeout(()=>URL.revokeObjectURL(u),1000);};
availability();$('download').disabled=true;
if(!$('book').value){$('connection').open=true;$('status').textContent='填写书籍 ID 后点击刷新。';}else load(true);
