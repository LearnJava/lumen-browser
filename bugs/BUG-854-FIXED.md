# BUG-854 — `<frame>` не существует как элемент: конструктор `HTMLElement`, ресурс из `src` не запрашивается, `load` не приходит, именованного доступа `window[name]` нет

**Статус:** FIXED 2026-08-25 (P1)
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
браузера (лог о подресурсах врать умеет, [BUG-826](BUG-826-FIXED.md)), а по
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

## Починено (P1, 2026-08-25)

`<frame>` стал таким же хостом вложенного browsing context, как `<iframe>`, —
обеими половинами сразу, потому что поодиночке ни одна не наблюдаема: без
интерфейса не с чего читать результат загрузки, без загрузки интерфейс нечего
показывать.

* **Интерфейс** (`crates/js/src/dom.rs`): `HTMLFrameElement` заведён в том же
  генераторе, что `HTMLAreaElement`/`HTMLTrackElement`/…, тег `FRAME` добавлен
  в `_lumen_html_tag_prototypes`, рефлексия — все восемь IDL-атрибутов
  HTML LS §16.3.3 (`src`/`name`/`scrolling`/`frameBorder`/`longDesc`/
  `noResize`/`marginHeight`/`marginWidth`).
* **`contentDocument`/`contentWindow`** ведут в тот же реестр под-документов
  ([BUG-480](BUG-480-OPEN.md) срез 2, `frame_bridge.rs`) — биндинг там
  адресуется id узла-хоста и о теге не знает вовсе. Аксессоры поставлены на
  **прототип**, а не на элемент, как у `iframe_element.rs`: `<frame>`, который
  написал парсер, ни через какой хук `createElement` не проходит, а прототип
  покрывает оба происхождения одним определением.
* **Загрузка** (`crates/engine/dom/src/lib.rs`): `collect_iframes` собирает
  оба тега — «вложенный browsing context» у них один алгоритм (§16.3.3
  «process the frame attributes» = §4.8.5), различаются только атрибуты,
  которых у `<frame>` нет. Шелл (`load_frame_sub_documents`) не изменился ни
  строкой: fetch, парс, JS-контекст ребёнка, `load` на хосте и биндинг
  `window[name]` уже были тег-независимы.
* `srcdoc` намеренно читается только у `<iframe>`: на `<frame>` это обычный
  неизвестный атрибут, и брать его как источник — выдумка (юнит-тест
  `collect_iframes_frame_srcdoc_is_not_a_source`).

### Замер

`--variant nbc-frame` (dev-release, Windows, `main` = `ea79b5c2c`) — до/после:

| ожидалось | до | после |
|---|---|---|
| `nbc-frame-load` | тишина | **есть** |
| сервер видит `vflm-child.html?frame=1` | `nothing` | **есть** |
| `window['nbc_frame']` | `undefined` | **`object`** |
| конструктор | `HTMLElement` | **`HTMLFrameElement`** |
| `window.frames.length` | 0 | **1** |

Плюс собственная проба (`--dump-layout` и живое окно, свой http-сервер):
`<frame>` в `<body>` и в `<frameset>`, оба грузятся, скрипты ребёнка
выполняются, `contentDocument`/`contentWindow` — объекты. Контроль `<iframe>`
на той же странице не сдвинулся. `dump_golden.py` — 12/12 совпадений: список
отображения не поехал (содержимое фрейма по-прежнему не рисуется, это отдельный
срез BUG-480).

### Ловушка замера, которая чуть не стала фантомным регрессом

Первый прогон `--variant nbc-frame` с дефолтными `--seconds 6` дал `ticks 0` и
лог, обрывающийся на `GET` ребёнка, — читается как «страница зависла на
загрузке фрейма», хотя всё работало: на этой машине только проба бэкенда
съедает 1.8 с, а первый непустой кадр приходит на ~5.9 с, то есть процесс
убивали ровно на середине. Диагноз развалился, как только тот же файл
запустили с `--seconds 12` (13 тиков, все маркеры) — **дефолтного окна пробы
на этой машине не хватает; сравнивать варианты можно только при одинаковом и
достаточном `--seconds`.**

### Остаток — не про `<frame>`

Фрейм, вставленный **после** единственного прохода загрузки фреймов в
`parse_and_layout`, не грузится — это [BUG-885](BUG-885-OPEN.md), и он ровно
такой же у `<iframe>`. Отсюда практическое следствие для WPT: 5 id
`query-encoding/*?include=nested-browsing` останутся TIMEOUT, потому что
`resolve-url.js` строит все элементы внутри `onload = function () {…}`, то есть
позже прохода.

Попутная поправка к формулировке BUG-885, полученная замером: граница не
`createElement`, как там написано, а момент прохода. Фрейм, созданный
`createElement` и вставленный **инлайновым скриптом страницы**, грузится
исправно (скрипты страницы выполняются в `parse_and_layout` до прохода) — это
проверено на `<frame>` и на `<iframe>` в одном прогоне, и именно поэтому
`--variant nbc-frame`/`nbc-iframe` проходят целиком.

Также остаётся не про этот баг: `<frame>` не рисуется (раскладка содержимого
фрейма — срез BUG-480), `window.frameElement` отсутствует
([BUG-588](BUG-588-OPEN.md)), а `<object data>`/`<embed src>` по-прежнему не
грузятся вовсе ([BUG-798](BUG-798-OPEN.md)) — маркер
`nbc-element-never-loads` остаётся за ними.

Найдено рядом и заведено отдельно: `iframe.src` отдаёт атрибут дословно вместо
разрешённого URL, потому что собственное свойство из `iframe_element.rs`
затеняет корректную строку рефлексии на прототипе
([BUG-920](BUG-920-OPEN.md)); у `<frame>`, у которого такого затенения нет,
`src` сразу абсолютный.
