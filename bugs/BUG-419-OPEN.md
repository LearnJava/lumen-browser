# BUG-419 — Защита canvas от fingerprint'а закрывает ровно один путь создания элемента из четырёх

**Статус:** OPEN
**Компонент:** js (`crates/js/src/webgl_canvas.rs:462-493` — `_addCanvasStubs` + monkeypatch
`document.createElement`), privacy
**Найден:** 2026-07-28 (P2), WPT-VENDOR-html-canvas, проба `--dump-layout`

## Симптом

`CAPABILITIES.md` (строка про Graphics) объявляет «toDataURL blank (anti-fingerprint)»,
и код это подтверждает собственным комментарием:

```js
// crates/js/src/webgl_canvas.rs:479-481
// Blank data URL — prevents canvas pixel-hash fingerprinting.
el.toDataURL = function() { return 'data:,'; };
el.toBlob = function(cb) { if (typeof cb === 'function') cb(null); };
```

Заглушка ставится **только** внутри `_addCanvasStubs(el)`, а он вызывается из одной точки —
патча `document.createElement`:

```js
// crates/js/src/webgl_canvas.rs:484-493
var _origCreate = document.createElement.bind(document);
document.createElement = function(tag) {
  var el = _origCreate(tag);
  if (typeof tag === 'string' && tag.toLowerCase() === 'canvas') { _addCanvasStubs(el); }
  return el;
};
```

Любой другой способ получить `<canvas>` отдаёт элемент с исходным `toDataURL` из
`dom.rs`, который честно кодирует пиксели. Проба (`--dump-layout`, страница с
`<canvas id=c width=60 height=30>`, во все канвасы рисуется один и тот же
`fillRect` цветом `#123456`):

```
parsed.2d=true    parsed.toDataURL=data:image/png;base64, len=118
made.2d=false     made.toDataURL=data:,                   len=6
madeNS.2d=true    madeNS.toDataURL=data:image/png;base64, len=118
cloned.2d=true    cloned.toDataURL=data:image/png;base64, len=118
parsed.getImageData.px=18,52,86,255
```

где `parsed` = `<canvas>` из разметки, `made` = `document.createElement('canvas')`,
`madeNS` = `document.createElementNS('http://www.w3.org/1999/xhtml','canvas')`,
`cloned` = `made.cloneNode(false)` — то есть **клон уже защищённого элемента защиту теряет**.

Обход в одну строку:

```js
document.createElementNS('http://www.w3.org/1999/xhtml', 'canvas').toDataURL()   // реальный PNG
document.createElement('canvas').cloneNode(false).toDataURL()                    // реальный PNG
```

`getImageData` не заглушён ни на одном пути вовсе (`18,52,86,255` — точные пиксели),
поэтому даже полностью исправленный `toDataURL` сам по себе от pixel-hash-снятия
не защищает: канонические скрипты фингерпринта читают и то, и другое.

## Вторая сторона того же патча

Тот же `_addCanvasStubs` подменяет `el.getContext` версией, которая знает только
`webgl`/`webgl2`/`experimental-webgl` и возвращает `null` для всего остального
(`webgl_canvas.rs:462-478`) — это [BUG-348](BUG-348-OPEN.md), и там же уточнён его
реальный охват: сломан ровно путь `createElement`, а не «любой canvas».
Итог для страницы противоположный по знаку, но с одним корнем: **`createElement`-канвас
не умеет рисовать, зато защищён; остальные три пути умеют рисовать и не защищены.**

## Что требует политика приватности проекта

`docs/plan/privacy.md` и `CAPABILITIES.md` описывают меру как свойство браузера, а не
свойство одного JS-пути создания элемента. Мера должна применяться к `<canvas>` как к
типу элемента — то есть жить в фабрике обёрток `dom.rs::_lumen_build_element`
(единая точка для разметки, `createElement`, `createElementNS`, `cloneNode`,
`importNode`, парсера фрагментов), а не в monkeypatch'е одного метода `document`.
Обязательно вместе с `getImageData`/`toBlob`/`convertToBlob` у `OffscreenCanvas` —
иначе мера остаётся косметической.

Отдельным решением (не этого бага) стоит зафиксировать, **включена** ли мера вообще:
глухой `data:,` ломает легальные сценарии (экспорт картинки, `<canvas>`-редакторы), и
CAPABILITIES.md уже помечает `canvas fingerprint noise` как ⬜ — шум вместо обнуления
закрыл бы обе задачи.

## Данные WPT

Тесты `html/canvas/element/canvas-host/*` (`toDataURL`/`toBlob`, 52 id) исполняются на
разметочном канвасе и потому меру не видят вовсе — WPT её не измеряет ни в одну сторону.
Находка получена пробой, а не прогоном.

## Направление починки

1. Перенести `toDataURL`/`toBlob`-заглушку (или будущий шум) из `_addCanvasStubs` в
   единую фабрику элементов `dom.rs`, по тегу `canvas`, чтобы путь создания не влиял.
2. Убрать сам monkeypatch `document.createElement` — он же корень BUG-348.
3. Покрыть `getImageData`/`OffscreenCanvas.convertToBlob` той же политикой.
