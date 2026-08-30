# BUG-450 — Члены `HTMLCanvasElement` установлены на каждом элементе DOM; `getContext` нарушает контракт аргумента

**Статус:** FIXED 2026-08-30
**Компонент:** js (`crates/js/src/dom.rs:5954-6060` — фабрика обёрток `_lumen_build_element`
ставит `getContext`/`toDataURL`/`toBlob`/`transferControlToOffscreen`/`width`/`height`
безусловно, а не по тегу)
**Найден:** 2026-07-28 (P2), WPT-VENDOR-html-canvas, проба `--dump-layout`
**Перепроверен:** 2026-07-29 на main после починки BUG-348 — оба симптома воспроизводятся
(номер сменён с BUG-421: 421 занят чужим багом хрома)

## Симптом 1: canvas-члены на любом элементе

Проба на обычном `<div id=d></div>`:

```
canvasMembersOnDiv=getContext,toDataURL,toBlob,transferControlToOffscreen,width,height
div.width=0/number          div.height=0
div.toDataURL=data:image/png;base64, len=118
div.toBlob=null
div.transferControlToOffscreen!THROW:InvalidStateError: not a canvas element
div.setWidth=42/42          # div.width = 42  →  <div width="42">
p.width=0
```

То есть `div.toDataURL()` отдаёт валидный PNG-data-URL, а присваивание `div.width`
создаёт на `<div>` атрибут `width`, которого в HTML LS у него нет. (Уточнение
2026-07-29: сам PNG — заглушка 1×1 из [BUG-454](BUG-454-OPEN.md), а не пиксели
элемента; утечки содержимого тут нет, есть лишний член интерфейса.) По спеке `width`,
`height`, `getContext`, `toDataURL`, `toBlob`, `transferControlToOffscreen` живут на
`HTMLCanvasElement`, и `'toDataURL' in document.createElement('div')` обязано быть
`false`.

Комментарий в коде считает это безобидным («Only meaningful on `<canvas>`; harmless on
other elements (creates an unused buffer at most)», `dom.rs:5956-5957`) — на деле это
web-видимая разница: скрипты определяют поддержку canvas именно через
`'getContext' in el`, а `element.width` теперь молча пишет атрибут в разметку.

Причина структурная: у Lumen нет per-tag прототипов (см. [BUG-449](BUG-449-FIXED.md) —
интерфейсов Canvas 2D нет вовсе, [BUG-367](BUG-367-FIXED.md) — все члены `Element`
собственные свойства экземпляра), поэтому «члены HTMLCanvasElement» физически негде
разместить, кроме общей фабрики.

## Симптом 2: контракт аргумента `getContext`

```
getContext.case = c.getContext('2D') !== null  →  true      (спека: null)
getContext.noarg = String(c.getContext())      →  null      (спека: TypeError)
```

Шим приводит аргумент к нижнему регистру (`('' + (contextType || '')).toLowerCase()`,
`dom.rs:5960`), а HTML LS §4.12.5 требует сравнения **по точному** значению: `'2D'`,
`'WebGL'`, `'BitmapRenderer'` обязаны давать `null`. Отсутствующий аргумент по WebIDL
(`required DOMString contextId`) обязан бросать `TypeError`, а не отдавать `null`.
Оба отклонения проверяются в `element/canvas-context/*` и
`element/conformance-requirements/2d.conformance.requirements.missingargs.html`.

Отдельно: `|| ''` в той же строке превращает `getContext(0)`/`getContext(false)` в
`getContext('')` вместо строковой конверсии `'0'`/`'false'` — расхождение того же корня.

## Данные WPT

Срез `html/canvas` (`run_report.py --all --root html/canvas --recursive`; числа — в
строке `WPT-VENDOR-html-canvas` ROADMAP.md). Прицельные файлы:
`element/canvas-context/*` (11 id), `element/canvas-host/*` (52 id),
`element/conformance-requirements/2d.conformance.requirements.missingargs.html`.

## Направление починки

1. Гейт членов canvas по тегу в `_lumen_build_element` (тег уже известен —
   `_lumen_get_tag_name(nid)` вызывается внутри самих методов, проверку нужно поднять
   на уровень установки члена). Радикальнее и правильнее — per-tag прототипы
   (общий фикс с [BUG-449](BUG-449-FIXED.md)/[BUG-367](BUG-367-FIXED.md)).
2. Убрать `toLowerCase()` и `|| ''` из `getContext`, добавить проверку
   `arguments.length === 0 → TypeError`.

## Исправлено 2026-08-30

Взят второй путь из п. 1 — не гейт по тегу, а перенос шести членов на
`HTMLCanvasElement.prototype` за бренд-проверкой `_lumen_canvas_nid` (тот же приём,
что слот состояния Canvas 2D в [BUG-449](BUG-449-FIXED.md)); `width`/`height` розданы
по интерфейсам, которым принадлежат по HTML LS. Подробности — коммит
`5fa1ba302` и строка `BUG-450` в [BUGS.md](../BUGS.md); там же шесть дефектов,
которые нашла проба «до» вместо двух заявленных.

### A/B по категории `html/canvas/element`

Замер обещан закрывающим коммитом и выполнен отдельно (P3, 2026-08-30). База —
прогон на состоянии сразу после BUG-449, то есть на `main` без этой правки;
`fix` — `main` с влитым `7d43ddab3`. Обе стороны `run_report.py --all --root
html/canvas/element --recursive`, 1496 id.

| | harness OK | сабтесты PASS |
|---|---|---|
| base (после BUG-449) | 1186/1496 | 838/3059 |
| fix (с BUG-450) | **1190/1496** | **841/3061** |

Изменились семь файлов. Прямо по предмету правки — четыре:

- `canvas-context/2d.canvas.context.invalid.args.html` FAIL → **PASS** (контракт
  аргумента `getContext`);
- `canvas-host/2d.canvas.host.size.attributes.idl.html` FAIL → **PASS** (отражение
  `width`/`height` как `unsigned long`);
- `fill-and-stroke-styles/2d.pattern.image.incomplete.emptysrc.html` ERROR → **OK**,
  сабтест PASS, и `…/2d.pattern.image.broken.html` ERROR → **OK** — оба падали до
  первого сабтеста, пока `img.width`/`img.height` обслуживал общий canvas-аксессор.

Два файла (`manual/draw-element-image/privacy/svg-images-ignored…`,
`manual/wide-gamut-canvas/canvas-display-p3-drawImage-ImageBitmap-Blob…`) прошли
TIMEOUT → OK при **неизменных** вердиктах сабтестов и времени 11.3 с → 0.13 с,
11.8 с → 1.07 с. Это не заслуга правки, а механизм «одна зависшая страница уносит
остаток шарда»: их сосед `2d.canvas.host.size.large.html` в базовом прогоне убивал
браузер. К плюсу правки эти два не относить.

**Единственный файл, ставший хуже по метке, — тот самый сосед:**
`canvas-host/2d.canvas.host.size.large.html` ERROR (16.2 с, `WebSocket connection
closed`, то есть браузер умер) → TIMEOUT (25 с, зависание). Сабтестов нет ни там,
ни там — регрессии в измеримом смысле нет, сменился режим отказа. Дефект
доэтотный: страница ставит `canvas.width = canvas.height = 2147483647`, и движок
берёт это за чистую монету на обеих сторонах A/B. Заведён отдельно —
[BUG-929](BUG-929-OPEN.md); эта правка его не создала и не чинила.
