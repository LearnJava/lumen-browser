/* chrome.js — рабочие переключатели пространств и профилей:
   клик по кнопке «пространства»/«профили» → дропдаун на месте (не окно).
   Выбор переключает состояние, «+» открывает inline-форму создания. */
(function(){
const WS=[
 {n:'Default',c:'#9aa3ad',tabs:4},{n:'Работа',c:'#7c3aed',tabs:5,iso:1},
 {n:'Дом',c:'#0b6fe0',tabs:3,iso:1},{n:'Личное',c:'#d6453d',tabs:2,iso:1}];
const PF=[
 {n:'Личный',l:'Л',c:'#0066FF',tabs:12},{n:'Рабочий',l:'Р',c:'#8B5CF6',tabs:8},
 {n:'Анонимный',l:'А',c:'#FF3B30',tabs:0,nodisk:1},{n:'Гость',l:'Г',c:'#8E8E93',tabs:0,nodisk:1}];
let activeWs=WS[1],activePf=PF[0];
const toast=document.createElement('div');
toast.style.cssText='position:fixed;bottom:22px;left:50%;transform:translateX(-50%) translateY(20px);background:#1c1f26;color:#e8eaed;font-size:13px;padding:10px 18px;border-radius:11px;box-shadow:0 10px 34px rgba(0,0,0,.3);opacity:0;transition:all .25s;z-index:400;pointer-events:none';
document.body.appendChild(toast);let tt;
function say(t){toast.textContent=t;toast.style.opacity=1;toast.style.transform='translateX(-50%) translateY(0)';clearTimeout(tt);tt=setTimeout(()=>{toast.style.opacity=0;toast.style.transform='translateX(-50%) translateY(20px)'},2200)}

function dropdown(anchor,html){
  closeDD();
  const r=anchor.getBoundingClientRect();
  const dd=document.createElement('div');dd.className='lumen-dd';
  dd.style.cssText='position:fixed;z-index:350;background:#fff;border:1px solid #dde2e8;border-radius:14px;box-shadow:0 18px 50px rgba(0,0,0,.22);min-width:270px;max-width:340px;padding:8px';
  dd.innerHTML=html;
  dd.style.left=Math.min(Math.max(8,r.left),innerWidth-360)+'px';
  const hh=dd.offsetHeight||280;
  dd.style.top=(r.bottom+8+hh>innerHeight?Math.max(8,r.top-hh-8):r.bottom+8)+'px';
  document.body.appendChild(dd);
  document.addEventListener('mousedown',function h(e){if(!dd.contains(e.target)&&e.target!==anchor){dd.remove();document.removeEventListener('mousedown',h)}});
  addEventListener('keydown',function k(e){if(e.key==='Escape'){dd.remove();removeEventListener('keydown',k)}});
  return dd;
}
function closeDD(){document.querySelectorAll('.lumen-dd').forEach(d=>d.remove())}

function wsMenu(anchor){
  const dd=dropdown(anchor,`<div style="padding:8px 12px 6px;font-size:11px;text-transform:uppercase;letter-spacing:.07em;color:#7d8590;font-weight:700">Пространства · Ctrl+Shift+W</div>
   <div id="ddWs"></div>
   <div style="border-top:1px solid #f0f2f4;margin-top:6px;padding-top:6px" id="ddNew"></div>`);
  function list(){
    dd.querySelector('#ddWs').innerHTML=WS.map((w,i)=>`
     <div class="dd-it ${w===activeWs?'on':''}" data-i="${i}" data-tip="<b>${w.n}</b> · ${w.tabs} вкладок${w.iso?' · изоляция cookie':''}<br>клик — переключить пространство">
      <span style="width:11px;height:11px;border-radius:3px;background:${w.c};flex:none"></span>
      <span style="flex:1;font-weight:${w===activeWs?600:400}">${w.n}</span>
      <span style="font-size:11px;color:#7d8590">${w.tabs}</span>
      <span class="dd-x" data-tip="Удалить пространство" style="color:#b6bcc4;cursor:pointer;padding:0 4px">✕</span>
     </div>`).join('');
    dd.querySelectorAll('.dd-it').forEach(el=>{
      el.style.cssText='display:flex;align-items:center;gap:10px;padding:8px 10px;border-radius:9px;cursor:pointer;font-size:13.5px';
      el.onmouseenter=()=>el.style.background='#eef1f4';el.onmouseleave=()=>el.style.background='';
      el.onclick=e=>{if(e.target.classList.contains('dd-x')){WS.splice(+el.dataset.i,1);say('Пространство удалено');list();return}
        activeWs=WS[+el.dataset.i];markChips();say('Пространство: '+activeWs.n+' · '+activeWs.tabs+' вкладок');list();};
    });
    dd.querySelector('#ddNew').innerHTML=`<div class="dd-add" data-tip="Новое пространство: имя, цвет, изоляция cookie" style="padding:8px 10px;border-radius:9px;cursor:pointer;font-size:13px;color:#0b6fe0;font-weight:600">＋ Новое пространство</div>`;
    dd.querySelector('.dd-add').onclick=()=>{
      dd.querySelector('#ddNew').innerHTML=`<div style="display:flex;gap:7px;align-items:center;padding:6px 4px">
       <input id="nwName" placeholder="Имя…" style="flex:1;border:1px solid #dde2e8;border-radius:8px;padding:7px 10px;font:inherit;font-size:13px;outline:none">
       <button id="nwGo" style="border:none;background:#0b6fe0;color:#fff;border-radius:8px;padding:8px 13px;font-size:12.5px;cursor:pointer">Создать</button></div>
       <div style="display:flex;gap:6px;padding:4px 4px 6px" id="nwCols">${['#0b6fe0','#7c3aed','#1f9d55','#d8a400','#d6453d'].map(c=>`<span class="nw-c" data-c="${c}" style="width:20px;height:20px;border-radius:50%;background:${c};cursor:pointer;border:2px solid transparent"></span>`).join('')}</div>`;
      let col='#0b6fe0';
      dd.querySelectorAll('.nw-c').forEach(c=>c.onclick=()=>{col=c.dataset.c;dd.querySelectorAll('.nw-c').forEach(x=>x.style.borderColor='transparent');c.style.borderColor='#1f2328'});
      dd.querySelector('#nwGo').onclick=()=>{const n=dd.querySelector('#nwName').value.trim()||'Пространство '+(WS.length+1);WS.push({n,c:col,tabs:0,iso:1});say('Создано: '+n);list()};
      dd.querySelector('#nwName').focus();
    };
  }
  list();
}
function pfMenu(anchor){
  const dd=dropdown(anchor,`<div style="padding:8px 12px 6px;font-size:11px;text-transform:uppercase;letter-spacing:.07em;color:#7d8590;font-weight:700">Профили</div><div id="ddPf"></div>
   <div style="border-top:1px solid #f0f2f4;margin-top:6px;padding-top:6px" id="ddPfNew"></div>`);
  function list(){
    dd.querySelector('#ddPf').innerHTML=PF.map((p,i)=>`
     <div class="dd-it ${p===activePf?'on':''}" data-i="${i}" data-tip="<b>${p.n}</b> · ${p.tabs} вкладок${p.nodisk?' · ничего на диске':''}">
      <span style="width:26px;height:26px;border-radius:50%;background:${p.c};color:#fff;display:flex;align-items:center;justify-content:center;font-size:12px;font-weight:700;flex:none">${p.l}</span>
      <span style="flex:1"><b style="font-weight:${p===activePf?700:600};font-size:13.5px">${p.n}</b>${p.nodisk?' <span style="font-size:10.5px;color:#7d8590">(anon)</span>':''}</span>
      <span style="font-size:11px;color:#7d8590">${p.tabs?p.tabs+' вкладок':'—'}</span>
     </div>`).join('');
    dd.querySelectorAll('#ddPf .dd-it').forEach(el=>{
      el.style.cssText='display:flex;align-items:center;gap:10px;padding:8px 10px;border-radius:9px;cursor:pointer';
      el.onmouseenter=()=>el.style.background='#eef1f4';el.onmouseleave=()=>el.style.background='';
      el.onclick=()=>{activePf=PF[+el.dataset.i];document.body.dataset.profile=activePf.n.toLowerCase();
        document.documentElement.style.setProperty('--acc',activePf.c);
        say('Профиль: '+activePf.n+(activePf.nodisk?' · без записи на диск':''));list();markPf();};
    });
    dd.querySelector('#ddPfNew').innerHTML=`<div class="dd-add" data-tip="Новый профиль: имя, цвет, щиты/Tor по умолчанию" style="padding:8px 10px;border-radius:9px;cursor:pointer;font-size:13px;color:#0b6fe0;font-weight:600">＋ Новый профиль</div>`;
    dd.querySelector('.dd-add').onclick=()=>{
      dd.querySelector('#ddPfNew').innerHTML=`<div style="display:flex;gap:7px;padding:6px 4px"><input id="npName" placeholder="Имя профиля…" style="flex:1;border:1px solid #dde2e8;border-radius:8px;padding:7px 10px;font:inherit;font-size:13px;outline:none"><button id="npGo" style="border:none;background:#0b6fe0;color:#fff;border-radius:8px;padding:8px 13px;font-size:12.5px;cursor:pointer">Создать</button></div>`;
      dd.querySelector('#npGo').onclick=()=>{const n=dd.querySelector('#npName').value.trim()||'Профиль '+(PF.length+1);PF.push({n,l:n[0].toUpperCase(),c:'#0b6fe0',tabs:0});say('Создан профиль: '+n);list()};
      dd.querySelector('#npName').focus();
    };
  }
  list();
}
function markChips(){
  document.querySelectorAll('.ws,.wsi').forEach(el=>{
    el.classList.toggle('on',el.textContent.toLowerCase().includes(activeWs.n.toLowerCase().slice(0,4)));
  });
}
function markPf(){
  document.querySelectorAll('.p[data-tip],.profiles .p').forEach(el=>{});
}
/* --- универсальные действия: иконка делает то, для чего придумана --- */
function ddItem(html,fn){const d=document.createElement('div');d.innerHTML=html;const el=d.firstElementChild;
 el.style.cssText='display:flex;align-items:center;gap:10px;padding:8px 10px;border-radius:9px;cursor:pointer;font-size:13.5px';
 el.onmouseenter=()=>el.style.background='#eef1f4';el.onmouseleave=()=>el.style.background='';el.onclick=fn;return el}
function shieldDD(anchor){
 const dd=dropdown(anchor,`<div style="padding:8px 12px 6px;font-size:11px;text-transform:uppercase;letter-spacing:.07em;color:#7d8590;font-weight:700">Щиты · эта страница</div><div id="ddSh"></div>`);
 let on=true;
 function list(){
  const w=dd.querySelector('#ddSh');w.innerHTML='';
  const sw=(label,sub,val,fn)=>{const r=document.createElement('div');r.style.cssText='display:flex;align-items:center;gap:10px;padding:8px 10px;border-radius:9px';
   r.innerHTML=`<div style="flex:1"><b style="font-size:13px">${label}</b><div style="font-size:11px;color:#7d8590">${sub}</div></div>`;
   const b=document.createElement('button');b.textContent=val;b.style.cssText='border:1px solid #dde2e8;background:#fff;border-radius:14px;padding:4px 12px;font-size:11.5px;cursor:pointer';
   b.onclick=()=>{fn();list()};r.appendChild(b);w.appendChild(r)};
  sw('Щиты','реклама · трекеры · fingerprint',on?'Вкл':'Выкл',()=>{on=!on;say('Щиты: '+(on?'включены':'выключены'))});
  sw('Заблокировано','на этой странице',on?'7 запросов':'0',()=>{});
  sw('Разрешения','камера · микрофон · уведомления','по сайту',()=>{say('Per-site разрешения — см. privacy.html')});
  sw('Tor','SOCKS5 · отдельный профиль',document.body.dataset.tor?'Вкл':'Выкл',()=>{document.body.dataset.tor=document.body.dataset.tor?'':'1';say('Tor: '+(document.body.dataset.tor?'включён':'выключен'))});
 }
 list();
}
function noteDD(anchor){
 const dd=dropdown(anchor,`<div style="padding:10px 12px 4px;font-size:13px;font-weight:700">Заметка к странице</div>
  <div style="padding:0 12px"><textarea id="ndTx" rows="3" placeholder="Текст заметки…" style="width:100%;border:1px solid #dde2e8;border-radius:9px;padding:9px 11px;font:inherit;font-size:13px;resize:vertical;outline:none;box-sizing:border-box"></textarea></div>
  <div style="padding:8px 12px 10px;display:flex;gap:8px;justify-content:flex-end">
   <button id="ndCancel" style="border:1px solid #dde2e8;background:#fff;border-radius:8px;padding:7px 13px;font-size:12.5px;cursor:pointer">Отмена</button>
   <button id="ndSave" style="border:none;background:#0b6fe0;color:#fff;border-radius:8px;padding:7px 15px;font-size:12.5px;cursor:pointer;font-weight:600">Сохранить</button></div>`);
 dd.querySelector('#ndCancel').onclick=()=>dd.remove();
 dd.querySelector('#ndSave').onclick=()=>{say('Заметка сохранена · @notes');dd.remove()};
 dd.querySelector('#ndTx').focus();
}
function menuDD(anchor){
 const dd=dropdown(anchor,`<div style="padding:6px"></div>`);
 const w=dd.firstElementChild;
 [['＋ Новая вкладка','Ctrl+T',()=>openPage('app.html?s=home')],
  ['🕘 История','Ctrl+H',()=>openPage('app.html?s=history')],
  ['📌 Закладки','Ctrl+D',()=>openPage('app.html?s=bookmarks')],
  ['⧗ Читать позже','',()=>openPage('app.html?s=readlater')],
  ['✎ Заметки','',()=>openPage('app.html?s=notes')],
  ['⚙ Настройки','',()=>openPage('app.html?s=settings')],
  ['🛡 Приватность','',()=>openPage('app.html?s=privacy')],
  ['— Выход','Ctrl+Q',()=>say('Демо: выход не реализован')]
 ].forEach(m=>w.appendChild(ddItem(`<span style="flex:1">${m[0]}</span>${m[1]?`<span style="font-size:11px;color:#7d8590">${m[1]}</span>`:''}`,()=>{dd.remove();m[2]()})));
}
function iconish(t){return !/[a-zа-я]{3,}/i.test(t.textContent)} /* иконка без словесной подписи */
function act(t,oc){
 const hay=((t.dataset.tip||'')+' '+oc).toLowerCase();
 if(!iconish(t))return false;
 const row=()=>t.closest('.tab,.mrow,.card-item,.mi,.wsi,.tile,.card');
 if(/закрыть|close|✕/.test(hay)){const r=row();if(r){r.style.transition='all .25s';r.style.opacity=0;r.style.transform='translateX(24px)';setTimeout(()=>r.remove(),240);say('Закрыто · Ctrl+Shift+T — вернуть')}else say('Закрыть · Ctrl+W');return true}
 if(/прочитан|mark/.test(hay)){const r=row();if(r)r.classList.remove('unread');say('Отмечено прочитанным');return true}
 if(/отлож|snooze|⏰/.test(hay)){say('Отложено · вернётся через 2 ч');return true}
 if(/скачать|загрузк|⬇|download/.test(hay)){say('⬇ Файл добавлен в загрузки · Ctrl+J');return true}
 if(/закладк|📌|☆|🔖/.test(hay)){const on=t.classList.toggle('saved');t.style.color=on?'#d8a400':'';t.style.fill=on?'#d8a400':'';say(on?'★ В закладках':'Убрано из закладок');return true}
 if(/читать позже|read-later|⧗/.test(hay)){say('Добавлено в «Читать позже» · @read-later');return true}
 if(/заметк|✎/.test(hay)){noteDD(t);return true}
 if(/найти|find|ctrl\+f/.test(hay)){const f=document.querySelector('.find input,input[placeholder*=найти i]');if(f){f.focus();f.select()}else say('Ctrl+F — поиск по странице');return true}
 if(/спать|💤|усн|hibernate/.test(hay)){const r=row();if(r){r.style.opacity=.45;r.style.filter='grayscale(.6)';if(!r.textContent.includes('💤'))r.insertAdjacentHTML('beforeend',' 💤')}say('Вкладка уснула · T3 — DOM+JS заморожены');return true}
 if(/pip|картинк|вынести/.test(hay)){say('PiP: видео поверх окон · ещё раз — вернуть');return true}
 if(/пауза|play|воспроизв|медиа/.test(hay)){const p=t.dataset.paused;t.dataset.paused=p?'':'1';t.textContent=p?'▶':'⏸';say(p?'▶ Медиа играет':'⏸ Пауза');return true}
 if(/читат|reader|режим чтения/.test(hay)){document.body.classList.toggle('reader-mode');say(document.body.classList.contains('reader-mode')?'Режим чтения: вкл':'Режим чтения: выкл');return true}
 if(/tor|лук|onion/.test(hay)){document.body.dataset.tor=document.body.dataset.tor?'':'1';say('Tor: '+(document.body.dataset.tor?'включён · SOCKS5':'выключен'));return true}
 if(/gpc|global privacy/.test(hay)){say('Sec-GPC: 1 — уже отправляется на каждом запросе');return true}
 if(/меню|⋮|три точки|ещё/.test(hay)){menuDD(t);return true}
 if(/назад|alt\+←|back/.test(hay)){say('← Назад · Alt+←');return true}
 if(/вперёд|alt\+→/.test(hay)){say('→ Вперёд · Alt+→');return true}
 if(/обновить|reload|f5|⟳/.test(hay)){t.style.opacity=.35;setTimeout(()=>t.style.opacity='',500);say('⟳ Обновлено · кэш сайта помнит');return true}
 if(/щит|shield|🛡|блокиров/.test(hay)){shieldDD(t);return true}
 return false;
}
document.addEventListener('click',e=>{
  const t=e.target.closest('[onclick]');if(!t)return;
  const oc=t.getAttribute('onclick')||'';
  if(/app\.html\?s=workspaces/.test(oc)){e.preventDefault();e.stopPropagation();wsMenu(t)}
  else if(/app\.html\?s=profiles/.test(oc)){e.preventDefault();e.stopPropagation();pfMenu(t)}
  else if(/app\.html/.test(oc)&&iconish(t)&&act(t,oc)){e.preventDefault();e.stopPropagation()}
},true);
})();
