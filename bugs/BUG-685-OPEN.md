# BUG-685 — HTML parser never switches to the SVG namespace inside `<svg>`; declarative SVG markup never gets the SVG DOM prototype chain

**Статус:** OPEN (ДОРАБОТКА → [GAP-XMLDOC](../ROADMAP.md))
**Тип:** нереализованная функциональность, не дефект реализованного кода — ведётся как задача `GAP-XMLDOC` в [ROADMAP.md](../ROADMAP.md), P3 как баг не берёт. Переклассифицировано 2026-09-02 ре-триажем пула WPT-RUN-5/6: срезы заводили багом всё подряд, потому что правила заведения ([docs/probe-method.md §8](../docs/probe-method.md)) тогда ещё не было. Файл сохраняет номер и путь — на него ссылаются CLAUDE.md, STATUS-файлы и python-тулинг, а запись наблюдений остаётся полезной там, где лежит.
**Компонент:** engine (`crates/engine/html-parser/src/tree_builder.rs:1879` — `create_element_with_attrs` calls `self.doc.create_element(QualName::html(name))` unconditionally, no foreign-content branch anywhere in the crate); knock-on in js (`crates/js/src/dom.rs:1673` `_lumen_element_prototype_for` — only patches the prototype away from `Element.prototype` for the XHTML namespace; `crates/js/src/svg.rs:905-921` — the `SVG_TAG_MAP` prototype swap only fires from the `document.createElementNS` override, never for parser-created nodes)
**Найден:** P2, WPT-VENDOR-svg, 2026-08-06

## Симптом

Категория `svg` (`tests/wpt/svg/`, vendored + run whole:
`run_report.py --all --root svg --recursive --processes=4`, 861 ids,
4:12 wall-clock) — **421/472 harness OK, 397/2068 subtests** (an
unusually high harness-OK ratio for this backlog, on par with `fetch`:
the category *is* substantially implemented, the failures are
concentrated and mechanical, not a blanket API-absence wall). Aggregate
error-message counts across the run log:

```
260  TypeError: Cannot read properties of undefined (reading 'baseVal')
180  TypeError: rootSVGElement.pauseAnimations is not a function
135  ReferenceError: svg is not defined            (see "Not investigated" below)
 90  TypeError: document.elementFromPoint is not a function   (BUG-464/477/580, reconfirmation)
 50  TypeError: document.getElementById(...).getBBox is not a function
 41  TypeError: el.getBBox is not a function
 38  TypeError: Cannot read properties of undefined (reading 'valueAsString')
 33  TypeError: image.getBBox is not a function
 27  TypeError: svg.setCurrentTime is not a function
 17  TypeError: Cannot read properties of undefined (reading 'length')
 16  TypeError: svg.pauseAnimations is not a function
 14  TypeError: refPath.getTotalLength is not a function
 13  TypeError: path.getTotalLength is not a function
  …  (getPointAtLength, getStartPositionOfChar, getNumberOfChars,
      createSVGLength, deselectAll, beginElement, isPointInFill/Stroke,
      setPathData — same pattern, smaller counts)
```

Every one of these — **except** the already-tracked `elementFromPoint`
line — is a missing member on an SVG element obtained by walking markup
that was declared directly in the test's HTML (`<svg><rect .../></svg>`,
`document.querySelector('svg')`, `document.getElementById(...)`), never
via `document.createElementNS`.

## Причина

`crates/js/src/svg.rs` implements a real, fairly complete SVG DOM —
`SVGGraphicsElement.getBBox()`, `SVGGeometryElement.getTotalLength()`/
`getPointAtLength()`, `SVGSVGElement.pauseAnimations()`/
`setCurrentTime()`, `SVGAnimatedLength`/`SVGAnimatedString` with real
`baseVal`/`animVal`, `SVGTextContentElement.getStartPositionOfChar()`,
etc. (all present, all unit-tested in that file's own `#[cfg(test)]`
module). It is wired in exactly one place: a monkey-patch of
`document.createElementNS` (`svg.rs:905-921`) that, for the SVG
namespace, `Object.setPrototypeOf`s the freshly-created node onto the
matching `SVG*Element.prototype` from a 45-entry tag→constructor table.

That patch only fires for elements *constructed from script*. Elements
that come from **parsing** `<svg>` markup — the overwhelming majority of
real-world SVG usage and of this WPT category — never go through
`createElementNS` at all; they are built by
`crates/engine/html-parser/src/tree_builder.rs`, whose
`create_element_with_attrs` (line 1879) does:

```rust
let id = self.doc.create_element(QualName::html(name));
```

unconditionally — there is no branch anywhere in the crate (confirmed:
`grep -rn "namespace\|Namespace" tree_builder.rs` has zero hits besides
this one call site) that checks whether the current insertion mode is
inside `<svg>`/`<math>` and should be applying the HTML LS §13.2.6.5
"foreign content" algorithm (SVG-namespace elements, case-sensitive tag
names via `adjustSVGTagNames`, foreign attribute adjustment). Every
element the parser ever creates is unconditionally namespaced
`http://www.w3.org/1999/xhtml`, SVG or not.

Two downstream consequences, both confirmed live (`--mcp-port`, page
`<svg id=root><rect id=r .../></svg>`):

```json
{
  "namespaceURI": "http://www.w3.org/1999/xhtml",   // should be .../2000/svg
  "ctorName": "HTMLElement",                          // should be SVGRectElement
  "protoChain": ["HTMLElement","Element","Node","Object"],
  "tagName": "RECT"                                   // should be case-preserved "rect"
}
```

vs. the same tag built via `document.createElementNS` in the same page,
which is correct:

```json
{ "isRectEl": true, "hasGetBBox": "function" }
```

