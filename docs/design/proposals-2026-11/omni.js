/* omni.js — превращает все .omni в настоящую адресную строку:
   клик → фокус, набор → подсказки (@-команды, URL, поиск), Enter → новая вкладка */
(function(){
const CMDS={
 '@history':['История','s=history'],'@bookmarks':['Закладки','s=bookmarks'],
 '@read-later':['Читать позже','s=readlater'],'@notes':['Заметки','s=notes'],
 '@tabs':['Вкладки','s=home'],'@downloads':['Загрузки','s=downloads'],
 '@settings':['Настройки','s=settings'],'@privacy':['Приватность','s=privacy'],
 '@profiles':['Профили','s=profiles'],'@workspaces':['Пространства','s=workspaces'],
 '@ai':['Спросить Lumen AI','s=ai']};
const ORDER=['@history','@bookmarks','@read-later','@notes','@tabs','@downloads','@settings','@ai'];
const HIST=['lumen.local/docs','github.com/lumen','developer.mozilla.org','whatwg.org'];

function build(el){
  if(el.dataset.omniReady)return;el.dataset.omniReady='1';
  const ph=el.textContent.trim()||'Адрес, поиск или @-команда…';
  el.removeAttribute('onclick');el.style.position='relative';
  const inp=document.createElement('input');
  inp.type='text';inp.placeholder=ph;inp.setAttribute('data-tip',el.dataset.tip||'Адресная строка: URL, поиск или @-команда · Enter — перейти · Esc — закрыть');
  inp.style.cssText='flex:1;min-width:0;border:none;background:transparent;outline:none;font:inherit;color:inherit;padding:0';
  el.textContent='';el.appendChild(inp);
  const dd=document.createElement('div');
  dd.style.cssText='display:none;position:fixed;z-index:300;background:#fff;border:1px solid #dde2e8;border-radius:12px;box-shadow:0 14px 44px rgba(0,0,0,.18);min-width:320px;max-width:520px;overflow:hidden';
  document.body.appendChild(dd);
  let items=[],sel=0;
  function sug(q){
    q=q.trim().toLowerCase();items=[];
    if(q.startsWith('@')){for(const k of ORDER){const c=CMDS[k];if(k.startsWith(q)||c[0].toLowerCase().startsWith(q.slice(1)))items.push({t:k+' — '+c[0],run:()=>open('app.html?'+c[1])})}}
    else{
      if(/^[\w.-]+\.[a-z]{2,}([/?#]\S*)?$/i.test(q)||/^https?:/i.test(q))items.push({t:'🌐 Перейти: '+q,run:()=>open('app.html?s=go&u='+encodeURIComponent(q))});
      items.push({t:'🔍 Искать «'+q+'» — FTS по истории, вкладкам, закладкам',run:()=>open('app.html?s=go&q='+encodeURIComponent(q))});
      for(const h of HIST)if(h.includes(q))items.push({t:'🕘 '+h,run:()=>open('app.html?s=go&u='+encodeURIComponent(h))});
    }
    render();
  }
  function render(){
    if(!items.length){dd.style.display='none';return}
    sel=Math.min(sel,items.length-1);
    dd.innerHTML=items.map((it,i)=>`<div style="padding:9px 14px;font-size:13px;cursor:pointer;display:flex;gap:9px;align-items:center;${i===sel?'background:#e3eefc':''}" onmouseenter="this.style.background='#e3eefc'" onmouseleave="this.style.background=''">${it.t}</div>`).join('');
    const r=inp.getBoundingClientRect();
    dd.style.left=Math.min(r.left,innerWidth-540)+'px';dd.style.top=(r.bottom+6)+'px';dd.style.display='block';
    dd.querySelectorAll('div').forEach((d,i)=>d.onmousedown=e=>{e.preventDefault();items[i].run();hide()});
  }
  function hide(){dd.style.display='none';items=[];sel=0}
  function open(u){window.open(u,'_blank')}
  inp.addEventListener('input',()=>{sel=0;sug(inp.value)});
  inp.addEventListener('focus',()=>{if(inp.value)sug(inp.value)});
  inp.addEventListener('blur',()=>setTimeout(hide,150));
  inp.addEventListener('keydown',e=>{
    if(e.key==='ArrowDown'){e.preventDefault();sel=Math.min(sel+1,items.length-1);render()}
    else if(e.key==='ArrowUp'){e.preventDefault();sel=Math.max(sel-1,0);render()}
    else if(e.key==='Enter'){e.preventDefault();if(items[sel])items[sel].run();else if(inp.value.trim()){const q=inp.value.trim();open(/^[\w.-]+\.[a-z]{2,}/i.test(q)?'app.html?s=go&u='+encodeURIComponent(q):'app.html?s=go&q='+encodeURIComponent(q))}hide()}
    else if(e.key==='Escape'){hide();inp.blur()}
  });
  el.addEventListener('click',e=>{if(e.target!==inp)inp.focus()});
}
function init(){document.querySelectorAll('.omni').forEach(build)}
if(document.readyState==='loading')addEventListener('DOMContentLoaded',init);else init();
})();
