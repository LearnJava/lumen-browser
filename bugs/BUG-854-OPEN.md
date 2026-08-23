# BUG-854 — `<frame>` не существует как элемент: конструктор `HTMLElement`, ресурс из `src` не запрашивается, `load` не приходит, именованного доступа `window[name]` нет

**Статус:** OPEN
**Заведён:** 2026-08-22 (WPT-RUN-6, срез 24 — живой замер, маркер `nbc-element-never-loads`)
**Область:** `crates/js/src/dom.rs` (таблица интерфейсов элементов — `HTMLFrameElement` не определён; `frameset` при этом даёт `HTMLFrameSetElement`), `crates/shell/src/main.rs:7181` (`apply_iframe_sandbox_gates` — «Phase 0: iframe sub-документы не загружаются»; для `<frame>` нет и этого), `crates/engine/layout/src/box_tree.rs:2262` (`collect_requests_inner` — подресурсы собираются только с `<img>`, см. [BUG-848](BUG-848-OPEN.md))
**Владелец:** P1/P3 (`lumen-js` + шелл). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

```js
var el = document.createElement('frame');
el.setAttribute('src', 'child.html');
el.name = 'f';
el.onload = () => console.log('load');   // не зовётся
document.body.appendChild(el);
el.constructor.name;                     // "HTMLElement" (ожидается HTMLFrameElement)
window['f'];                             // undefined
```

Запрос `child.html` не уходит вообще — это видно не по странице и не по логу
браузера (лог о подресурсах врать умеет, [BUG-826](BUG-826-OPEN.md)), а по
серверу пробы, который просто не получает такого пути.

## Прямое измерение

`tests/wpt/verify_frame_load_media_gaps.py --variant nbc-frame` (2026-08-22,
dev-release, Linux, коммит `c583a90b4`, `--seconds 5`, страница жива — 9
тиков):

| ожидалось | получено |
|---|---|
| `nbc-frame-load` | тишина |
| сервер видит `vflm-child.html?frame=1` | `server saw: nothing` |
| `window['nbc_frame']` — объект | `named=undefined` |
| `HTMLFrameElement` | `ctor=HTMLElement` (при `frameset-ctor=HTMLFrameSetElement`) |

Соседние варианты того же замера отделяют этот дефект от уже заведённых:
`<object data>`/`<embed src>` ведут себя так же и принадлежат
[BUG-798](BUG-798-OPEN.md); `<iframe src>` тоже не запрашивает документ, но у
него есть `contentWindow`/`contentDocument` и свой баг —
[BUG-480](BUG-480-OPEN.md); `window.frameElement` отсутствует по
[BUG-588](BUG-588-OPEN.md). Разметочный `<frame>` внутри `<frameset>` не
измерялся отдельно: в замере страница — обычный `<body>`, как и в самих
тестах, которые создают элемент скриптом.

## Масштаб

Маркер `nbc-element-never-loads` в `tests/wpt/timeout_audit.py` — **7 id**
остатка снимка WPT-RUN-5 (общий с BUG-798, разделение по подтестам):

* 5 × `html/infrastructure/urls/resolving-urls/query-encoding/*.html?include=nested-browsing`
  — по 4 подтеста «load nested browsing context <frame|iframe|object|embed …>»
  в каждом, все висят;
* `html/browsers/windows/nested-browsing-contexts/name-attribute.window.html`
  — 24 подтеста, 18 из них про `<frame>`/`<object>`/`<embed>`;
* `html/semantics/embedded-content/the-object-element/object-handler.html`
  — BUG-798.

Ни один из этих id раньше не атрибутировался: `resolve-url.js` создаёт все
четыре элемента одним циклом по `createElement(tag)`, поэтому маркера по
исходнику для них не существует — их назвал только отчёт самого хёрнесса
(стадия `SUBTEST_MARKERS`, срез 24).

## Направление починки (не предписание)

Малая часть — интерфейс: завести `HTMLFrameElement` и рефлексию `src`/`name`.
Основная часть общая с BUG-480/BUG-798 и в одиночку не решается: пока нет
вложенного browsing context, «загрузить `<frame src>`» некуда. Разумный
порядок — сначала общий механизм sub-документа (BUG-480), затем подключить к
нему `<frame>`, `<object data>` и `<embed src>`.

## Как проверить фикс

1. `tests/wpt/.venv/bin/python tests/wpt/verify_frame_load_media_gaps.py
   --variant nbc-frame` — ожидаются `nbc-frame-load`, запрос ребёнка на
   сервере пробы и `named=object`.
2. WPT: `run_report.py --all --root html/infrastructure/urls/resolving-urls/query-encoding`.