(1) `_lumen_element_prototype_for` (`dom.rs:1673`) resolves the
prototype for every parser-built node from `_lumen_get_namespace_uri`;
since that always reads back XHTML for SVG markup, every `<rect>`/
`<circle>`/`<svg>`/... in the tree gets plain `HTMLElement.prototype` —
none of `svg.rs`'s classes are ever reached. (2) the tag name is
upper-cased the same way an ordinary HTML tag is (SVG requires
case-sensitive local names — `feGaussianBlur`, `textPath`, etc. — which
also breaks the `SVG_TAG_MAP` lookup in `svg.rs:910` for any *scripted*
`createElementNS` call spelled with the correct SVG casing if a caller
ever cross-checks `tagName`, and is a smaller aspect of the already-open
[BUG-367](BUG-367-FIXED.md), filed against the `createElementNS` path
specifically — this bug's parser-side namespace gap is the larger,
independent defect: BUG-367's element *does* get the right prototype,
just the wrong tag-name case; this bug's element gets neither).

Layout/paint are unaffected — `CAPABILITIES.md`'s "✅ SVG layout pass"
line reflects a separate code path that resolves `<svg>`/`<rect>`/etc.
by tag-name string match during layout construction, not through the JS
DOM namespace at all, so rendering of declarative SVG already works;
only the JS-visible object model is wrong.

## Масштаб

This is the dominant single cause of the category's subtest failures:
every `baseVal`/`animVal` read (SVGAnimatedLength/-Number/-String/-Rect/
-TransformList — 260+38+17+12+6 ≈ 333 hits), every SMIL timeline call
(`pauseAnimations`/`unpauseAnimations`/`setCurrentTime`/`getCurrentTime`/
`beginElement`/`beginElementAt` ≈ 230 hits), every `getBBox()`/`getCTM()`
call (≈130 hits), every `SVGGeometryElement` path query
(`getTotalLength`/`getPointAtLength`/`isPointInFill`/`isPointInStroke`/
`setPathData` ≈ 45 hits), and every `SVGTextContentElement` character
query (`getNumberOfChars`/`getStartPositionOfChar`/etc. ≈ 10 hits) trace
back to this one gap — a working, tested SVG DOM implementation
(`svg.rs`) that the HTML parser never connects declarative markup to.
Affects every WPT category that embeds `<svg>` directly in HTML rather
than building it via `createElementNS` (this run's `svg/animations/`,
`svg/geometry/`, `svg/interact/`; likely also SVG-in-HTML subtrees of
already-vendored `html`/`css` categories that were not flagged at the
time because their `<svg>` usage was incidental to the test, not the
subject under test).

One more manifestation outside the SVG DOM itself, found 2026-08-20 while
fixing [BUG-412](BUG-412-FIXED.md): any shim code that keys off the
namespace sees markup-parsed foreign content as HTML. `document.getElementsByName`
must match HTML-namespace elements only (HTML LS §3.1.5), and its
`_lumen_is_html_namespace` filter is correct — but a `<svg name=x>` written
literally in the page reports `namespaceURI === 'http://www.w3.org/1999/xhtml'`
and so is still returned, which is exactly what `document.getElementsByName-namespace.html`
checks. The same blindness applies to `_lumen_html_collection_named`/
`_lumen_html_collection_own_names` (DOM §4.2.10.2 exposes the `name` attribute of
HTML-namespace elements only). Both close together with this bug, with no
separate fix point.

## Не расследовано

`ReferenceError: svg is not defined` (135 hits, `let svg =
document.querySelector("svg")` at file scope, later referenced from an
inline SMIL event-handler attribute like `onbegin="svg.pause..."`) —
plausibly correct behavior (the HTML LS "handler content attribute"
algorithm scopes inline handlers through `with` over several objects but
does **not** expose top-level `let`/`const` bindings from `<script>`
blocks the way `var` is), not re-derived byte-for-byte against the spec
this pass — flag for whoever picks up the fix, don't assume it is a
second bug without checking a handler-scope probe first.

## Дальше

Fix scope: give `create_element_with_attrs` (and whatever tracks the
open-elements stack / insertion mode) a foreign-content-aware namespace —
push a "current namespace" alongside the open-elements stack, switch it
to SVG on entering an `<svg>` start tag (and back to HTML on the
appropriate close/integration-point boundary per §13.2.6.5), and use it
both for `QualName` construction and to preserve SVG's case-sensitive
tag names via the spec's `adjustSVGTagNames` table (also fixes the
`tagName` half of [BUG-367](BUG-367-FIXED.md) for parser-created
elements, though not its `createElementNS` half). Once `namespaceURI`
is correct, `_lumen_element_prototype_for` (`dom.rs:1673`) already knows
to consult the SVG shim — no changes needed there — as long as `svg.rs`
also grows a `_lumen_element_prototype_for`-callable hook (today it only
patches `createElementNS`; the tag→constructor table `SVG_TAG_MAP`
already exists and can be reused directly). MathML foreign content has
the same gap but is out of scope for this WPT category and not measured
here.

## Пересверка 2026-08-20 (P3, срез 1 [BUG-413](BUG-413-FIXED.md)) — MathML тоже, и цена в сабтестах

Замерено при закрытии первого среза BUG-413 через шим (`v8_runtime_with_dom`,
`_lumen_get_namespace_uri`) — тот же дефект виден и на `innerHTML`-фрагменте, не
только на разборе полного документа:

```js
c.innerHTML = '<svg><rect></rect></svg><math><mi></mi></math>';
String(c.firstChild.namespaceURI)             // http://www.w3.org/1999/xhtml  (ждём .../2000/svg)
String(c.firstChild.firstChild.namespaceURI)  // http://www.w3.org/1999/xhtml  (ждём .../2000/svg)
String(c.lastChild.namespaceURI)              // http://www.w3.org/1999/xhtml  (ждём .../1998/Math/MathML)
String(c.firstChild.tagName)                  // SVG                           (ждём svg)
```

Уточнения к «Дальше» выше:

- **MathML не «вне скоупа», а тот же дефект того же места.** Раздел «Дальше»
  отложил его как не измеренный в категории `svg`; здесь он измерен и ведёт
  себя идентично (`<math>`/`<mi>` → `Namespace::Html`). Отдельного бага не
  требуется — таблицы §13.2.6.5 покрывают оба пространства имён сразу.
- **Третий пострадавший потребитель namespace** (к `getElementsByName` и
  прототипам SVG DOM): сеттеры `innerText`/`outerText`
  ([BUG-413](BUG-413-FIXED.md), срез 1) — члены `HTMLElement`, поэтому на
  SVG/MathML их быть не должно вовсе. Проверка сделана по `namespaceURI` и на
  `createElementNS`-элементах работает; на разобранных из разметки — нет.
  Цена ровно в сабтестах WPT: `innertext-setter.html` не берёт 4 (`<svg>`/`<math>`,
  обычный и detached), `outertext-setter.html` — 2.

## GAP-XMLDOC срез 3 (2026-09-13): namespace + adjustSVGTagNames в парсере

Реализована ровно parser-половина «Дальше» выше — SVG only, без MathML.
`IncrementalTreeBuilder` (`crates/engine/html-parser/src/tree_builder.rs`)
теперь ведёт «current namespace» как функцию от `open_elements.last()`
(`current_namespace`/`node_namespace`) вместо константного `Namespace::Html`:

- `resolve_element_name` присваивает `Namespace::Svg` элементу `<svg>` и всем
  потомкам, пока стек не вышел из foreign content; `create_element_with_attrs`
  строит `QualName` через неё вместо безусловного `QualName::html(name)`
  (было — строка 1879 из симптома выше, актуальный номер после доработок
  сдвинулся).
- Новый модуль `crates/engine/html-parser/src/foreign_content.rs` даёт
  таблицы `adjustSVGTagNames`/`adjustSVGAttributeNames` (HTML LS §13.2.6.5) —
  токенизатор лишает регистр каждое имя тега/атрибута ещё на этапе
  токенизации, так что `linearGradient`/`foreignObject`/`viewBox` и т.п.
  восстанавливаются по статической таблице, а не по исходному написанию.
