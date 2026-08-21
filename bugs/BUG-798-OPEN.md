# BUG-798 — `<embed>`/`<object>` не грузят содержимое вовсе: нет резолва ресурса, нет `load`/`error`, элементы — просто прототип с рефлекторными атрибутами

**Статус:** OPEN
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
`<img>` (частичный путь, дефект — [BUG-630](bugs/BUG-630-OPEN.md)) и
`<iframe>` (частичный путь, дефект — [BUG-480](bugs/BUG-480-OPEN.md)).

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
