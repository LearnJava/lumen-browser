# BUG-798 — `<embed>`/`<object>` не грузят содержимое вовсе: нет резолва ресурса, нет `load`/`error`, элементы — просто прототип с рефлекторными атрибутами

**Статус:** OPEN (ДОРАБОТКА → [OBJECT-1](../ROADMAP.md)) — загрузка и события сделаны GAP-LOADEV срезом 4, не хватает отрисовки содержимого
**Тип:** нереализованная функциональность, не дефект реализованного кода — ведётся как задача `GAP-LOADEV` в [ROADMAP.md](../ROADMAP.md), P3 как баг не берёт. Переклассифицировано 2026-09-02 ре-триажем пула WPT-RUN-5/6: срезы заводили багом всё подряд, потому что правила заведения ([docs/probe-method.md §8](../docs/probe-method.md)) тогда ещё не было. Файл сохраняет номер и путь — на него ссылаются CLAUDE.md, STATUS-файлы и python-тулинг, а запись наблюдений остаётся полезной там, где лежит.
**Заведён:** 2026-08-21 (WPT-RUN-6, срез 6 — `html/semantics/embedded-content/the-embed-element`, `the-object-element`)
**Область:** `crates/js/src/dom.rs:13837-13851` (`HTMLObjectElement`/`HTMLEmbedElement` — только `_lumen_install_reflection`, никакой загрузки), `crates/shell/src/main.rs` (нет обработки `<embed>`/`<object>` как источников ресурса)
**Владелец:** P1/P3 (движок). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

`tests/wpt/run_report.py --all --root
html/semantics/embedded-content/the-embed-element --recursive`: 5/21
TIMEOUT. `--root .../the-object-element`: 6/17 TIMEOUT. Все шесть/пять —
тесты, ждущие `embed.onload`/`object.onload` (или сравнимый сигнал вроде
`object.data` после смены), например:

```js
// embed-change-src.html
const embed = document.createElement('embed');
let loadPromise = new Promise(resolve => embed.onload = resolve);
embed.src = '/media/white.mp4';
document.body.appendChild(embed);
await loadPromise;   // виснет навсегда
```

## Причина (локализована чтением кода)

`HTMLEmbedElement`/`HTMLObjectElement` в `dom.rs` получают только
рефлекторные IDL-атрибуты (`src`/`type`/`data`/`name`/…,
`_lumen_install_reflection`, строки 13837/13844) — то же самое лечение,
что любой обычный элемент. Никакого механизма, который бы:

- резолвил `src`/`data` в реальный ресурс,
- диспатчил `load`/`error`,
- держал состояние загрузки (аналог `HTMLImageElement.complete`),

не существует. `grep -n "embed\b\|object\b" crates/shell/src/main.rs` не
находит ни одной строки, обрабатывающей эти теги как источники
встраиваемого содержимого — движок их просто не грузит, в отличие от
`<img>` (частичный путь, дефект — [BUG-630](BUG-630-FIXED.md)) и
`<iframe>` (частичный путь, дефект — [BUG-480](BUG-480-OPEN.md)).

Это тот же КЛАСС пробела («embedded content не грузится»), что BUG-630 и
BUG-480, но **не тот же баг** — `<embed>`/`<object>` не имеют вообще
никакого кода загрузки (ни рабочего, ни сломанного), поэтому чинить их
чинением img/iframe не получится: нужна отдельная реализация резолва
ресурса + событий для этих двух тегов.

## Масштаб

11 из 38 файлов (`the-embed-element` + `the-object-element`) — TIMEOUT
одним и тем же механизмом (ожидание `load`/`error`, которых не бывает).
Остальные файлы этих категорий проходят harness OK, потому что не зависят
от реальной загрузки (проверяют только рефлексию атрибутов/DOM-структуру).

## Направление починки (не предписание)

Симметрично `<img>`/`<iframe>`: резолвить `src`(`<embed>`)/`data`(`<object>`)
через уже существующий сетевой путь, диспатчить `load` на успехе,
`error` на неудаче (404/сетевая ошибка/неподдерживаемый MIME — `<object>`
должен при ошибке показывать fallback-содержимое, HTML LS
§4.8.6). Полноценный «встроенный плагин»/PDF-viewer не требуется —
довольно того же уровня, что `<iframe>`: сам факт диспатча `load`/`error`
разблокирует эти 11 файлов, даже если визуальный рендер содержимого
остаётся заглушкой.

## Как проверить фикс

1. `embed.onload`/`object.onload` срабатывает после `appendChild` с валидным `src`/`data`.
2. `embed.onerror`/`object` fallback-содержимое — после невалидного `src`/`data`.
3. WPT: обе категории — TIMEOUT-счётчик уходит к нулю (11 файлов).

## Срез 24 WPT-RUN-6 (2026-08-22) — доказательство со стороны сервера и 7 id остатка