- `apply_token` перед обычной `dispatch` проверяет `current_namespace() ==
  Svg` для Start/EndTag и уводит их в новую `dispatch_foreign_content` —
  упрощённую версию §13.2.6.5: breakout-список (`div`, `p`, `table`, …,
  `font` только с `color`/`face`/`size`) возвращает в HTML-режим и
  переигрывает токен через обычный `dispatch`; закрывающий тег ищет
  совпадение по стеку до первой HTML-границы. Text/Comment/Doctype токены
  через этот путь не идут — `insert_text` уже namespace-агностичен.
- `push_open_element`: self-closing (`/>`) теперь закрывает foreign-элемент
  сразу в ЛЮБОМ документе (не только `xml_mode`) — HTML LS §13.2.6.5 шаг 4
  требует этого безусловно, и `<svg><rect/><circle/></svg>` без фикса
  вложил бы `circle` внутрь `rect` тем же паттерном, что BUG-786 «Вторая
  грань» для XML-документов.

**Сознательно не сделано** (следующий срез, если/когда возьмут):
MathML (`<math>`/`<mi>`/…) — таблицы и breakout-список рассчитаны только на
SVG; HTML/MathML integration points (`<foreignObject>`/`<desc>`/`<title>`/
`annotation-xml`) не переключают детей обратно в HTML — сейчас всё под
`<foreignObject>` остаётся в SVG-неймспейсе, что спецификационно неверно,
но безопаснее, чем совсем не иметь foreign content; foreign-attribute
namespacing (`xlink:href` и т.п. остаются с `Namespace::Html` на самом
атрибуте, только `local` восстановлен по таблице). **Главное — из
симптома этого бага ещё не закрыто:** прототип-цепочка. Once namespace is
correct, `_lumen_element_prototype_for`
(`crates/js/src/shim/web_api_shim_mid.js:3215`) отдаёt голый
`Element.prototype` для любой не-HTML-namespace ноды по собственному
комментарию в коде — `svg.rs`'s `SVG_TAG_MAP` перевешивает прототип только
из-под монки-патча `document.createElementNS`, а не из общего пути
построения элемента (`_lumen_build_element`), так что `<rect>`, разобранный
из разметки, теперь корректно имеет `namespaceURI`/`tagName`, но всё ещё
`instanceof Element`, не `instanceof SVGRectElement` — `getBBox()` и весь
остальной SVG DOM по-прежнему недоступны на нём. Это отдельный, сравнимый
по объёму кусок в `crates/js/src/svg.rs` + `web_api_shim_mid.js`, не
затронутый этим срезом.

Тесты: `crates/engine/html-parser/src/foreign_content.rs` (таблицы) +
`crates/engine/html-parser/src/tree_builder.rs::tests::svg_*` (сквозной
разбор — namespace, регистр тега, self-closing, breakout). `cargo test -p
lumen-html-parser` — 424/424; `scripts/scoped-test.sh` (все обратные
зависимости, включая `lumen-driver`/`lumen-network`) — зелёный, кроме
чужого дрейфа CPU-эталонов, подтверждённого идентичным на чистом `main`
без этого среза (`55-text-rendering`/`57-canvas-2d`/`32-list-markers`/
`34-forms`/`45-multiple-backgrounds`/`51-scrollbar-rendering`/
`1000000-final` — те же расхождения в байтах что с патчем, что без).

## GAP-XMLDOC срез 4 (2026-09-13): прототип-цепочка parser-built SVG-элементов

Закрывает ровно тот остаток, который срез 3 назвал явно: «прототип-цепочка.
Once namespace is correct, `_lumen_element_prototype_for`
… отдаёт голый `Element.prototype`». `<rect>`, разобранный из разметки, имел
правильный `namespaceURI`/`tagName` (срез 3), но оставался `instanceof
Element`, не `instanceof SVGRectElement` — `getBBox()` и весь остальной SVG
DOM были недоступны.

Правка чисто в JS-шиме, без Rust:

- `svg.rs`: поиск конструктора по SVG-тегу вынесен из тела патча
  `document.createElementNS` в переиспользуемую `window._lumen_svg_ctor_for_local`
  (по-прежнему `SVG_TAG_MAP[local] || SVG_TAG_MAP[local.toLowerCase()] ||
  SVGElement`), `createElementNS` теперь тоже её вызывает — поведение не
  изменилось, только источник единый.
- `web_api_shim_mid.js::_lumen_element_prototype_for` (общий путь построения
  элемента, `_lumen_build_element`) получил ветку для namespace
  `http://www.w3.org/2000/svg`: берёт case-preserving локальное имя через уже
  существующий `_lumen_get_local_name` (не `_lumen_get_tag_name`, тот
  безусловно аплкейсит) и отдаёт `_lumen_svg_ctor_for_local(local).prototype`.
  Если SVG-шим не установлен в рантайме — фоллбэк на `Element.prototype`, как
  и раньше.

Теперь `<rect>`/`<circle>`/`<svg>` и т.д., разобранные из разметки (парсером
документа или через `innerHTML`, один и тот же `IncrementalTreeBuilder`),
получают тот же типизированный прототип, что и `createElementNS`-путь:
`instanceof SVGRectElement` истинно, `getBBox`/`getCTM`/остальной SVG DOM
доступны.

**Сознательно не сделано:** отражение анимируемых атрибутов
(`rect.width`/`svg.viewBox`/`g.transform` всё ещё дают `undefined` —
[GAP-SVGDOM](../ROADMAP.md) отдельной задачей, теперь разблокирована ровно
этим срезом) и MathML (тот же паттерн, но `MathMLElement`-иерархии в
кодовой базе ещё нет).

Тесты: новый `crates/js/src/dom/tests/v8_core/mod.rs::parser_built_svg_gets_typed_prototype`
(`innerHTML='<svg><rect/><circle/></svg>'` → `instanceof SVGSVGElement` /
`SVGRectElement` / `SVGCircleElement`, `getBBox` доступен). `cargo test -p
lumen-js --features v8-backend` — 3612/3612 (юнит) + 116/116 (интеграционные)
зелёные, включая все существующие `svg::tests_v8::*` и
`create_element_ns_builds_native_svg_tree`. `cargo clippy -p lumen-js
--all-targets --features v8-backend -- -D warnings` — чисто.

## GAP-XMLDOC срез 5 (2026-09-13): namespace-префиксный `html:`/`h:` breakout

