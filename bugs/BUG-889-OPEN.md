# BUG-889 — SVG-элемент, написанный парсером, — это `HTMLUnknownElement`: интерфейсов `SVG*` у него нет вовсе, а отражение анимируемых атрибутов (`x`/`width`/`viewBox`/`transform`.baseVal) отсутствует на ОБОИХ путях

**Статус:** OPEN
**Заведён:** 2026-08-23 (WPT-RUN-6, срез 29 — живой замер, варианты `svg-length`/`svg-dom`/`svg-createns`)
**Область:** js (`crates/js/src/svg.rs:901-918` — типизированные классы навешиваются ТОЛЬКО патчем `document.createElementNS`, через `Object.setPrototypeOf`; поля `x`/`y`/`width`/`viewBox` объявлены телами конструкторов этих классов, `:308-311`, `:402-430`, а конструктор при подмене прототипа не выполняется)
**Владелец:** P1/P3. Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

Два независимых дефекта в одной точке, оба видны на первой строке теста:

1. **Парсерный `<svg>` не является SVG-элементом.** `Object.getPrototypeOf(el).constructor.name` для `<svg>`, написанного в разметке, — `HTMLUnknownElement`; `el instanceof SVGElement` — `false`; `getBBox`/`getCTM`/`getScreenCTM` — `undefined`. Всю разметку WPT-тесты пишут в файле, то есть попадают ровно в этот путь.
2. **Отражения анимируемых атрибутов нет ни на одном пути.** `text.x`, `rect.width`, `svg.viewBox`, `g.transform`, `g.className` — `undefined` и у парсерного элемента, и у созданного через `createElementNS`. Причина у второго случая своя: `svg.rs` объявляет эти поля в ТЕЛЕ конструктора (`this.x = new SVGAnimatedLength(0)`), а патч `createElementNS` только переставляет прототип и конструктор не зовёт.

Из-за (2) чтение `element.x.baseVal` даёт `TypeError: Cannot read properties of undefined (reading 'baseVal')` — 7 id снимка WPT-RUN-5 падают ровно этим текстом, и падают ДО регистрации первого `test()`, поэтому вердикт TIMEOUT, а не FAIL.

Побочно: значения в конструкторах захардкожены (`new SVGAnimatedLength(0)`, у `<svg>` — 300×150), то есть даже при исправлении (2) они не читались бы из атрибутов; `getBBox()` у элемента, созданного через `createElementNS`, возвращает `0x0` для `<rect width=7 height=8>` (заглушка, `svg.rs` признаёт это в заголовке файла), при исправном `getBoundingClientRect()` = `7x8`; `ownerSVGElement` — `null` на обоих путях.

## Прямое измерение

`tests/wpt/verify_cssom_svg_interface_gaps.py --variant svg-createns`
(2026-08-23, dev-release, Linux):

```
created-instanceof = true          created-ctor-name = SVGRectElement
created-getBBox = 0x0              created-getCTM = object
created-x = undefined              created-width-baseVal THREW ... (reading 'value')
created-viewBox THREW ... (reading 'baseVal')
parser-ctor-name = HTMLUnknownElement
parser-instanceof = false          parser-getBBox = undefined
```

`--variant svg-length` (та же страница, что у `SVGLength-*.html`):

```
text-x = undefined
text-x-baseVal THREW Cannot read properties of undefined (reading 'baseVal')
rect-x-baseVal THREW ... (reading 'baseVal')
svg-width-baseVal THREW ... (reading 'value')
globals = SVGLength,SVGAnimatedLength,SVGRect,SVGElement,SVGSVGElement,SVGTextElement,SVGRectElement,SVGTransform,SVGMatrix,SVGPoint
unit-consts = 5/0
createSVGLength THREW root.createSVGLength is not a function
```

То есть классы-значения на месте и константы правильные — не хватает именно
привязки их к узлам.

## Цена по WPT

7 id снимка WPT-RUN-5 с текстом `reading 'baseVal'`, все из `svg/types/scripted/`:
`SVGLength-lh.html`, `-rem.html`, `-ch.html`, `-ic.html`, `-rlh.html`,
`-viewport.html`, `-cap.html` (механизм `svg-dom-not-reflected` в
`tests/wpt/timeout_audit.py`).
Форма шире кластера: любой тест, читающий геометрию SVG через DOM
(`svg/types/`, `svg/coordinate-systems/`, `svg/painting/`), упирается в то же.

## Что дальше

SVG 2 §4.1: `x`/`y`/`width`/`height`/`viewBox`/`transform` — это `SVGAnimated*`-атрибуты
интерфейса элемента, то есть аксессоры на ПРОТОТИПЕ, читающие контентный атрибут,
а не поля экземпляра. Два шага, независимых друг от друга: перенести поля из тел
конструкторов `svg.rs` в геттеры прототипов (тогда путь `createElementNS` начнёт
работать), и навесить типизированный прототип на узлы SVG-неймспейса, которые
построил парсер (точка — та же, что у [BUG-830](BUG-830-OPEN.md): неймспейс узла
известен движку, но в JS-обёртку не доезжает).
