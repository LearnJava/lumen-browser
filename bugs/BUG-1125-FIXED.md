# BUG-1125 — `DOMTokenList` (`classList`) не итерируем: нет `Symbol.iterator`/`values`/`keys`/`entries`

**Статус:** FIXED 2026-09-26 (P6)
**Заведён:** 2026-09-24 (P2, разбор совместимости после прогона top100-foreign: 48 сайтов с поломкой отрисовки, видимое окно `--maximized` против Chrome 153, **без блокировщика** (`LUMEN_NO_ADBLOCK=1`); [журнал](../docs/perf/journal.md) §2026-09-24 compat). Передан P6 по решению пользователя.
**Область:** js (`crates/js/src/shim/web_api_shim_mid.js:1336-1398` — `DOMTokenList.prototype`: есть `forEach`, итераторов нет; образец — `NodeList`, `:6067-6088`)

## Симптом

- wordpress: `Uncaught TypeError: e.classList.values is not a function at <anonymous>:64:16436 at Array.map`.
- mozilla: `Uncaught TypeError: e[$s] is not a function` в `airgap.js` (Transcend consent) — он
  разворачивает `DocumentFragment.childNodes`, `.children` и `classList` через `Symbol.iterator`.

У `NodeList`/`HTMLCollection` итератор есть (`_lumen_index_iterator`), у `DOMTokenList` — только
`forEach`. `Array.from(classList)` работает только потому, что Proxy отдаёт индексы.

## Репро

Файлы — `.tmp/compat/` в worktree аудита (`.claude/worktrees/perf-base`), отдаются `python -m http.server` с `127.0.0.1`; сравнение — `.tmp/compat/probe.py both <url>` (видимое окно, без блокировщика).

`g6/site/iter.html`:

```html
<!doctype html><html><head><meta charset=utf-8></head><body><p>iter</p>
<script>
window.__r=()=>{const f=document.createDocumentFragment(), o={};
const c={nodelist_frag:f.childNodes, htmlcoll_frag:f.children, tokenlist:document.createElement('_').classList,
  nodelist_body:document.body.childNodes, htmlcoll_body:document.body.children, qsa:document.querySelectorAll('p')};
for (const k in c){ const v=c[k]; let r={}; try{r.ctor=v&&v.constructor&&v.constructor.name; r.iter=typeof v[Symbol.iterator]; r.next=typeof (v[Symbol.iterator]&&v[Symbol.iterator]().next); r.forEach=typeof v.forEach;}catch(e){r.err=String(e)} o[k]=r;}
try{ const [a,b,cc]=[f.childNodes,f.children,document.createElement('_').classList].map(e=>e&&e[Symbol.iterator]().next); o.airgap_pattern='ok '+[typeof a,typeof b,typeof cc]; }catch(e){o.airgap_pattern='THROW '+e}
return o;};
</script></body></html>
```

**Результат:** Lumen: `tokenlist {ctor:'DOMTokenList', iter:'undefined', next:'undefined', forEach:'function'}`, шаблон airgap — `THROW TypeError: e[Symbol.iterator] is not a function`. Chrome: `iter:'function'`, `ok function,function,function`; `Array.from(classList.values())` = `'a,b,c'`.

## Что сделать

DOM §7.1: `DOMTokenList` объявлен `iterable<DOMString>` → WebIDL §3.7.9 даёт
`entries`/`keys`/`values`/`forEach`/`@@iterator` (`@@iterator === values`). Добавить их на
`DOMTokenList.prototype` по образцу `NodeList`. Критерий: репро даёт результат Chrome.

## Исправление (2026-09-26, P6)

`crates/js/src/shim/web_api_shim_mid.js`, `DOMTokenList.prototype`: добавлены `entries`/`keys`/`values`
и `@@iterator`. По WebIDL §3.7.9 это тот же объект функции, что `values`. Итератор тот же живой
`_lumen_index_iterator`, что у `NodeList`: он читает `length`/`[i]` через indexed-Proxy на каждом шаге,
поэтому токен, добавленный посреди обхода, виден. `forEach` переписан на тот же живой обход:
третьим аргументом колбэку он передаёт сам список, а не массив-снимок, и бросает `TypeError`,
если колбэк не функция (как у `NodeList`). `relList` построен на том же прототипе и получил всё это без
отдельной правки.

Тесты: `crates/js/src/dom/tests/v8_events_cache.rs` — `classlist_is_iterable` (итератор, `values`,
`keys`, `entries`, spread, `@@iterator === values`, `.next` у итератора — это шаблон airgap) и
`classlist_iterator_is_live_and_foreach_passes_list`.