Закрывает «Третья грань, случай 1» (срез 16) — `<h:script src="…"/>`/
`<html:script>…</html:script>` не становились скриптом вообще, внешний
файл не запрашивался. Измерение по вендоренному корпусу
(`grep -rhoE '<[a-zA-Z][a-zA-Z0-9]*:script\b'`) — **224 уникальных файла**
несут этот паттерн (212 с префиксом `h:`, 12 — `html:`), каждый гарантированно
TIMEOUT: харнесс физически не грузится. Тот же XHTML-namespace-идиом
(`xmlns:h="…/1999/xhtml"` на корне `<svg>`) используется в корпусе и для
`link`/`meta`/`div`/`frameset` — сотни occurrences, не только `script`.

Правка в трёх точках:

- `foreign_content::strip_known_html_prefix` — новая функция, жёстко
  зашитая пара префиксов (`html:`, `h:`), не резолвер: реальное XML
  namespace-разрешение потребовало бы ходить по цепочке предков в поисках
  `xmlns:*`-деклараций, что снова «вне духа точечных срезов» (тот же
  принцип уже заявлен в шапке модуля для MathML/integration
  points/foreign-attribute namespacing). Другие префиксы корпуса
  (`d:testDescription` — SVG 1.1 test-metadata, `m:mi` — MathML, `rdf:li`,
  `svg:svg`) не трогаются — только `h:`/`html:`, единственные два реально
  измеренных случая, где префикс означает XHTML.
- `tree_builder::apply_token` — при `xml_mode` снимает `html:`/`h:` с имени
  StartTag/EndTag БЕЗУСЛОВНО, ещё до SVG-роутинга: к моменту, когда придёт
  закрывающий `</html:div>`, элемент уже мог уйти из SVG-неймспейса в
  HTML (см. ниже), так что решение «снимать ли префикс» не может зависеть
  от текущего namespace на тот момент — только от литерального префикса в
  самом токене. `had_html_prefix` запоминает, что снятие произошло, и
  передаётся в `dispatch_foreign_content` как `forced_breakout`.
- `tree_builder::dispatch_foreign_content(token, forced_breakout)` —
  `forced_breakout` форсирует переход в HTML-неймспейс (тот же
  pop-до-первого-non-SVG-предка, что и обычный §13.2.6.5 breakout-список)
  даже для имён вроде `script`/`link`, которых в самом breakout-списке нет
  и быть не должно (простой `<script>` без префикса внутри `<svg>` —
  легальный SMIL/встроенный SVG-скрипт, должен остаться в SVG-неймспейсе,
  это НЕ регрессия).
- `tokenizer::is_raw_text_element`/`is_rcdata_element` — токенизатор не
  знает о namespace/tree builder state, только о литеральном имени тега,
  поэтому `html:script`/`h:script` (и `html:title`/`h:title` для RCDATA)
  добавлены туда же, жёстко, рядом с обычными `"script"`/`"title"` —
  иначе RAWTEXT-сканирование (в т.ч. совместно с CDATA-обёрткой среза 1)
  не запустится и `<![CDATA[...]]>` внутри `<h:script>` разберётся как
  markup, а не как литеральный текст.

**Побочный фикс, обнаруженный тестами этого среза:** `mode_in_body`'s
in-head-redirect для `base`/`basefont`/`bgsound`/`link`/`meta`/`noframes`/
`script`/`style`/`title` физически переключал `self.insertion_mode` в
`InHead` перед вызовом `self.dispatch(token)`, а не просто делегировал
обработку (спека §13.2.6.4.7 говорит «process the token using the rules
for the 'in head' insertion mode» — вызов, а не смена состояния).
RAWTEXT-теги (`script`/`style`/`noframes`/`title`) внутри `mode_in_head`
сохраняют текущий `self.insertion_mode` в `original_insertion_mode`,
чтобы вернуться к нему после закрывающего тега (§13.2.6.2 «generic raw
text element parsing algorithm») — но к моменту этого сохранения
`self.insertion_mode` уже был подменён на `InHead` самим редиректом, а не
реальным «снаружи» режимом (`InBody`, если тег встречен уже внутри
`<body>`, что как раз и происходит внутри `<svg>` из этого среза).
Первая попытка фикса («восстанавливать `saved` только если
`mode_in_head` не переключил режим», по образцу соседней ветки для
`<template>`) исправляла симптом не там: `original_insertion_mode`
всё равно фиксировал неверный `InHead`, и по закрытии RAWTEXT-элемента
парсер возвращался в `InHead`, чей fallback «anything else» интерпретирует
всё, что идёт дальше, как всё ещё находящееся в `<head>` — следующий тег
(`<p>` в регрессионном тесте) реализовывал implied-`<body>` заново,
создавая ВТОРОЙ `<body>` в дереве. Правильный фикс — не трогать
`self.insertion_mode` перед делегированием вовсе, звать `self.mode_in_head(token)`
напрямую как функцию: тогда `original_insertion_mode` корректно фиксирует
реальный текущий режим (`InBody`), и после RAWTEXT-содержимого парсер
возвращается именно туда.

**Сознательно не сделано:** произвольные `xmlns:*`-декларации (полноценный
резолвер), другие HTML-теги, которых корпус не показал с этим префиксом
(измерено только `script`/`link`/`meta`/`div`/`frameset` через грепы выше).

Тесты: пять новых в `crates/engine/html-parser/src/tree_builder.rs`
(`html_prefixed_script_breaks_out_of_svg_and_stays_rawtext`,
`html_prefixed_script_cdata_body_is_unwrapped`,
`html_prefixed_void_element_breaks_out`,
`html_prefixed_end_tag_closes_broken_out_element`,
`other_namespace_prefixes_do_not_break_out`) плюс два в
`foreign_content.rs` (`strips_known_html_prefixes`,
`leaves_other_prefixes_and_bare_names_alone`). `cargo test -p
lumen-html-parser` — 431/431 юнит + 9/9 интеграционных (`fragment_parsing`)
зелёные. `cargo clippy -p lumen-html-parser --all-targets -- -D warnings`
— чисто.

## GAP-XMLDOC срез 6 (2026-09-14): MathML namespace в парсере

Закрывает для MathML ровно то, что срез 3 закрыл для SVG: parser-side
namespace assignment. Измерение (корпус вендоренного WPT, `.xht`/`.xhtml`/
`.svg`, `grep -rlE '<math[ >]'`) — **113 файлов** несут `<math>` markup;
до этого среза каждый такой элемент и весь его поддерева уходил в
`Namespace::Html` тем же путём, что SVG до среза 3 (§13.2.6.5 не
применялась ни к одному из двух foreign-content неймспейсов, только теперь
у SVG есть свой путь, а у MathML — нет).

Механизм — обобщение среза 3, не переизобретение:

- `is_foreign_namespace(ns)` (новая свободная функция, верх
  `tree_builder.rs`) — `matches!(ns, Namespace::Svg | Namespace::MathMl)`.
  Три места, ранее сравнивавшие `current_namespace() == Namespace::Svg`
  напрямую (`apply_token`'s foreign-content routing,
  `dispatch_foreign_content`'s breakout pop-loop, `push_open_element`'s
  self-closing check), теперь зовут её — тем самым MathML бесплатно
  получает тот же breakout-список, тот же self-closing-в-foreign-content
  путь (§13.2.6.5 шаг 4, тот же, что нашёл 27 `flexbox-justify-content-
  vert-*.xhtml` в BUG-786), что уже был у SVG.
