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

## GAP-XMLDOC срез 13 (2026-09-14): исполнимость foreign `<script>` — закрывает файл целиком (5/5)

Срез 12 закрыл RAWTEXT/RCDATA, но оставил три сабтеста
`unclosed-svg-script.html` красными — все три об одном и том же: движок
исполнял foreign (SVG) `<script>`, закрытый ЛЮБЫМ способом, если в его
детях осталось непустое текстовое содержимое. По HTML LS §13.2.6.5
«already started»-флаг настоящего движка выставляется только двумя
путями — литеральный `</script>` конец-тега, пока скрипт ещё текущий
узел, или self-closing стартовый тег (шаг 7 того же алгоритма, который
также требует исполнить скрипт). Любое другое закрытие — неявный pop как
побочный эффект конец-тега предка (`</svg>` после незакрытого `<script>`)
или foreign-content breakout-тег (`<s>` внутри текста скрипта) — не
взводит этот флаг вовсе, хотя уже разобранный текст остаётся в дереве как
обычный контент.

**Почему это отдельная точка правки, а не пере-использование RAWTEXT-фикса
среза 12.** Lumen не моделирует «already started» во время разбора —
исполнение скриптов целиком отложено до постпарсингового обхода дерева в
порядке документа (`collect_scripts_ordered`, `crates/shell/src/scripts.rs`),
который сегодня считает исполнимым любой `<script>` с непустым текстом
или `src`. Три красных сабтеста требуют, чтобы ЭТОТ обход знал то, что
знает только парсер в момент закрытия элемента — само по себе решение
не выводится из финального дерева.

**Исправление — три места:**

- `Document` (`crates/engine/dom/src/lib.rs`) получил
  `non_executable_foreign_scripts: HashSet<NodeId>` плюс
  `mark_foreign_script_not_executable`/`is_script_executable` — тот же
  приём, что уже применён для `viewport_meta`/`meta_refresh`: парсер
  пишет наблюдение в `Document`, шелл читает его позже.
- `tree_builder.rs::dispatch_foreign_content` — новый хелпер
  `mark_if_foreign_script_not_executable` вызывается в двух местах:
  - breakout-ветка (forced или из `foreign_content::breaks_out_of_foreign_content`)
    — помечает КАЖДЫЙ foreign `<script>`, снятый со стека в цикле
    `while is_foreign_namespace(...) { pop() }`, до того как он выпадет;
  - ветка `EndTag` — если найденная граница `i` НЕ верх стека (то есть
    искомый тег закрыл что-то помимо самого себя, включая случай, когда
    искомый тег — вовсе не `"script"`) или найденное имя не `"script"`,
    все `<script>`-элементы в `open_elements[i..]` перед `truncate`
    помечаются. Единственное исключение (`top_is_direct_script_close`) —
    буквально `</script>`, пока скрипт был верхним элементом стека: это и
    есть спековая «current node is an SVG script element» ветка.
- `crates/shell/src/scripts.rs::collect_scripts_ordered` (и
  `collect_inline_scripts`, тот же обход из другого потребителя) —
  проверяет `doc.is_script_executable(id)` первым делом внутри ветки
  `<script>`, и разрешает внешний источник искать `href`/`xlink:href`
  вдобавок к `src`, когда элемент в `Namespace::Svg` (само-закрывающийся
  `<script href="…"/>` иначе даже не попадал бы в исполнение — SVG не
  знает атрибута `src`).

**Живое измерение** (`run_smoke.py`, полный streaming/network путь):
`unclosed-svg-script.html` — было `TEST_END: Test OK. Subtests passed
2/5`; стало `TEST_END: Test OK. Subtests passed 5/5` — все три
`UNEXPECTED-PASS`, `.ini`-ожидания (`tests/wpt/metadata/html/syntax/
parsing/unclosed-svg-script.html.ini`) удалены целиком, файл больше не
нуждается в отдельных expectations.

Новый интеграционный набор
`crates/engine/html-parser/tests/foreign_script_execution.rs` (не в
`tree_builder.rs` — тот файл уже 5232 строки, выше грандфазеренного
лимита в 2000, растить нельзя) — 5 тестов через публичный `parse`/
`Document::is_script_executable`, по одному на каждую ветку алгоритма
(закрыт своим тегом, не закрыт, закрыт breakout'ом, self-closing,
регрессия на bogus-end-tag среза 12).

`cargo test -p lumen-html-parser` — 462+9+5 зелёных (5 новых из
`foreign_script_execution.rs`, ни одна существующая не тронута).
`cargo clippy -p lumen-dom -p lumen-html-parser --all-targets -- -D
warnings` и `cargo clippy -p lumen-shell --all-targets --profile
dev-release -- -D warnings` — чисто.