Замер `tests/wpt/verify_frame_load_media_gaps.py --variant nbc-object
--variant nbc-embed --variant nbc-parser` (dev-release, Linux, коммит
`c583a90b4`, `--seconds 5`, страница жива — 9 тиков) добавляет к записи то,
чего в ней не было: **ресурс не запрашивается вовсе**. Сервер пробы, который
логирует каждый спрошенный путь, не получает ни `?object=1`, ни `?embed=1` —
ни для элементов, созданных скриптом, ни для написанных парсером. Это отделяет
«элемент не диспатчит событие» от «загрузки не было» без доверия к странице
([BUG-438](BUG-438-FIXED.md)) и к логу браузера ([BUG-826](BUG-826-FIXED.md)).

Прочее из того же замера: `object.constructor.name === "HTMLObjectElement"` и
`embed.constructor.name === "HTMLEmbedElement"` (интерфейсы есть),
`object.contentDocument === undefined`, `window['имя']` для `<embed>` —
`undefined`, для `<object>` — объект.

Маркер `nbc-element-never-loads` в `tests/wpt/timeout_audit.py` (стадия
`SUBTEST_MARKERS`, введена этим срезом) — **7 id** остатка снимка WPT-RUN-5,
общих с [BUG-854](BUG-854-FIXED.md): `object-handler.html` целиком здесь, а
пять `query-encoding/*?include=nested-browsing` и
`nested-browsing-contexts/name-attribute.window.html` делят подтесты между
`<object>`/`<embed>`/`<frame>` (сюда) и `<iframe>`
([BUG-480](BUG-480-OPEN.md)).

## Срез 4, GAP-LOADEV (2026-09-13, `p1-gap-loadev-bug798`) — закрыт

Строки из «Причина» устарели по нумерации (файл разошёлся на
`crates/js/src/shim/*.js` в SPLIT-JS3, реализация теперь в
`web_api_shim_mid.js`/`web_api_shim_tail_b.js`), но диагноз остался верным:
ни резолва, ни фетча, ни события не было вовсе.

Фикс — JS-only, без единой правки на Rust-стороне: `<embed>`/`<object>`
заведены в уже существующую fetch-based модель, которой раньше пользовались
только `<script src>`/`<link rel=stylesheet|preload|…>`/`<style>`
(`_lumen_resource_pending`/`_lumen_resource_try_prepare`/
`_lumen_resource_is_connected` в `web_api_shim_mid.js`), а не в тяжёлый
декодирующий пайплайн `<img>` (`page_load.rs`/`page_pipeline.rs`) — decode
здесь не нужен, только байты пришли/не пришли. Сам fetch — переиспользованный
как есть `_lumen_link_hint_fetch(nid, url, null)`, уже написанный для
`<link>`-хинтов.

Три независимых пути обновления `src`/`data`, все ведут в один
`_lumen_embed_object_reload(nid, tag)`:

1. `document.createElement('embed'|'object')` + вставка — `'embed'`/`'object'`
   добавлены в список тегов `_lumen_resource_track`/`_lumen_resource_try_prepare`,
   тем же способом, что `'link'`.
2. Элемент от HTML-парсера — новый `_lumen_embed_object_scan()`, вызываемый из
   `_lumen_apply_ready_state('interactive')` (та же причина, что у
   `_lumen_link_hints_scan`: разметка никогда не проходит через хук вставки).
3. `src`/`data`, установленный на уже подключённом элементе
   (`embed-change-src.html` меняет `src` после первого `load` и ждёт второй) —
   IDL-аксессоры `object.data`/`embed.src` заменены с генерической
   `_lumen_install_reflection('url')`-строки на собственные `get`/`set`,
   вызывающие `_lumen_embed_object_attr_changed`; для `setAttribute('src'|'data', …)`
   — та же проверка добавлена в общую обёртку `setAttribute`/`setAttributeNS`
   через `_lumen_embed_object_maybe_attr_changed`.

**Не в этом срезе** (совпадает со «направление починки» изначального файла):
полноценная встроенная browsing context для `<object>`
(`contentDocument`/`contentWindow`) и §4.8.6 fallback-content на детей при
неудаче — тот же уровень, что уже принят для `<iframe>`/`<frame>`: сам факт
диспатча `load`/`error` снимает WPT TIMEOUT, визуальный рендер остаётся
заглушкой.

**Проверка.** `tests/wpt/verify_frame_load_media_gaps.py --variant nbc-object
--variant nbc-embed --variant nbc-parser` (dev-release, Windows): сервер
видит все три пути (`?object=1`, `?embed=1`, `?p-object=1`/`?p-embed=1`),
`nbc-object-load`/`nbc-embed-load`/`nbc-parser-load ×3` печатаются — раньше
ни один запрос не уходил вовсе (срез 24 выше). `cargo test -p lumen-js
--features v8-backend --lib` — 1855/1855, без изменений (шим-правка не
трогала уже покрытые пути).

Гейт: изменения — только `crates/js/src/shim/{web_api_shim_mid,
web_api_shim_tail_b}.js`, без Rust-кода — `scripts/scoped-test.sh` и
workspace clippy прогоняются в `/lumen-task-finish`, не здесь.