- `resolve_element_name`/`create_element_with_attrs` — новая ветка для
  `Namespace::MathMl`: `<math>` и всё, пока стек не покинул MathML,
  получают этот неймспейс; local name не приводится к camelCase (в
  отличие от SVG, у MathML нет таблицы регистро-чувствительных имён тегов
  — спека называет только один регистро-чувствительный **атрибут**,
  `definitionURL`, обслуженный новой `foreign_content::
  adjust_mathml_attribute_name`).
- §13.2.6.5 "any other start tag" breakout-список — общий для SVG и
  MathML по спеке, так что `breaks_out_of_foreign_content` не продублирован,
  только doc-comment в `foreign_content.rs` перестал говорить «SVG only».

**Сознательно не сделано** (следующий срез, если/когда возьмут, как и
прототип-цепочка SVG была отдельным срезом 4 после среза 3):
прототип-цепочка (`MathMLElement`-иерархии в JS ещё нет вовсе — грубее
пробела, оставленного срезом 4 для SVG, там классы уже существовали и не
хватало только подключения); HTML/MathML integration points
(`<annotation-xml>` с `encoding="text/html"`, `<mi>`/`<mo>`/`<mn>`/`<ms>`/
`<mtext>` как MathML text integration points) — по этому же срезу всё под
ними остаётся в MathML-неймспейсе, спецификационно неверно, но тем же
компромиссом, что срез 3 принял для `<foreignObject>`; foreign-attribute
namespacing не тронут (не входил в скоуп и для SVG).

Тесты: четыре новых в `crates/engine/html-parser/src/tree_builder.rs`
(`mathml_descendants_get_mathml_namespace`,
`mathml_definitionurl_attribute_case_is_restored`,
`mathml_self_closing_elements_do_not_nest_siblings`,
`mathml_breakout_tag_returns_to_html_namespace`) плюс один в
`foreign_content.rs` (`adjusts_mathml_definitionurl`). `cargo test -p
lumen-html-parser` — 443/443 юнит + 9/9 интеграционных зелёные. `cargo
clippy -p lumen-html-parser --all-targets -- -D warnings` — чисто.
`scripts/scoped-test.sh` (все обратные зависимости) — зелёный, кроме
чужого дрейфа CPU-эталонов (`lumen-driver`), подтверждённого идентичным
предыдущими срезами (`55-text-rendering`/`57-canvas-2d`/`32-list-markers`/
`34-forms`/`45-multiple-backgrounds`/`51-scrollbar-rendering`/
`1000000-final`), и разового флака `lumen-js --lib` под нагрузкой полного
прогона — прогнан отдельно (`cargo test -p lumen-js --lib --features
v8-backend`) сразу после, 3612/3612 зелёные.

## GAP-XMLDOC срез 7 (2026-09-14): прототип-цепочка для parser-built MathML-элементов

Закрывает для MathML ровно то, что срез 4 закрыл для SVG: `<math>`-разметка
(и `document.createElementNS('http://www.w3.org/1998/Math/MathML', ...)`)
теперь даёт типизированный прототип вместо голого `Element.prototype`.
Отличие от SVG — по объёму, не по механизму: MathML Core §2.2 определяет
РОВНО ОДИН интерфейс, `MathMLElement`, для всех элементов неймспейса; нет ни
таблицы тег→конструктор (`SVG_TAG_MAP`), ни отдельного `getBBox`-подобного
API для этого интерфейса — сам класс существовал бы пустым, если бы не был
нужен как якорь для `instanceof` и как цель `_lumen_element_prototype_for`.

- Новый модуль `crates/js/src/mathml.rs` (по образцу `svg.rs`, но на два
  порядка меньше) — `class MathMLElement extends Element {}`, без
  `focus`/`blur`-заглушки (SVGElement её несёт, но MathML-элементы не
  фокусируемы по спеке) и без per-tag map. `window.MathMLElement` +
  `window.MATHML_NAMESPACE` — тем же путём, что SVG вешает `SVG_NAMESPACE`.
  Подключён в `install_v8!`-батч `v8_runtime.rs` сразу после
  `svg::install_svg_bindings_v8`.
- `_lumen_element_prototype_for` (`web_api_shim_mid.js`) — новая ветка для
  MathML-неймспейса перед общим HTML-путём: `MathMLElement.prototype`, если
  шим установлен, иначе `Element.prototype` (тот же безопасный fallback, что
  и у SVG-ветки). В отличие от SVG-ветки, локальное имя не читается —
  незачем, интерфейс один на всех.
- `_lumen_create_element_ns` (`v8_runtime/install/dom_core.rs`) — namespace-
  селектор получил ветку `"http://www.w3.org/1998/Math/MathML" =>
  Namespace::MathMl` рядом с существовавшей SVG-веткой; раньше
  `createElementNS` на этом namespace URI молча откатывался в `Namespace::Html`
  (тот же класс проблемы, что и общий пробел BUG-830 для произвольных
  namespace URI — тут закрыт только этот один известный случай, не общий
  регистр).

**Сознательно не сделано** (следующий срез, если/когда возьмут): HTML/MathML
integration points (`<annotation-xml>` с `encoding="text/html"`,
`<mi>`/`<mo>`/`<mn>`/`<ms>`/`<mtext>` как MathML text integration points) —
тот же вырез, что срез 6 оставил открытым для namespace assignment, теперь
открыт и для прототипов: всё под этими точками остаётся с MathML-прототипом,
хотя по спеке должно переключаться на HTML-прототип цепочки.
Никакого нового JS DOM API у `MathMLElement` не появилось — MathML Core не
даёт этому интерфейсу собственных методов сверх базового `Element`.

Тесты: три новых юнит-теста в `crates/js/src/mathml.rs`
(`mathml_element_class_exists`, `mathml_element_extends_element`,
`mathml_namespace_constant_is_set`) плюс два интеграционных в
`crates/js/src/dom/tests/v8_core/mod.rs`
(`parser_built_mathml_gets_typed_prototype`,
`create_element_ns_mathml_gets_typed_prototype`) — зеркалят пару SVG-тестов
среза 4. `cargo test -p lumen-js --lib --features v8-backend` — 3616/3617
зелёные (один разовый флак `worker_blob_url_script`, тот же паттерн, что и
у среза 6 — зелёный при изолированном прогоне). `cargo clippy -p lumen-js
--all-targets --features v8-backend -- -D warnings` — чисто.

## GAP-XMLDOC срез 8 (2026-09-14): HTML/SVG/MathML integration points