Закрывает GAP-XMLDOC срез 12's remainder полностью — `unclosed-svg-
script.html` больше не появится ни в одном списке остатков.

## GAP-XMLDOC срез 14 (2026-09-14): контекст фрагмента и CDATA-секции (`p1-gap-xmldoc-srez14`)

Ре-замер `html/syntax/parsing` (после того, как срез 13 закрыл файл
целиком) нашёл два ранее неизмеренных красных файла из того же
семейства, оба про то, чего в `parse_fragment` не было вовсе (задокументировано
как известный пробел ещё в исходной формулировке `parse_fragment`):
реальный контекстный элемент §13.4 и CDATA-секции.

**`cdata-in-integration-point-fragment.html` (0/9 → 9/9).** HTML LS
§13.2.6.5 "adjusted current node" в fragment-случае — пока стек
открытых элементов держит только синтетический корень, adjusted current
node = контекстный элемент, а не сам корень (всегда HTML-неймспейса).
`parse_fragment` игнорировал контекст целиком (`el.innerHTML=` для
`createElementNS`-элемента разбирался как `<body>`-контекст), поэтому
namespace-зависимые решения (CDATA-allowed, integration-point routing)
были недостижимы для этого пути вообще.

- Новый `parse_fragment_with_context(input, Option<FragmentContext>)`
  (`tree_builder.rs`) — `FragmentContext { namespace, local, attrs }` не
  вставляется в дерево (синтетический `<html>`-корень остаётся HTML,
  как требует спека), а лишь подменяет `current_namespace()`/
  `start_tag_namespace()` ровно пока `open_elements.len() == 1`.
  `parse_fragment` — тонкая обёртка с `context: None`, старое
  поведение не тронуто ни для одного существующего вызывающего.
- `start_tag_namespace`/`is_integration_point_host` переписаны в
  параметризованную `resolve_content_namespace(ns, local,
  has_html_encoding, tag_name)`, общую для реального узла стека и для
  контекста — код integration-point-таблицы (срез 8) не задублирован.
- CDATA-секции — новый механизм в `tokenizer.rs`, отсутствовавший
  целиком (`<![CDATA[...]]>` раньше молча поглощался без единого
  токена, даже в HTML-контенте): `Tokenizer::set_cdata_allowed(bool)`,
  вызывается `run_pull` перед каждым `next()` по свежепосчитанному
  `IncrementalTreeBuilder::cdata_sections_allowed()` (тот же
  `resolve_content_namespace` с `tag_name=""`, т.к. CDATA — не тег, и
  MathML-исключение `mglyph`/`malignmark` тут не при делах). Разрешено
  → `Token::Text`; запрещено → HTML LS "cdata-in-html-content" parse
  error, bogus comment с данными `[CDATA[...` (было — вообще ничего,
  ни узла, ни текста). Реализовано только в pull-режиме
  (`parse`/`parse_fragment*`) — `PushTokenizer` (сетевая загрузка)
  флаг никогда не взводит, там `<![CDATA[` теперь тоже bogus comment
  (было — молчаливое поглощение), полноценная foreign-content поддержка
  в потоковом пути остаётся отдельным срезом.
- Мост до JS: `_lumen_set_inner_html` (`dom_core.rs`) теперь передаёт
  `nid` — сам элемент, на который вызван `Element.innerHTML=` (не
  `target`, который для `<template>` перенаправлен на content-фрагмент)
  — как контекст в новый `dom_helpers::parse_html_fragment_with_context`.
  `outerHTML`/`insertAdjacentHTML` (`_lumen_parse_html_fragment`) не
  тронуты — их контекст (родитель цели) не измерен.

**`html_content_in_foreign_context.html` (0/1 → 1/1).** Отдельный,
существовавший до контекстной работы баг в `dispatch_foreign_content`'s
EndTag-ветке: тест перебирает полный breakout-список (§13.2.6.5) не
только как start tag (уже работало, срезы 3/6), но и как ГОЛЫЙ END TAG
без открытого соответствия (`e[0]=='/' → "/p"`/`"/br"`, т.е. разметка
`<svg></p></svg`) — поиск по стеку не находил совпадения, доходил до
HTML-namespace `<body>`/корня и просто игнорировал тег целиком (ни
`<p>`, ни `<br>` не создавались). Реальные браузеры для ИМЕННО этих
breakout-имён создают настоящий элемент (`<br>` self-closing, пустой
`<p>`) как ребёнка ближайшего HTML-предка — тот же эффект, что у
start-tag breakout'а. Исправление: если имя end tag'а само есть в
`foreign_content::breaks_out_of_foreign_content`, ветка сразу пуляет
(pop while foreign, тот же цикл, что у start-tag breakout'а) и
диспатчит EndTag через `self.dispatch` (routing в "in body", чьи
собственные специальные правила для `br`/`p` создают элемент) — имя НЕ
из breakout-списка (например бессмысленный `</g>`) остаётся на старом
generic-поиске-или-игнорировать, чтобы не схлопывать реально открытые
элементы (регрессия на это — `svg_script_with_bogus_end_tag_inside_
stays_executable`, срез 12: `</g>` внутри `<script>` внутри `<svg>` не
должен закрывать сам `<script>`).

**Побочная находка, тоже исправлена.** WPT-ассерты обоих файлов активно
используют `Node.TEXT_NODE`/`Node.COMMENT_NODE` и т.п. — эти
legacy-константы (DOM §4.4) не были заведены в шиме НИ РАЗУ (0 хитов
`TEXT_NODE`/`COMMENT_NODE` по всему `crates/js/src/shim/`), хотя
`Node.DOCUMENT_POSITION_*` рядом уже были. Без них `cdata-in-
integration-point-fragment.html` был красным ДАЖЕ ПОСЛЕ правильного
разбора — `Node.TEXT_NODE === undefined` заваливал `assert_equals`
независимо от парсера. Добавлены `ELEMENT_NODE`(1)…`NOTATION_NODE`(12)
на `Node` и `Node.prototype` тем же `forEach`-паттерном
(`web_api_shim_mid.js`).

Тесты: `crates/engine/html-parser/tests/fragment_parsing.rs` — 6 новых
(`mathml_text_integration_point_context_disallows_cdata`,
`svg_html_integration_point_context_disallows_cdata`,
`non_integration_point_svg_context_allows_cdata`,
`html_breakout_tag_exits_svg_opened_inside_fragment`,
`breakout_end_tag_exits_svg_with_no_matching_open_element`,
`bogus_end_tag_inside_svg_is_ignored`); `tokenizer.rs` — 3 новых/1
переписанный (CDATA allowed/disallowed/lowercase-marker). `cargo test
-p lumen-html-parser` — 465+5+15 зелёных. `cargo clippy --workspace
--all-targets --profile dev-release --features v8-backend -- -D
warnings` — чисто.

Живое измерение (`run_smoke.py`, полный streaming/network путь через
`.venv`): оба файла — было `Subtests passed 0/9`/`0/1`, стало `9/9`/
`1/1`; regression-набор (`unclosed-svg-script.html`,
`foreign_content_00*.html`, `Document/Element.getElementsByTagName-
foreign-*.html`) без изменений. `.ini`-заглушки обоих файлов удалены —
не нуждаются в отдельных expectations.

Не в этом срезе: полноценная XML-парсер-архитектура (сам GAP-XMLDOC
остаётся `planned`, это очередной срез, не закрытие); CDATA в потоковом
foreign content (push-режим); self-closing `<table>`/`<select>`/
`<button>` (по-прежнему 0 хитов в вендоренном корпусе, срезы 2/9/11).
`scripts/scoped-test.sh` — единственный красный
(`lumen-driver::cases::snapshot_cpu`, 55-text-rendering/57-canvas-2d/
32-list-markers/34-forms/45-multiple-backgrounds/51-scrollbar-
rendering/1000000-final) — тот же класс чужого CPU-эталонного дрейфа,
что уже подтверждён на чистом `main` срезами 12/13 (этот срез не
касается paint/layout/raster кода вовсе).

## GAP-XMLDOC срез 15 (2026-09-14): CDATA в потоковом (push-режим) foreign content (`p1-gap-xmldoc-srez15`)

Срез 14 взвёл `cdata_allowed` только в `run_pull` (`parse`/
`parse_fragment`) — реальный сетевой путь, `IncrementalTreeBuilder::feed`/
`feed_bytes` → `PushTokenizer`, никогда его не устанавливал, поэтому
`<![CDATA[` внутри `<svg>`/`<math>`, загруженных постранично, всегда шёл
bogus-comment веткой независимо от namespace — последний пункт из
"Не в этом срезе" среза 14 и из `docs/engine-gaps.md`.

**Фикс — два независимых места.**

1. **Взвод флага.** `PushTokenizer` получил персистентное поле
   `cdata_allowed` (тот же приём, что уже есть у `text_only` для RAWTEXT/
   RCDATA, срез 12) и второй элемент в возврате `on_token`:
   `feed_with_context`/`feed_bytes_with_context`/`end_with_context` теперь
   принимают `FnMut(Token) -> (bool, bool)` — `(cancel_text_only,
   cdata_allowed)`. `IncrementalTreeBuilder::apply_token_for_stream`
   пересчитывает оба сигнала после каждого токена (`current_context_
   forbids_text_only()` и `cdata_sections_allowed()`), в точности как
   `run_pull` делает для pull-режима. `tokenize()` взводит его на живом
   `Tokenizer` перед КАЖДЫМ `next()`, не только между chunk-ами.

2. **Безопасность chunk-boundary (реальный баг, не просто недостающая
   проводка).** `find_safe_split`'s Data-state ветка ищет только
   ПОСЛЕДНИЙ `<` в буфере — эвристика, которая молча ломается внутри
   CDATA-контента: `<`/`>` там данные, а не начало новой конструкции.
   `<svg><![CDATA[a<b]]>c` (весь `<svg>` и открытие CDATA-секции в одном
   chunk-е) — последний `<` в буфере это НЕ `<![CDATA[`, а тот, что внутри
   `a<b`; старая логика проверяла его как обычный тег, "случайно" находила
   `>` где-то дальше и признавала буфер безопасным раньше настоящего
   `]]>`, что дало временному под-токенизатору на этом усечённом slice
   ложный EOF внутри CDATA-секции (`consume_cdata_section` лениво
   финализирует контент на EOF — корректно для настоящего конца ввода,
   некорректно для конца временного slice). Добавлена
   `last_unterminated_cdata_start(bytes)` — ищет буквальный
   (case-sensitive) `<![CDATA[`, чей `]]>` ещё не пришёл, и если такой
   есть, split ставится на его позиции БЕЗ ЗАПУСКА generic-скана вообще
   (приоритетная ветка перед обычным "последний `<`"). Это же исключило
   вторую ложную зависимость: `is_tag_closed`'s CDATA-ветка первой
   версии фикса опиралась на `self.cdata_allowed` ДО этого chunk-а — не
   видит открывающий `<svg>` из ТОГО ЖЕ буфера — убрана, буквальный
   `<![CDATA[` теперь просто ВСЕГДА ждёт `]]>`, независимо от
   `cdata_allowed`: лишняя буферизация в случае, когда секция окажется
   bogus comment-ом, безопасна (тот же консервативный принцип, что и у
   остального `find_safe_split`).

Тесты: `push_tokenizer.rs` — 3 новых
(`cdata_inside_svg_opened_in_the_same_stream_matches_pull` — byte/2/3/5/
100-байтовые chunk-и против pull, `cdata_with_gt_before_terminator_
survives_chunk_split_right_after_gt` — регрессия именно на находку выше,
разрез сразу после `>` внутри контента, `cdata_disallowed_context_still_
becomes_bogus_comment_when_streamed`); `tree_builder.rs` —
`feed_streams_cdata_inside_svg_foreign_content` (`feed_bytes` на 1/2/5/
11/100-байтовых chunk-ах и `feed` на `&str`-чанках, сравнение `Document::
to_string()` с pull `parse()`). `cargo test -p lumen-html-parser` —
469+5+15 зелёных (было 465+5+15). `cargo clippy -p lumen-html-parser
--all-targets --profile dev-release -- -D warnings` — чисто.

`tree_builder.rs` пересекло собственный baseline (5121 → 5416,
`scripts/file-size-baseline.tsv` обновлён тем же коммитом, тот же
паттерн, что срезы 8/9). Не паяльный/layout/paint код — `scoped-test.sh`/
`dump_golden.py` не запускались руками (правило `docs/commands.md`:
полный гейт — один раз, внутри `/lumen-task-finish`).

Не в этом срезе (без изменений): self-closing `<table>`/`<select>`/
`<button>`; сама XML-парсер-архитектура.

## GAP-XMLDOC срез 36 (2026-09-16): `Namespace::Other` — закрытый enum больше не теряет произвольный namespace URI на элементах (`p1-gap-xmldoc-srez36`)

Взял задачу с первой строки `STATUS-P1.md` (`ROADMAP.md:896`). Пере-замерил
остаток срезов 30–35 (параметрические entity/внешний DTD/`SYSTEM`-entity) —
по-прежнему 0 файлов на корпусе, без изменений. Вместо этого нашёл и закрыл
ровно один пласт **настоящего** резолвера namespace, о котором срезы 11/35
писали «отдельная, более крупная подсистема»: живой, не 0-хитовый WPT-тест
(`html/webappapis/dynamic-markup-insertion/the-innerhtml-property/
innerhtml-and-xml-namespaces.svg`, testharness, не reftest) явно требует
`document.createElementNS`/элементный `.namespaceURI` для **произвольного**
URI, не только шести уже известных Lumen'у.

**Причина.** `lumen_dom::Namespace` (`crates/engine/dom/src/lib.rs:160`) был
закрытым enum — `Html`/`Svg`/`MathMl`/`Xml`/`XmlNs`/`XLink`/`None`, без
варианта для «URI, которого Lumen не знает». `_lumen_create_element_ns`
(`crates/js/src/v8_runtime/install/dom_core.rs`) на любом нераспознанном `ns`
молча откатывался на `Namespace::Html` (BUG-830 — «нет общего реестра
namespace», задокументированный, но не устранённый предыдущими срезами).
Живое подтверждение (`--mcp-port`, до правки):

```js
document.createElementNS('https://example.org/ns', 'widget').namespaceURI
// "http://www.w3.org/1999/xhtml"  — ждём "https://example.org/ns"
```

**Фикс.** Добавлен `Namespace::Other(String)` — enum больше не `Copy` (несёт
владеющую строку), только `Clone`; пересобраны все точки, где компилятор
терял неявный `Copy` (`tree_builder.rs` ×5 клонов namespace при чтении
текущего/контекстного, `dom_helpers.rs` ×2). Два новых метода на `Namespace`
сами по себе не добавляют нового поведения — они переносят уже существовавшую,
но раскиданную по вызывающим точкам логику «URI → variant»/«variant → URI»
в одно место:

- `Namespace::from_uri(Option<&str>) -> Namespace` — `None`/`""` → `None`;
  шесть известных URI → их variant; что угодно ещё → `Other(uri)` вместо
  молчаливого отката.
- `Namespace::uri(&self) -> Option<&str>` — обратное отображение,
  `Other(s)` отдаёт `s.as_str()`.

`_lumen_create_element_ns` теперь ровно `Namespace::from_uri(...)` вместо
ручной if-цепочки с Html-фоллбэком — заодно и `xml:`/`xmlns:`/`xlink:`
URI-константы для **элементов** (не только атрибутов, которые их уже
поддерживали с среза 10) перестали откатываться на Html, хотя измеренной
надобности в этом не было, только побочный эффект использования единой
функции. `dom_helpers::namespace_uri` (бэкенд `_lumen_get_namespace_uri`/
`_lumen_get_attr_namespace_uri`) сменил сигнатуру с `Namespace -> Option<&'static
str>` на `&Namespace -> Option<String>` — `Other`'s строка не `'static`.

**Сознательно не тронуто (JS-шим уже был готов, живым замером проверено, что
ничего не сломалось и ничего не нужно было чинить):** `_lumen_element_prototype_for`
(`web_api_shim_mid.js:3458`) уже отдавал `Element.prototype` для любого
`ns !== xhtml/svg/mathml` — произвольный namespace корректно даёт
`instanceof Element`, не `HTMLElement`/`HTMLUnknownElement`; `_lumen_qualified_tag_name`
уже не аплкейсит tag name для `ns !== xhtml` — `.tagName` на `Other`-элементе
уже сохранял регистр правильно. `resolve_attribute_namespace`
(`setAttributeNS`'s атрибутная половина BUG-830) не тронута — её откат на
`Html` для неизвестного `ns` документирован отдельно и имеет другой набор
вызывающих мест (весь обычный, ненамеспейсенный путь атрибутов держится на
этом же фоллбэке), трогать не входило в измеренный скоуп этого среза.

**Что осталось открытым и НЕ было закрыто, несмотря на анализ до конца.**
Живой разбор `innerhtml-and-xml-namespaces.svg` (вложенное `<svg
xmlns="…svg" xmlns:h="…xhtml"><foreignObject><h:body>…`) показал: даже с
`Namespace::Other` тест не пройдёт, потому что элементный namespace в Lumen
по-прежнему выводится из **модели HTML5 foreign-content** (наследование от
`node_namespace` текущего открытого элемента + integration-point/breakout
эвристики по имени тега, срезы 3–20), а не из настоящего XML Namespaces §6
резолвинга (`xmlns="…"`/`xmlns:foo="…"` — лексическая область видимости
деклараций по предкам, независимая от того, что это за тег). Для теста это
не одно и то же: `<g/>` внутри `<h:body>` (которое само форсируется в
Html-неймспейс по правилу «`html:`-префикс — брейкаут», срез 5) обязано по
XML-правилам оставаться в SVG-неймспейсе (унаследовав `xmlns="…svg"` с
корневого `<svg>`, который ничем не перекрыт) — а по текущей HTML-модели
Lumen наследует именно `Namespace::Html` от реального DOM-родителя `<h:body>`.
Это два структурно разных алгоритма, а не один пробел с недостающим кейсом:
нужен отдельный, независимый от `open_elements`/`node_namespace` стек
`(default_ns, prefix → uri)`, обновляемый по фактическим `xmlns`/`xmlns:*`
атрибутам каждого элемента (включая его собственные — `xmlns` на себе самом
меняет и собственный namespace элемента, не только потомков), а integration-
point/breakout-эвристики тег-по-тегу должны перестать быть единственным
источником для этой части. Попытка сделать это точечной правкой поверх
существующих 35 срезов рискует тихо сломать что-то из уже измеренного и
зелёного корпуса — следующему срезу нужен явный, отдельно спроектированный
резолвер, не заплатка.

Тесты: `crates/engine/dom/src/lib.rs::tests` — три новых
(`from_uri_maps_known_uris_to_their_dedicated_variant`,
`from_uri_preserves_an_unrecognized_uri_verbatim`,
`uri_is_the_inverse_of_from_uri_for_every_known_variant`); `crates/js/src/dom/
tests/v8_core/mod.rs::create_element_ns_arbitrary_namespace_round_trips_its_uri`
(живой V8-прогон: `namespaceURI`/`localName`/`tagName`/`instanceof`). `cargo
test -p lumen-dom` — 300/300. `cargo test -p lumen-html-parser` — 519/519
юнит + 8/8 (`xml_entity_expansion`) + 15/15 (`fragment_parsing`) интеграционных.
`cargo test -p lumen-js --lib --features v8-backend` — 3720/3720. `cargo
clippy --workspace --all-targets --profile dev-release --features v8-backend
-- -D warnings` — чисто.

Не в этом срезе: настоящий `xmlns`/`xmlns:*`-резолвер описан выше как
отдельная подсистема, не начат; `resolve_attribute_namespace`'s Html-фоллбэк
для setAttributeNS с неизвестным ns; параметрические entity/внешний
DTD/`SYSTEM`-entity — по-прежнему 0 на корпусе.

## GAP-XMLDOC срез 37 (2026-09-16): `setAttributeNS` сохраняет произвольный namespace-URI (`p1-gap-xmldoc-srez37`)

*(запись добавлена срезом 38 задним числом — срез 37 влился без записи в
этот файл; текст ниже реконструирован из коммита `1a8821c3b`, без повторного
запуска его гейта.)*

Атрибутная половина среза 36: `resolve_attribute_namespace` (`dom_helpers.rs`)
для нераспознанного непустого `ns` схлопывал его в `Namespace::Html` вместо
сохранения — та же проблема, что `_lumen_create_element_ns` имел до среза 36.
Непустой нераспознанный URI теперь проходит через `Namespace::from_uri` и
round-trip'ится верно (`getAttributeNS`/`.namespaceURI` на атрибуте); `null`/
пустой `ns` по-прежнему схлопывается в `Namespace::Html` (сознательное
отклонение от DOM §4.5, тот же трейдофф, что уже был у
`known_attribute_namespace`, не тронут этим срезом). Тесты: `crates/js/src/
dom/tests/v8_core/mod.rs` — 2 новых (по diffstat коммита).

Не в этом срезе (без изменений): настоящий `xmlns`/`xmlns:*`-резолвер;
параметрические entity/внешний DTD/`SYSTEM`-entity — по-прежнему 0 на корпусе.

## GAP-XMLDOC срез 38 (2026-09-16): `Node.lookupNamespaceURI`/`lookupPrefix`/`isDefaultNamespace` не существовали вовсе (`p1-gap-xmldoc-srez38`)

Взял задачу с первой строки `STATUS-P1.md` (`ROADMAP.md:896`). Пере-замерил
остаток (параметрические entity/внешний DTD/`SYSTEM`-entity — по-прежнему 0
файлов на корпусе, без изменений; настоящий `xmlns`-резолвер для парсинга —
по-прежнему не начат, единственный живой корпусный хит,
`innerhtml-and-xml-namespaces.svg`, требует отдельного стека деклараций в
`tree_builder.rs`, не заплатки, см. срез 36). Вместо парсерной стороны взял
соседний, но структурно отдельный пласт: три DOM-метода
(`Node.lookupNamespaceURI`/`.lookupPrefix`/`.isDefaultNamespace`, DOM §4.4
"locate a namespace"/"locate a prefix") — чистая query-API над уже
распарсенными атрибутами, не участвует в резолюции namespace во время
парсинга.

**Измерение.** Grep по `lookupPrefix`/`lookupNamespaceURI`/`isDefaultNamespace`
во всём `crates/js/src/` (Rust-биндинги и все `.js`-шимы) — 0 хитов: методы
не существовали вообще, ни в каком виде (падение как "не функция", не
неверный ответ). Живой корпусный хит: `dom/nodes/Node-lookupPrefix.xhtml`
(testharness, не reftest) — прямой тест `Node.lookupPrefix()` с вложенными
`xmlns:x`/`xmlns:s`/`xmlns:t` на разных уровнях предков.

**Фикс.** `_lumen_locate_namespace(node, prefix)`/`_lumen_locate_prefix(node, ns)`
(`web_api_shim_mid.js`) — рекурсивный обход по `nodeType`, зеркалящий DOM §4.4
буква в букву: Element проверяет свой `namespaceURI`/`prefix`, затем
`xmlns`/`xmlns:*`-атрибуты (через уже существующие `Attr.prefix`/`.localName`,
текстовое разбиение qualified-имени), затем рекурсия на `.parentElement`;
Document делегирует на `.documentElement`; DocumentType/DocumentFragment
всегда `null`; Attr делегирует на `.ownerElement`; всё прочее (Text/Comment/
ProcessingInstruction) — на `.parentElement`. `Node.prototype.lookupNamespaceURI`/
`.lookupPrefix`/`.isDefaultNamespace` — тонкие обёртки с нормализацией
аргумента (`''`/`null`/`undefined` → `null`). Поскольку `.prefix` на любой
живой обёртке всегда `null` (Lumen не выделяет префикс из имени тега — см.
`get prefix()`), шаг 1 алгоритма («если свой namespace не null и свой prefix
совпадает с искомым») срабатывает только для искомого `prefix === null` и в
этом случае просто возвращает уже резолвленный `namespaceURI` элемента — тот
же шорткат, на который идёт настоящий браузер с корректно распарсенным
namespace. Любой непустой искомый prefix всегда проваливается в
`xmlns:*`-атрибутный обход — это и есть то, что реально покрывает корпусный
тест.

DocumentFragment — отдельный случай: `_lumen_make_document_fragment` строит
плоский объектный литерал без `[[Prototype]]` (BUG-377 уже документирует это
для `baseURI`), так что `Node.prototype`'ные методы до него не доходят;
добавлены три собственные копии на самом литерале, делегирующие в те же
`_lumen_locate_namespace`/`_lumen_locate_prefix` (которые для `nodeType === 11`
и так сразу возвращают `null` по спеке — просто чтобы вызов не падал как
"не функция").

Тесты: `crates/js/src/dom/tests/v8_bug685_lookup_namespace_apis.rs` — 4 новых
(`lookup_namespace_uri_walks_ancestor_xmlns_attributes`,
`lookup_prefix_finds_the_nearest_ancestor_binding`,
`is_default_namespace_matches_the_elements_own_namespace`,
`document_and_document_fragment_delegate_or_stop`). `cargo test -p lumen-js
--lib --profile dev-release --features v8-backend` — 3731/3731 (было 3727).
`cargo clippy -p lumen-js --all-targets --profile dev-release --features
v8-backend -- -D warnings` — чисто.

Не в этом срезе: настоящий `xmlns`/`xmlns:*`-резолвер для самого парсинга
(отдельная, более крупная подсистема, срез 36); `innerhtml-and-xml-namespaces.svg`
продвинулось (его `lookupNamespaceURI`/`lookupPrefix`-сабтесты, если такие
есть, теперь реальны), но не закрыто целиком — тот файл структурно требует
именно парсерного резолвера, не только query-API; параметрические entity/
внешний DTD/`SYSTEM`-entity — по-прежнему 0 на корпусе.

## GAP-XMLDOC срез 39 (2026-09-16): XML Namespaces §6 «default namespace» — первая половина настоящего резолвера (`p1-gap-xmldoc-srez39`)

Взял задачу с первой строки `STATUS-P1.md` (`ROADMAP.md:896`). Срезы 36/38
оба остановились на одном и том же выводе: живой корпусный тест
(`html/webappapis/dynamic-markup-insertion/the-innerhtml-property/
innerhtml-and-xml-namespaces.svg`) требует не точечной правки, а «отдельного,
не зависящего от `open_elements`/`node_namespace` стека `(default_ns,
prefix → uri)`». Этот срез строит ПОЛОВИНУ этого стека — `default_ns` без
`prefix → uri` — и явно проводит границу, где именно кончается half-done и
начинается настоящий пробел.

**Измерение.** Первый же под-тест файла (`"prerequisites"`) уже требовал
default-namespace резолвинга без единого `innerHTML` вызова:
`<element xmlns=""/>` внутри `<svg>` обязан дать `namespaceURI === null`,
`<element xmlns="…arbitrary…"/>` — произвольный URI; ни то, ни другое не
проходило, потому что `resolve_element_name` до этого среза вообще не смотрел
на атрибуты создаваемого элемента — только на текущий namespace стека.
Более тонкий случай, тоже из «prerequisites»: `<g>` — прямой child
`<h:body>` (тот форсится в `Html` префиксным брейкаутом среза 5) — обязан
получить `SVG_NS`, унаследованный от настоящего `<svg xmlns="…">` предка
двумя уровнями выше, а не `Html` от `<h:body>`. Это доказывает то, что
срезы 36/38 формулировали абстрактно: собственный namespace элемента
(через префикс) и «текущий default namespace в области видимости» (для
резолвинга непрефиксованных потомков) — это два структурно разных понятия,
и HTML5 foreign-content эвристика (наследование `node_namespace` реального
родителя) путает их в один.

**Фикс.** `resolve_element_name` (`tree_builder.rs`) получил два новых
параметра (`attrs`, `had_prefix` — последний генерализует старый
`had_html_prefix` на все четыре ветки `apply_token`'s xml_mode
prefix-стриппинга, не только `html:`/`h:`/`xhtml:`) и два новых хука:

1. Непрефиксованный тег (`!had_prefix`, не литеральный `svg`/`math`) — своё
   собственное `xmlns="…"` атрибут побеждает БЕЗУСЛОВНО, до всякого обращения
   к `start_tag_namespace` — иначе элемент, генуинно вложенный в SVG/MathML
   (резолвится через существующую Svg/MathMl-ветку матча раньше, чем матч
   вообще доходит до чего-то нового), никогда не показал бы новый код своих
   атрибутов.
2. Иначе, финальная «otherwise HTML» ветка (единственная, которую трогает
   этот срез — `Svg`/`MathMl`-ветки и литеральные `svg`/`math` остаются
   ровно как их оставили срезы 3/6/8) консультируется с новым
   `default_namespace_override()`: ближайший `xmlns` вверх по
   `open_elements` (реальному стеку открытых элементов, не парсерной
   эвристике), с fallback на `FragmentContext::default_namespace` (новое
   поле) для fragment-парсинга.

Ключевой guard, без которого фикс тихо ломает integration points
(`<foreignObject><body>` обязан остаться `Html` несмотря на предка со
`SVG`-default): override применяется, только когда
`current_namespace()` (реальный/adjusted namespace ТЕКУЩЕГО узла) сам НЕ
foreign. Для `<h:body>`'s ребёнка `<g>` это условие истинно (`<h:body>` —
генуинно `Html`, форсинг префиксом уже случился и осел в самом узле); для
`<foreignObject>`'s ребёнка `<body>` — ложно (`<foreignObject>` генуинно
`Svg`, а `Html` для `<body>` — integration-point форсинг конкретно ЭТОГО
тега, не унаследованный default). Без этого различения проверка `!==
Html` слишком узкая (реально нужна `!is_foreign_namespace`, не `== Html`
— иначе вложенный `xmlns=""`-сброс, сам по себе `Namespace::None`, а не
`Html`/`Svg`/`MathMl`, ломал резолвинг СВОИХ ЖЕ детей: `resolve_content_namespace`
теперь может вернуть `None`/`Other` там, где раньше видел только `Html`/
`Svg`/`MathMl`, и «otherwise HTML» ветка обязана пропускать унаследованное
значение (`inherited`), а не жёстко `QualName::html(name)`).

Дешёвый общий случай (документ/фрагмент, ни один элемент которого никогда
не пишет `xmlns`) остаётся O(1): `saw_own_xmlns` — булев флаг, взводимый
один раз при первом встреченном `xmlns`-атрибуте где угодно в разборе —
гейтит сам обход `open_elements`, так что обычный глубокий HTML-документ
не платит O(depth) за проверку, которая для него никогда не найдёт
совпадения.

`FragmentContext` (используется `Element.innerHTML=`) получил поле
`default_namespace: Option<Namespace>`, вычисляемое в
`dom_helpers::parse_html_fragment_with_context` новым
`Document::nearest_xmlns_default` (реальный, живой обход предков ЦЕЛЕВОГО
документа, не парсерного фрагмента) — контекстный элемент сам никогда не
попадает в дерево фрагмента (см. доку структуры), так что без этого поля
`xmlns`, объявленный где-то выше него в реальном документе, был бы просто
невидим фрагментному парсеру, у которого свой собственный, отдельный
`Document`.

**Сознательно не в этом срезе (вторая половина резолвера — `prefix → uri`
не начата).** Три под-теста корпусного файла остаются непройденными,
все три требуют настоящего отслеживания ПРЕФИКСНЫХ биндингов, не только
default namespace:
* префиксованный элемент (`<h:template xmlns=''>`), объявляющий СВОЙ
  `xmlns`, обязан завести новую область видимости для НЕПРЕФИКСОВАННЫХ
  потомков несмотря на то, что его СОБСТВЕННЫЙ namespace резолвится через
  префикс, а не через это `xmlns` (этот срез специально пропускает
  own-attribute fast path для `had_prefix == true` — правильно для
  собственного namespace элемента, неполно для его роли в качестве
  scope-объявителя для потомков);
* `xmlns:h='…'` внутри фрагмента, переопределяющий, во что резолвится
  префикс `h:` НИЖЕ этой точки (жёсткие пары срезов 5/33/34/35 не читают
  `xmlns:*` атрибуты вообще);
* как следствие первых двух — WPT тест «declaring namespace with prefix
  inside of fragment parsed by innerHTML» падает.

Настоящий `Node.lookupPrefix`-style обход (срез 38) для ЭТОЙ работы не
переиспользуется: тот резолвит статически, по готовому дереву, постфактум;
этому срезу нужно то же самое, но ВО ВРЕМЯ построения дерева, до того как
следующий тег придёт резолвиться. Также без изменений: параметрические
entity/внешний DTD/`SYSTEM`-entity (0 на корпусе); `<h:body>`-специфичный
баг (не срез-39: буквальный `<body>` start tag внутри "in body" сливается
в реальный документный `<body>` через существующее правило "merge
attributes", не создавая элемент вовсе, и это правило не смотрит,
исполняется ли оно внутри integration point — отдельный, более узкий
дефект, найденный только потому, что регресс-тест сначала попытался
использовать `<h:body>` как в оригинальном корпусном файле и получил
пустое дерево).

Тесты: `crates/engine/html-parser/src/tree_builder.rs` — 4 новых
(`unprefixed_child_of_a_prefix_forced_element_still_inherits_the_real_svg_default`,
`integration_point_forced_html_is_not_reopened_by_an_ancestor_default_namespace`,
`own_xmlns_resets_and_redeclares_the_default_namespace_even_inside_foreign_content`,
`fragment_context_default_namespace_reaches_a_genuinely_unprefixed_child`);
`crates/js/src/dom/tests/v8_bug685_default_namespace_resolver.rs` — 2 новых,
живой V8-прогон через настоящий `Element.innerHTML=` (не только
in-crate парсер), подтверждающий и наследование через `FragmentContext::
default_namespace`, и сброс/переобъявление внутри содержимого самого
фрагмента. `cargo test -p lumen-html-parser --profile dev-release` —
523/523 юнит (было 519) + 5/5 + 15/15 + 8/8 интеграционных. `cargo test -p
lumen-dom --profile dev-release` — 300/300 (без изменений). `cargo test -p
lumen-js --lib --profile dev-release --features v8-backend` — 3733/3733
(было 3731). `cargo clippy -p lumen-dom -p lumen-html-parser -p lumen-js
--all-targets --profile dev-release --features v8-backend -- -D warnings`
— чисто.
