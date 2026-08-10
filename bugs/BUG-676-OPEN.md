# BUG-676 — `ShadowRoot` is a plain object literal: no global constructor, no `cloneNode`, no `Node`-derived methods

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs:1310-1366` — `_lumen_make_shadow_root`)
**Найден:** P2, WPT-VENDOR-shadow-dom, 2026-08-06

## Симптом

Категория `shadow-dom` (`tests/wpt/shadow-dom/`, 393 файла) — вендорена и
прогнана целиком (`run_report.py --all --root shadow-dom --recursive
--processes=4`, ~4 мин, 276 отобранных id): **198/276 harness OK, 221/1780
сабтестов** — категория реально реализована (Shadow DOM работает end-to-end),
что резко контрастирует с типичными 0-и-мало-сигнальными прогонами
🚫-категорий в этом бэклоге.

`_lumen_make_shadow_root` (`dom.rs:1315`) собирает `ShadowRoot` как обычный
`{}`-литерал с навешанными методами/геттерами (`querySelector`,
`appendChild`, `addEventListener` и т.д.) — не через `class ShadowRoot {}` +
`Object.setPrototypeOf`/`new`. Следствия:

* глобального имени `ShadowRoot` не существует вовсе — `'ShadowRoot' in
  window === false`, `typeof window.ShadowRoot === 'undefined'`;
* `sr.constructor.name === 'Object'` — не именованный класс;
* `sr instanceof window.ShadowRoot` бросает `TypeError` (RHS не объект),
  а не возвращает `true`/`false`;
* `sr.cloneNode` отсутствует вовсе (`typeof sr.cloneNode === 'undefined'`) —
  спека требует, чтобы `cloneNode()` на `ShadowRoot` **бросал**
  `NotSupportedError` (DOM LS §4.9), а не просто не существовал;
* `sr.contains`/`sr.getRootNode` отсутствуют тоже (тот же класс дефекта, что
  уже отдельно документирован для узлов вообще — [BUG-574](BUG-574-OPEN.md)/
  [BUG-599](BUG-599-OPEN.md) — но здесь подтверждён именно на `ShadowRoot`).

Подтверждено живьём (`--mcp-live-port`, `attachShadow({mode:'open'})` на
элементе вне WPT-раннера):

```json
{"hasShadowRootInWindow": false, "typeofShadowRoot": "undefined",
 "srConstructorName": "Object",
 "instOf": "ERR:Right-hand side of 'instanceof' is not an object",
 "hasCloneNode": "undefined",
 "cloneNodeResult": "TypeError: sr.cloneNode is not a function",
 "hasSlotAssign": "undefined",
 "hasContains": "undefined", "hasGetRootNode": "undefined"}
```

## Масштаб

* `ShadowRoot is not defined` — 88 сабтест-хитов / 6 файлов
  (`Element-interface-attachShadow-custom-element.html`,
  `HighlightRegistry-highlightsFromPoint.html`, `Node-prototype-cloneNode.html`,
  `ShadowRoot-interface.html`, `attach-shadow-non-html-namespace.html`,
  `wheel-event-related-target.html`) — каждый начинается с
  `instanceof`-sanity-check, падающего до проверки предмета теста.
* `shadowRoot.cloneNode is not a function` — 4 хита / 2 файла
  (`Node-prototype-cloneNode.html`, `wheel-event-related-target.html`).
* Компаньон, отдельный объект, но тот же класс дефекта (плоский литерал
  вместо WebIDL-интерфейса): `HTMLSlotElement.prototype.assign()`
  (imperative slot API, DOM LS §4.2.2.3) отсутствует вовсе —
  `slotA.assign is not a function`, 4 хита / 2 файла
  (`imperative-slot-api-cross-shadow-root.html`, `wheel-event-related-target.html`).
  `assignedNodes`/`assignedElements` (`dom.rs:3445-3459`) работают, `assign`
  рядом не заведён.

Не единственная причина отказов категории — доминирующие независимые
классы, все уже открыты: [BUG-384](BUG-384-OPEN.md) (именованный доступ на
`window` — `container`/`host`/`sandbox`/`test1..6` и т.п., самый крупный
кластер), [BUG-346](BUG-346-OPEN.md) (`..`-сегменты в `Url::resolve()` не
схлопываются — `../../../../html/resources/common.js` → HTTP 404,
`newHTMLDocument is not defined`), [BUG-574](BUG-574-OPEN.md)
(`elementDocument.contains is not a function`, ломает `test_driver.click()`
в 52 хитах), [BUG-415](BUG-415-OPEN.md) (отсоединённый документ
`createHTMLDocument()` без Node-методов — `document.createNodeIterator`/
`importNode` "not a function" на нём же), [BUG-464](BUG-464-OPEN.md)/
[BUG-477](BUG-477-OPEN.md) (`document.elementFromPoint`/`elementsFromPoint`
не реализованы), [BUG-601](BUG-601-OPEN.md) (глобальный `DOMTokenList`
отсутствует), [BUG-471](BUG-471-OPEN.md) (`CSSStyleSheet`/CSSOM не
подключены). Новых номеров под них не заведено — реконфирмации.

## Причина

Тот же класс дефекта, что уже документирован для `Selection`
([BUG-671](BUG-671-OPEN.md)) и `Headers`/`Response`
([BUG-369](BUG-369-FIXED.md)/[BUG-370](BUG-370-FIXED.md)): объект собирается
как ES5-литерал с методами вместо `class` + прототипной цепочки, поэтому
глобального конструктора негде взяться, а методы, которых нет в самом
литерале (`cloneNode`, `contains`, `getRootNode`), просто отсутствуют вместо
наследования от `Node.prototype`.

## Дальше

Fix scope: завести `class ShadowRoot extends DocumentFragment` (или
эквивалент с верным `.prototype`/`Symbol.toStringTag` — см. класс
BUG-369/589) в `crates/js/src/dom.rs`, выставить на `window`, переключить
`_lumen_make_shadow_root` на `new ShadowRoot(...)`; добавить `cloneNode()`,
явно бросающий `NotSupportedError`. Отдельно — `HTMLSlotElement.prototype.assign()`
рядом с существующими `assignedNodes`/`assignedElements` (`dom.rs:3445-3459`).