Закрывает остаток, который срезы 3 и 6 сознательно оставили открытым:
HTML LS §13.2.6.5 «integration point» — markup, вложенная в
`<foreignObject>`/`<desc>`/`<title>` (SVG) или в MathML `<annotation-xml
encoding="text/html">`/`<mi>`/`<mo>`/`<mn>`/`<ms>`/`<mtext>`, — настоящий
HTML-контент, вложенный в дерево прямо под этими узлами, а не «отброшенный
наверх» через breakout-список, и не остающийся в foreign-неймспейсе.

Механизм — обобщение уже существующего пути, не новый код-путь:

- `foreign_content::is_svg_html_integration_point`/
  `is_mathml_text_integration_point` — новые таблицы (по образцу
  `breaks_out_of_foreign_content`).
- `tree_builder::start_tag_namespace(name)` — новый метод, вычисляющий
  неймспейс для СОЗДАВАЕМОГО start-тега с учётом текущего узла: обычный
  `current_namespace()`, кроме двух перевесов из §13.2.6.5 — (1) текущий
  узел является integration point → новый элемент по умолчанию HTML,
  а не наследует foreign-неймспейс родителя (`is_integration_point_host`,
  учитывает исключение `mglyph`/`malignmark` для MathML text integration
  points через параметр `name`); (2) `<svg>` прямо под `annotation-xml`
  всегда становится SVG-элементом независимо от `encoding` (спека
  «insert a foreign element», шаг с particular exception).
  `has_html_encoding` проверяет атрибут `encoding` узла `annotation-xml`
  на `text/html`/`application/xhtml+xml` (регистронезависимо).
- `apply_token` — маршрутизация start-тега в `dispatch_foreign_content`
  теперь смотрит на `start_tag_namespace(name)`, а не на сырой
  `current_namespace()`; end-тег маршрутизируется как раньше (у спеки нет
  integration-point исключения для закрывающих тегов — общий
  by-name-поиск по стеку в `dispatch_foreign_content` уже корректен для
  этого случая).
- `resolve_element_name` — та же замена `current_namespace()` на
  `start_tag_namespace(name)`; когда `apply_token` уже решил, что тег
  обрабатывается «как HTML», элемент создаётся с `Namespace::Html`, даже
  если родитель в стеке остаётся SVG/MathML — соответствует тому, что в
  дереве под integration point лежат настоящие HTML-элементы, а не
  «прикидывающиеся» foreign.

**Не задето integration-point'ами**: end-теги (не нужно — см. выше),
foreign-attribute namespacing (не входило и раньше). **Обнаруженный,
но не устранённый смежный пробел**: токенизатор решает RAWTEXT/RCDATA по
голому имени тега без учёта неймспейса (тот же корень, что и обходной путь
`html:`/`h:` для `<script>`/`<title>` в срезе 5) — SVG `<title>`,
содержащий markup, поэтому мис-токенизируется точно как обычный HTML
`<title>` не на своём месте (весь `<b>...</b>` читается как буквальный
текст). Отдельный тест (`svg_desc_is_html_integration_point`) сознательно
проверяет только `<desc>`, не `<title>`, и документирует находку —
починка требует, чтобы токенизатор знал про namespace-состояние дерева,
что структурно больше, чем точечный срез.

Тесты (`crates/engine/html-parser/src/tree_builder.rs`):
`svg_breakout_tag_returns_to_html_namespace` (переписан на `<g>` вместо
`<foreignObject>`, чтобы отличать обычный breakout от integration point,
плюс проверка позиции в дереве через `Node::parent`),
`svg_foreign_object_is_html_integration_point`,
`svg_desc_is_html_integration_point`,
`svg_reenters_foreign_namespace_from_inside_integration_point`,
`mathml_breakout_tag_returns_to_html_namespace` (аналогично переписан на
`<mrow>`), `mathml_text_integration_point_nests_html_children`,
`mathml_text_integration_point_mglyph_stays_mathml`,
`mathml_annotation_xml_html_encoding_is_integration_point`,
`mathml_annotation_xml_without_html_encoding_is_not_integration_point`,
`svg_always_becomes_svg_under_annotation_xml_regardless_of_encoding`; плюс
`svg_integration_points_detected`/`mathml_text_integration_points_detected`
в `foreign_content.rs`. `cargo test -p lumen-html-parser` — 453/453 юнит +
9/9 интеграционных зелёные. `cargo clippy -p lumen-html-parser
--all-targets -- -D warnings` — чисто. `scripts/scoped-test.sh` (все
обратные зависимости, 17 крейтов) — зелёный, кроме того же чужого дрейфа
CPU-эталонов (`lumen-driver::cases::snapshot_cpu`,
`55-text-rendering`/`57-canvas-2d`/`32-list-markers`/`34-forms`/
`45-multiple-backgrounds`/`51-scrollbar-rendering`/`1000000-final`),
подтверждённого идентичным на чистом `main` без этого среза.

## GAP-XMLDOC срез 10 (2026-09-14): foreign-attribute namespacing (`xlink:href` и другие)

Закрывает пробел, который срезы 3 и 8 сознательно оставляли открытым:
HTML LS §13.2.6.5 "adjust foreign attributes" — одиннадцать имён
(`xlink:actuate`/`arcrole`/`href`/`role`/`show`/`title`/`type`,
`xml:lang`/`space`, `xmlns`, `xmlns:xlink`) внутри SVG или MathML должны
получать реальный namespace вместо обычного HTML-атрибута. По корпусу
вендоренного WPT `xlink:`-атрибуты встречаются в **763** `.svg`/`.xhtml`/
`.xht`/`.html`-файлах — на два порядка больше, чем у любого другого
оставшегося пробела GAP-XMLDOC (self-closing `table`/`select`/`button` —
0 совпадений тем же грепом, до сих пор неизмерены; SVG `<title>` с markup,
не знающий о namespace токенизатор — 1 файл).

Живой, воспроизводимый дефект, а не только пробел в спеке: JS-мост уже
явно документировал игнорирование — `getAttributeNS`/`setAttributeNS`/
`hasAttributeNS`/`removeAttributeNS` принимали и отбрасывали аргумент
namespace (комментарий "the namespace argument is accepted but ignored",
BUG-309), а `Attr.namespaceURI` был захардкожен в `null` — при том что
структура `Attribute`/`QualName`/`Namespace` (`crates/engine/dom/src/lib.rs`)
уже несёт поле namespace и варианты `Xml`/`XmlNs`/`XLink` уже существовали,
просто ничего их не заполняло и не читало.

Механизм — точечная доработка, не резолвер (та же граница, что и у
[`strip_known_html_prefix`], срез 5):

- `foreign_content::adjust_foreign_attribute` — новая таблица (по образцу
  `adjust_mathml_attribute_name`), классифицирует ровно одиннадцать имён в
  `Namespace`; токенизатор уже лишь приводит атрибуты к нижнему регистру, и
  все одиннадцать имён и так целиком строчные в спеке, так что
  восстанавливать нечего — только классификация.
