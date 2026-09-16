# BUG-889 — SVG-элемент, написанный парсером, — это `HTMLUnknownElement`: интерфейсов `SVG*` у него нет вовсе, а отражение анимируемых атрибутов (`x`/`width`/`viewBox`/`transform`.baseVal) отсутствует на ОБОИХ путях

**Статус:** FIXED 2026-09-16 (P1, ветка `p1-gap-svgdom`)
**Тип:** нереализованная функциональность, не дефект реализованного кода — велась как задача `GAP-SVGDOM` в [ROADMAP.md](../ROADMAP.md). Переклассифицировано 2026-09-02 ре-триажем пула WPT-RUN-5/6: срезы заводили багом всё подряд, потому что правила заведения ([docs/probe-method.md §8](../docs/probe-method.md)) тогда ещё не было. Файл сохраняет номер и путь — на него ссылаются CLAUDE.md, STATUS-файлы и python-тулинг, а запись наблюдений остаётся полезной там, где лежит.
**Заведён:** 2026-08-23 (WPT-RUN-6, срез 29 — живой замер, варианты `svg-length`/`svg-dom`/`svg-createns`)
**Область:** js (`crates/js/src/svg.rs` — типизированные классы навешиваются ТОЛЬКО патчем `document.createElementNS`, через `Object.setPrototypeOf`; поля `x`/`y`/`width`/`viewBox` объявлены телами конструкторов этих классов, а конструктор при подмене прототипа не выполняется)
**Владелец:** P1/P3. Заведён P2 в ходе WPT-задачи.

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

## Исправлено

Симптом (1) — типизированный прототип у парсерного `<svg>` — уже был закрыт
GAP-XMLDOC срезом 4 (BUG-685, до этой задачи): `_lumen_element_prototype_for`
(`crates/js/src/shim/web_api_shim_mid.js`) резолвит SVG-неймспейс через
`svg.rs`'s `_lumen_svg_ctor_for_local` для КАЖДОГО построения обёртки
(`_lumen_build_element`), не только для `createElementNS` — `instanceof
SVGElement`/`getBBox`/`getCTM` у парсерного элемента уже работали на момент
старта этой задачи; проверено регрессионным `parser_built_svg_gets_typed_prototype`.

Симптом (2) — отражение анимируемых атрибутов — был реальным и по обоим путям
одновременно, ровно по причине, названной в заявке: `_lumen_build_element`
строит обёртку через `Object.create(prototype)`, а не `new Ctor()`, поэтому
поля, которые `svg.rs`'s классы задавали в теле конструктора, не выполнялись
НИ НА ОДНОМ реальном элементе — они были достижимы только через синтетический
`new SVGRectElement()`, которым пользуются лишь собственные тесты файла.
2026-09-16 (P1) поля `x`/`y`/`width`/`height`/`cx`/`cy`/`r`/`rx`/`ry`/`x1`/`y1`/
`x2`/`y2`/`dx`/`dy`/`refX`/`refY`/`markerWidth`/`markerHeight`/`startOffset`
(`SVGAnimatedLength`), `viewBox` (`SVGAnimatedRect`), `transform`/
`gradientTransform`/`patternTransform` (`SVGAnimatedTransformList`), `points`/
`animatedPoints` (`SVGPointList`) и `d` (plain string, SVG2 §9.3.9) перенесены
из полей конструктора в живые геттеры на прототипе каждого класса
(`_lumen_def_svg_lengths`/`_lumen_def_svg_viewbox`/`_lumen_def_svg_transform`/
`_lumen_def_svg_points` — общие фабрики в `svg.rs`), читающие и пишущие
контентный атрибут через `_lumen_get_attr`/`_lumen_set_attr` на каждый доступ.
`className` уже отражался общим дескриптором `Element` (SVG2 склеил его
`SVGAnimatedString` с обычным `Element.className`) — правки не требовалось.

Попутно: `getBBox()` у `rect`/`circle`/`ellipse`/`line`/`polyline`/`polygon`
теперь считает реальный прямоугольник по геометрии своих же отражённых
атрибутов (SVG 2 §10.6.2), вместо захардкоженного `0×0`; `ownerSVGElement`/
`viewportElement` — живой обход `parentNode` до ближайшего `SVGSVGElement`,
а не всегда-`null` поле. `<path>`/`<text>`/`<g>` — за `SVGGraphicsElement`'s
generic `getBBox()` (`0×0`): вычислять bbox из `d` или из раскладки строк
текста в этой задаче не входило, оставлено следующим срезом. `SVGLengthList`
(text `x`/`y` как список, не скаляр) и unit-aware разрешение (`lh`/`rem`/…,
`SVGLength-*.html`) тоже вне скоупа — сохранившийся упрощённый скаляр
`SVGAnimatedLength` был таким уже до этой задачи, парсит голое число.

`cargo test -p lumen-js --lib --features v8-backend` 3746/3746 (было 3742,
+4 новых регрессионных теста в `dom/tests/v8_core/mod.rs`); один существующий
тест (`canvas_members_are_absent_from_other_elements`) уточнён — он проверял
отсутствие `width`/`height` на `<svg>` как побочный эффект того самого
дефекта, теперь легитимно принадлежащих `SVGSVGElement`, проверка перенесена
на `<g>`, у которого их по спеке нет. `cargo clippy -p lumen-js --all-targets
--features v8-backend -- -D warnings` чист.
