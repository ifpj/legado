'use strict';
const $=id=>document.getElementById(id),query=new URLSearchParams(location.search);
const state={scope:'book',page:1,cursor:null,busy:false,result:null,device:null};
$('book').value=query.get('bookId')||'';$('chapter').value=query.get('chapterId')||'';
if(query.get('title'))$('title').textContent=query.get('title')+' · 书评与讨论';
function node(tag,text,cls){const n=document.createElement(tag);if(text!==undefined)n.textContent=text;if(cls)n.className=cls;return n;}
function image(src,cls){try{const u=new URL(src);if(!['http:','https:'].includes(u.protocol))return null;const img=node('img',undefined,cls);img.src=u.href;img.loading='lazy';img.alt=cls==='avatar'?'用户头像':'评论图片';img.referrerPolicy='no-referrer';return img;}catch{return null;}}
async function call(operation,args){
    // The native bridge evaluates only our own fixed call and JSON-encoded arguments.
    // Credentials remain inside the source and never appear in the URL or page.
    if(typeof window.run==='function'){
        const value=await window.run('eval(String(source.mainJs));relayCall('+JSON.stringify(operation)+','+JSON.stringify(args)+');');
        return typeof value==='string'?JSON.parse(value):value;
    }
    const token=$('token').value||sessionStorage.getItem('relay.token')||'';
    const r=await fetch('/v1/call',{method:'POST',cache:'no-store',headers:{'Content-Type':'application/json','X-Relay-Client':'Community',...(token?{Authorization:'Bearer '+token}:{})},body:JSON.stringify({operation,args,device:state.device||{deviceType:'Android',deviceBrand:'Generic'},osVersion:'13'})});
    const next=r.headers.get('x-fanqie-state');if(next)state.device=JSON.parse(new TextDecoder().decode(Uint8Array.from(atob(next),c=>c.charCodeAt(0))));
    const data=await r.json();if(!r.ok)throw new Error(data.error||'HTTP '+r.status);return data;
}
function args(){return {book:{bookUrl:$('book').value.trim()},chapter:{url:$('chapter').value.trim()},scope:state.scope,paraIndex:Number($('para').value)||0,sort:Number($('sort').value),page:state.page,cursor:state.cursor};}
function card(item,reply=false){
    const c=item.content||{},article=node(reply?'div':'article',undefined,reply?'reply':''),identity=node('div',undefined,'identity');
    const avatar=image(item.avatar,'avatar');if(avatar)identity.append(avatar);identity.append(node('span',item.name||'读者'));
    for(const label of item.badge||[])identity.append(node('span',label,'badge'));article.append(identity);
    const text=(c.replyToName?'回复 '+c.replyToName+'：':'')+(c.text||'');article.append(node('div',text,'body'));
    for(const src of item.images||[c.img].filter(Boolean)){const img=image(src,'picture');if(img)article.append(img);}
    const meta=node('div',undefined,'meta');meta.append(node('span',c.time||''),node('span','赞 '+(c.likeCount||0)),node('span','回复 '+(c.replyCount||0)));
    if(!reply&&(c.replyCount||0)>0){
        const button=node('button','查看回复'),wrap=node('div');let page=1,cursor=null,loading=false;
        button.onclick=async()=>{if(loading)return;loading=true;button.disabled=true;try{const data=await call('review_replies',{...args(),reviewId:item.id,page,cursor});for(const r of data.items||[])wrap.append(card(r,true));cursor=data.cursor;page++;button.textContent=data.hasMore?'更多回复':'回复已加载';button.hidden=!data.hasMore;}catch(e){$('status').textContent=e.message;}finally{loading=false;button.disabled=false;}};
        meta.append(button);article.append(meta,wrap);
    }else article.append(meta);
    return article;
}
async function load(reset){
    if(state.busy)return;if(reset){state.page=1;state.cursor=null;}
    state.busy=true;$('next').disabled=true;$('refresh').disabled=true;$('status').textContent='正在读取官方数据…';const begin=performance.now();
    try{const result=await call('reviews',args());state.result=result;const wrap=$('items');wrap.replaceChildren();for(const item of result.items||[])wrap.append(card(item));if(!result.items?.length)wrap.append(node('div','暂无评论','empty'));
        state.cursor=result.cursor;$('next').disabled=!result.hasMore;$('page').textContent='第 '+state.page+' 页';$('status').textContent=(result.total!=null?'共 '+result.total+' 条 · ':'')+(result.items?.length||0)+' 条 · '+Math.round(performance.now()-begin)+' ms';$('raw').textContent=JSON.stringify(result.raw,null,2);
    }catch(e){$('status').textContent=e.message||String(e);}finally{state.busy=false;$('refresh').disabled=false;}
}
document.querySelectorAll('[data-scope]').forEach(b=>b.onclick=()=>{if(state.busy)return;state.scope=b.dataset.scope;document.querySelectorAll('[data-scope]').forEach(x=>x.classList.toggle('active',x===b));$('para').hidden=state.scope!=='paragraph';$('sort').hidden=state.scope==='chapter';load(true);});
$('refresh').onclick=()=>load(true);$('first').onclick=()=>load(true);$('next').onclick=()=>{state.page++;load(false);};$('sort').onchange=()=>load(true);$('para').onchange=()=>load(true);
$('download').onclick=()=>{if(!state.result)return;const u=URL.createObjectURL(new Blob([JSON.stringify(state.result,null,2)],{type:'application/json'})),a=node('a');a.href=u;a.download='fanqie-reviews-'+state.page+'.json';a.click();setTimeout(()=>URL.revokeObjectURL(u),1000);};
if(!$('book').value){$('connection').open=true;$('status').textContent='填写书籍 ID 后点击刷新。';}else load(true);