- `tree_builder::create_element_with_attrs` — шаг «adjust foreign attributes»
  идёт ПЕРЕД таблицами case-restoration для SVG/MathML и применяется на
  любом элементе в обоих foreign-неймспейсах разом (в отличие от
  `adjust_svg_attribute_name`/`adjust_mathml_attribute_name`, которые
  привязаны каждая к своему), как того требует спека. `local` в
  `QualName` остаётся полным квалифицированным именем (`xlink:href`, не
  `href`) — у модели атрибутов Lumen нет отдельного поля prefix, так что
  строка одновременно служит и ключом хранения, которым уже пользуются
  существующие сайты `Node::get_attr("xlink:href")`
  (`crates/engine/layout/src/box_tree/svg.rs`,
  `crates/engine/layout/src/box_tree/image_requests.rs`), и именем
  сериализации; меняется только `namespace`.
- JS-мост (`crates/js/src/v8_runtime/install/dom_core.rs`,
  `crates/js/src/v8_runtime/dom_helpers.rs`,
  `crates/js/src/shim/web_api_shim_mid.js`): новый натив
  `_lumen_get_attr_namespace_uri` питает `Attr.namespaceURI` (было
  `null` всегда); новый натив `_lumen_find_attr_by_ns` находит хранимое
  квалифицированное имя атрибута по (namespace URI, local name) и питает
  `getAttributeNS`/`hasAttributeNS`/`removeAttributeNS`; новый натив
  `_lumen_set_attr_ns` (+ `dom_helpers::set_attribute_ns`) заводит атрибут
  с реальным namespace вместо всегда-`Html` для `setAttributeNS`. Namespace
  URI, не входящий в известный набор (xlink/xml/xmlns/svg/mathml), даёт
  плоский поиск по имени — тот же откат на `Html`, что уже делает
  `_lumen_create_element_ns` для элементов (BUG-830, «нет общего реестра
  namespace»), сохраняющий дособытийное поведение для `getAttributeNS`
  с произвольным `ns`, а не превращающий каждый такой атрибут в
  ненаходимый.

**Сознательно не сделано**: `NamedNodeMap.getNamedItemNS`/
`setNamedItemNS`/`removeNamedItemNS` и `Element.getAttributeNodeNS`
по-прежнему откатываются на плоский по-имени путь (не входили в измеренный
корпусом список, тот же выбор границы, что и general namespace-prefix
resolution). Общая проблема — токенизатор выбирает RAWTEXT/RCDATA без учёта
namespace (найдено в срезе 8, SVG `<title>` с markup) — не тронута, как и
self-closing `<table>`/`<select>`/`<button>` (срез 2, по-прежнему 0
совпадений по корпусу).

Тесты: `svg_xlink_href_gets_xlink_namespace`,
`mathml_xlink_href_gets_xlink_namespace`,
`xmlns_and_xml_lang_get_their_namespace`,
`plain_svg_attributes_keep_html_namespace` (`tree_builder.rs`);
`foreign_attributes_get_their_namespace`,
`plain_and_unknown_prefixed_attributes_are_not_foreign`
(`foreign_content.rs`); `attribute_ns_methods_are_namespace_aware_for_xlink`,
`parser_built_xlink_href_reports_its_real_namespace_uri`,
`plain_attribute_namespace_uri_is_null`, и обновлённый
`attribute_ns_methods_fall_back_to_name_based_for_an_unknown_namespace`
(было `..._are_name_based`, `crates/js/src/dom/tests/v8_perf_observers.rs`).
`cargo test -p lumen-html-parser` — 461/461 юнит + 9/9 интеграционных
зелёные. `cargo test -p lumen-js --lib --features v8-backend` — 3620/3620
зелёные. `cargo clippy -p lumen-html-parser --all-targets -- -D warnings` и
`cargo clippy -p lumen-js --all-targets --features v8-backend -- -D
warnings` — чисто. `scripts/scoped-test.sh` (17 крейтов) — зелёный, кроме
того же чужого дрейфа CPU-эталонов (`lumen-driver::cases::snapshot_cpu`,
тот же список семи фикстур, что и в срезах 6–9), не связанного с этой
правкой (парсер атрибутов и JS-мост, не растеризация).

## GAP-XMLDOC срез 11 (2026-09-14): ре-замер остатков — все три измеренных случая уже закрыты

Взял задачу с первой строки `STATUS-P1.md` (`ROADMAP.md:896`), собираясь
закрыть следующий измеренный остаток. Перед правкой заново прогнал по
вендоренному WPT-корпусу все три пункта, оставленные «сознательно не
сделано» в срезах 2/5/9/10:

- `<h:script src="…"/>` (BUG-786, «Третья грань, случай 1») — **212**
  файлов с `h:`, ещё 12 с `html:`. `docs/engine-gaps.md` до этой правки
  всё ещё утверждал «never requested» — стало ясно из чтения самого
  же файла среза 5 (`## GAP-XMLDOC срез 5`, выше), что случай уже закрыт:
  снятие префикса форсирует HTML-неймспейс, а `crates/shell/src/scripts.rs`
  ищет `<script>` только по `name.local == "script"`, без фильтра по
  namespace, — значит загрузчик уже видит такой узел как обычный
  `<script src>`. Добавлен регрессионный тест
  `xml_flavoured_self_closing_h_script_with_src_is_requestable`
  (`tree_builder.rs`) — прогоняет ровно корпусную идиому
  (`xmlns:h`, self-closing, атрибут `src`) и проверяет и namespace, и
  сохранность атрибута, и то, что сосед после тега не потерян; тест
  зелёный без единой правки движка.
- Самозакрывающийся `<script src="…"/>` без префикса (BUG-786, «случай
  2») — уже закрыт срезом 2 (`xml_flavoured_self_closing_script_does_not_swallow_following_markup`,
  существующий тест зелёный).
- Самозакрывающиеся `<table>`/`<select>`/`<button>` — **0** совпадений
  тем же грепом, что и в срезах 9/10 (`grep -rlE '<(table|select|button)[^>]*/>'`
  по `.svg`/`.xhtml`/`.xht`/`.html`); единственное совпадение
  (`nested-select-crash.html`) — обычный `.html`-файл вне XML-пути,
  к GAP-XMLDOC не относится.
- SVG `<title>` с вложенной разметкой (токенизатор без
  namespace-awareness, срез 5) — **0** файлов с реальным вложенным тегом
  внутри `<title>` (проверено PCRE-грепом на непустой контент между
  `<title>` и `</title>`, содержащий открывающий тег).

