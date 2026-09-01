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
