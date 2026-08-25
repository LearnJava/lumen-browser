# BUG-885 — фрейм, которым распоряжается скрипт, не грузится никогда: ни вставленный через `createElement`, ни парсерный, которому `src` присвоили из JS (парсерный с готовым `src` — грузится)

**Статус:** OPEN
**Заведён:** 2026-08-23 (WPT-RUN-6, срез 28 — живой замер, варианты `frame-parser`/`frame-late-src`)
**Область:** shell (`crates/shell/src/main.rs:5464` — `load_frame_sub_documents` зовётся из `parse_and_layout`, т.е. один раз на разбор документа; вставка узла из JS этот путь не запускает)
**Владелец:** P1/P3. Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

[BUG-480](BUG-480-OPEN.md) срез 1 (P3, 2026-08-23) научил движок грузить
под-документы `<iframe>` — и это работает, но **только для фреймов, которые
написал парсер**. `document.createElement('iframe')` + `appendChild` с тем же
`src` не даёт ни одного HTTP-запроса, ни `load` на хосте, ни исполнения
скриптов ребёнка — ни через 1,5 с, ни через 9 с. Ошибки нет нигде: элемент
живой, `contentWindow` — объект.

Ровно то же и для **парсерного** фрейма, которому `src` присваивают позже из
скрипта — и с исходным `src="about:blank"`, и вовсе без атрибута: запроса нет,
`load` нет. То есть работает единственный случай — фрейм, которого парсер уже
увидел с готовым URL; всё, чем распоряжается скрипт, мертво.

Все три формы измерены на одной странице, одним сервером, в одном прогоне.

## Прямое измерение

`tests/wpt/verify_window_history_jsurl_gaps.py --variant frame-parser`
(2026-08-23, dev-release, Linux, `main` = `0dc60692d`):

```
child-ran search=?from=parser opener=null parent-is-self=true name=undefined
parser-frame-load-listener
parser-frame-load-attr
parser-frame cw=object doc=none frames=0
frame-parser-final frames=0 dyn-cw=object
[server saw: GET /vwjh-child.html?from=parser]
```

`?from=dynamic` на сервере не появляется вовсе — то есть за URL скриптового
фрейма никто не ходил. Это доказательство именно серверное: браузерный лог
здесь не улика ([BUG-826](BUG-826-FIXED.md)), а страница о незагрузке узнать не
может ([BUG-438](BUG-438-FIXED.md)).

Вариант `frame-late-src` (два парсерных фрейма, `src=about:blank` и без
атрибута, обоим `src` присваивается из обработчика `load`):

```
late-src-assigned a=vwjh-child.html?from=late-blank b=vwjh-child.html?from=late-bare
late-src-final frames=0 len=1
[server saw: nothing]
```

Ни `late-src-load-blank`, ни `late-src-load-bare` не напечатаны.

Побочно измерено в том же варианте и относится к [BUG-480](BUG-480-OPEN.md), а
не сюда: у загруженного парсерного фрейма `window.length` в родителе всё равно
`0`, `iframe.contentDocument` — пусто при `contentWindow`-объекте, а внутри
ребёнка `window.parent === window` (то есть идиома `parent.foo()`, на которой
построена половина фреймовых тестов WPT, попадает в собственный глобал
ребёнка).

## Цена по WPT

Одиннадцать id остатка WPT-RUN-5, где фреймом распоряжается скрипт — вставкой
или присваиванием `src`:

`html/browsers/history/the-history-interface/009.html`,
`…/010.html`,
`html/browsers/history/the-location-interface/location_reload.html`,
`html/browsers/history/joint-session-history/joint-session-history-only-fully-active.html`,
`…/joint-session-history-remove-iframe.html`,
`html/semantics/embedded-content/the-iframe-element/change_parentage.html`,
`xhr/open-url-javascript-window.htm`,
`xhr/open-url-javascript-window-2.htm`,
`html/browsers/history/joint-session-history/joint-session-history-only-fully-active.html`
и `…/joint-session-history-remove-iframe.html` — здесь фрейм парсерный, но
`src` ему присваивают из скрипта (третья форма),
`html/semantics/embedded-content/the-iframe-element/change_parentage.html`
остаётся за [BUG-480](BUG-480-OPEN.md): его фрейм парсерный и с готовым `src`,
то есть грузится, а ломается на `parent` внутри ребёнка.

## Что дальше

HTML LS §4.8.5 «process the iframe attributes» запускается при вставке
элемента в документ и при каждом изменении `src`, а не один раз при разборе
документа.
Точка подключения — тот же `load_frame_sub_documents`, вызванный из пути
вставки узла и из установки атрибута, а не только из `parse_and_layout`.

## Поправка замером: граница — не `createElement`, а момент прохода (P1, 2026-08-25)

Заголовок и §Симптом называют границей «фрейм, которым распоряжается скрипт».
Замер попутно к [BUG-854](BUG-854-FIXED.md) (свой http-сервер, `--dump-layout`
и живое окно, `<frame>` и `<iframe>` на одной странице в одном прогоне)
показал, что граница проходит не там: фрейм, созданный `createElement` и
вставленный **инлайновым скриптом страницы**, грузится исправно — скрипты
страницы выполняются в `parse_and_layout` (`run_scripts_with_dom`) *до* вызова
`load_frame_sub_documents`, поэтому проход видит их вставку как обычный узел
дерева.

Не грузится то, что вставлено **после** прохода: из обработчика `load`, из
таймера, из `requestAnimationFrame`, а также присвоение `src` уже вставленному
фрейму. Это ровно случай, который мерил `--variant frame-parser` (вставка из
обработчика).

Практическое следствие для триажа WPT: тест, который строит фрейм в теле
инлайнового `<script>`, к этому багу **не относится** (`--variant nbc-frame` и
`nbc-iframe` пробы `verify_frame_load_media_gaps.py` проходят целиком), а
`query-encoding/resources/resolve-url.js` относится — он открывается с
`onload = function () {…}`, то есть строит все четыре элемента позже прохода.