Правка этого среза — только документация: `docs/engine-gaps.md` нёс
устаревшее «never requested»/«swallows the rest of the document» —
дописано выше. Остаток GAP-XMLDOC на текущий замер: два случая с 0
измеренным совпадением (не чинятся без корпусного примера, тот же
принцип, что и раньше) плюс архитектурный пробел — настоящего
namespace-резолвера (`xmlns:*` по цепочке предков) как не было, так и
нет, что и держит `ROADMAP.md:896` в статусе `planned`. Следующему
срезу нужен либо новый измеренный случай, либо явное решение закрыть/
сузить задачу — компромисс между «полноценный XML-парсер» и «серия
точечных срезов» упирается в то, что каждый оставшийся пункт сейчас
даёт 0 на корпусе.

`cargo test -p lumen-html-parser` — 462/462 юнит + 9/9 интеграционных
зелёные. `cargo clippy -p lumen-html-parser --all-targets -- -D
warnings` — чисто.

## GAP-XMLDOC срез 12 (2026-09-14): RAWTEXT/RCDATA тоже namespace-blind — исправлено, измерено через вендоренный WPT-тест

Срез 11 закрыл grep-по-корпусу пункты, но упустил один из двух, оставленных
`docs/engine-gaps.md`: «`<title>`/`<script>` whose RAWTEXT/RCDATA state the
tokenizer picks by bare tag name, not tree namespace». Grep по XML-корпусу на
вложенный тег внутри `<title>` дал 0 (проверено срезом 11), но сам дефект не
ограничен `<title>` — он бьёт по `<script>` в SVG/MathML тоже, а measurable
evidence для него не в корпусе, а в уже вендоренном
`tests/wpt/html/syntax/parsing/unclosed-svg-script.html` (`svg` scripts с
bogus end tag/breakout/self-closing внутри — ровно этот случай).

**Причина.** `crates/engine/html-parser/src/tokenizer.rs::consume_start_tag`
переключает токенизатор в RAWTEXT (`<script>`/`<style>`) или RCDATA
(`<title>`/`<textarea>`) по голому имени тега — `Tokenizer`/`PushTokenizer`
вообще не видят namespace/insertion mode, эта информация целиком в
`tree_builder.rs`. По HTML LS §13.2.6.5 «generic raw text/RCDATA element
parsing algorithm» вызывается только HTML insertion-mode правилами («in
head»/«in body»), никогда — «rules for parsing tokens in foreign content»:
настоящий foreign `<script>`/`<title>` (SVG/MathML, не integration point)
не должен переводить токенизатор в text-only вовсе. Из-за этого
`<svg><script>a=1;</g></script></svg>` в Lumen читал RAWTEXT до первого
литерального `</script>` — т.е. включая `</g>` — и подсовывал JS-парсеру
невалидный код `a=1;</g>` (`Unexpected token '<'`), а не корректные
`a=1;` + отдельный (проигнорированный внутри foreign content) `</g>`.

**Живое измерение до правки** (`run_smoke.py` через полный streaming/network
путь, тот же, что для реальных страниц): `unclosed-svg-script.html` —
`TEST_END: ERROR` (0/5 сабтестов даже выполнились — JS-исключение при
загрузке `testharness.js` рушило тест целиком, потому что RAWTEXT-мусор
из первого `<svg><script>` утаскивал за собой всё, вплоть до
harness-скриптов). После правки — `TEST_END: Test OK. Subtests passed
2/5`: два сабтеста, ровно завязанных на этот дефект («SVG scripts with end
tag should run», «SVG scripts with bogus end tag inside should run»), стали
зелёными.

**Исправление — в двух местах, потому что решение принимается ПОСЛЕ того,
как токенизатор уже переключился, а не до:**

- `Tokenizer::cancel_text_only()` (`tokenizer.rs`) — отменяет
  RAWTEXT/RCDATA, которое `consume_start_tag` только что выставил для
  последнего `StartTag`. Вызывать нужно до следующего `next()`.
- Pull-режим (`parse`/`parse_xml_flavoured`/`parse_fragment`,
  `tree_builder.rs`) — раньше гонял `for token in Tokenizer::new(input) {
  builder.apply_token(token) }`; вынесено в общий `run_pull`, который после
  каждого `StartTag` спрашивает новый `IncrementalTreeBuilder::
  current_context_forbids_text_only()` (= `is_foreign_namespace(
  current_namespace())`, тот самый namespace, что уже учитывает
  integration-point- и `html:`/`h:`-breakout-резолвинг из срезов 5/8 —
  никакого дублирования логики) и, если да, зовёт `cancel_text_only()`.
- Push-режим (`IncrementalTreeBuilder::feed`/`feed_bytes`/`finish`, реальный
  путь сетевой загрузки страниц) — сложнее: `PushTokenizer::tokenize`
  раньше делал `(&mut tokenizer).collect()` на весь «безопасный» кусок
  буфера ЦЕЛИКОМ, до того как `apply_token` видел хоть один токен из
  этого куска, — отменять было уже поздно и не для чего (токенизатор к
  этому моменту успевал целиком просканировать RAWTEXT). Переписано на
  `feed_with_context`/`feed_bytes_with_context`/`end_with_context` —
  каждый токен уходит в переданный `on_token` немедленно, пока внутренний
  `Tokenizer` ещё жив, и `on_token` (в `tree_builder.rs` —
  `apply_token_for_stream`) успевает применить токен к дереву и тут же
  отменить text-only, если контекст запрещает. Старые `feed`/`feed_bytes`/
  `end` (используются ~30 юнит-тестами `push_tokenizer.rs`) сохранены
  как есть — просто оболочка над той же логикой с `on_token = |_| false`.

**Не закрыто этим срезом** — три сабтеста того же файла остаются
`FAIL` (metadata обновлена: `expected: ERROR` на весь тест → per-subtest
`expected: FAIL` на эти три, `tests/wpt/metadata/html/syntax/parsing/
unclosed-svg-script.html.ini`):

- «SVG scripts without end tag should not run» — незакрытый `<script>`
  внутри `<svg>` (RAWTEXT остаётся не активной, но контент всё равно
  выполняется — вероятно, отдельный порядок операций foreign-content
  dispatch, не исследовано);
- «SVG scripts ended by HTML breakout should not run» — breakout-тег
  (`<s>`) внутри текста `<script>` в foreign content;
- «SVG scripts with self-closing start tag should run» — самозакрывающийся
  SVG `<script href="…"/>` (тот же класс, что self-closing
  `<table>`/`<select>`/`<button>` из срезов 9/11 — отдельная задача,
  не про RAWTEXT/RCDATA).

`docs/engine-gaps.md` обновлён — снята закрытая часть, оставлены эти три
как явный остаток.

`cargo test -p lumen-html-parser` — 462 юнит (tree_builder) + 41 юнит
(push_tokenizer) + 9 интеграционных (`fragment_parsing.rs`) зелёные, без
единой правки существующих тестов. `cargo clippy -p lumen-html-parser
--all-targets -- -D warnings` — чисто.
