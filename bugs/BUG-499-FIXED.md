# BUG-499: `getComputedStyle().getPropertyValue('--custom-prop')` always returns
`""` — custom properties are never serialised into the computed-style cache

**Статус:** FIXED (закрыто ревизией) 2026-09-03
**Дата:** 2026-08-02
**Компонент:** layout (`crates/engine/layout/src/selector_query.rs::computed_style_to_map`)
+ js (`crates/js/src/v8_runtime.rs::_lumen_get_computed_style`)
**Найден:** WPT-RUN-3 срез 10 (`ROADMAP.md`) — массовый прогон `css/css-variables`

## Механизм

`computed_style_to_map` (`selector_query.rs:625`) serialises ~68 well-known
CSS properties from `ComputedStyle`'s typed fields (`width`, `color`,
`filter`, …) into the `HashMap<String, String>` that
`_lumen_get_computed_style` (`v8_runtime.rs:3146`) later does a pure lookup
against. It never iterates `ComputedStyle::custom_props`
(`style.rs:3492`/`4542`, a `CustomProps` map holding every resolved
`--name: value` declaration) — there is no loop anywhere in the function
inserting a `"--…"`-prefixed key. Since `_lumen_get_computed_style` is a flat
`HashMap::get(&prop)`, any query for a custom property name — regardless of
whether it resolved successfully, is guaranteed-invalid, or was never
declared — returns `None` → `.unwrap_or_default()` → `""`, indistinguishably
from a genuinely-absent property. This is unconditional: it does not depend
on cache-freshness (contrast [BUG-493](BUG-493-OPEN.md)) or on the property
being registered via `@property` — even a `--var: value;` custom property
read back through `getComputedStyle()` on a fully-settled, statically-marked-up
element returns `""`.

Confirmed live (`--mcp-port`, page with `<div id="t1" style="--x: 20px; width:
var(--x);">`, read via a **separate** `eval()` call after `navigate()`
returned — i.e. the [BUG-493](BUG-493-OPEN.md) cache-timing gap is ruled
out):

```js
getComputedStyle(document.getElementById("t1")).getPropertyValue("width")  // → "20px" (correct — var() substitution genuinely works)
getComputedStyle(document.getElementById("t1")).getPropertyValue("--x")   // → ""     (wrong — should be "20px")
```

The inline-style path (`div.style.getPropertyValue('--x')`, not
`getComputedStyle`) is unaffected — `_lumen_make_style`'s `getPropertyValue`
(`dom.rs:4271`) reads straight off the parsed `style=` attribute string, a
different, working code path (see [BUG-484](BUG-484-OPEN.md) for that path's
*own*, unrelated gaps).

## Симптом

```
FAIL variable (Computed Style) -- assert_equals: Expected Value should match actual value expected "value" but got ""
FAIL --value is blue before animation runs -- assert_equals: expected "blue" but got ""
FAIL testing cascaded CSS Variables on div 't0' -- assert_equals: expected "x" but got ""
```

Every WPT test in `css/css-variables` that calls
`getComputedStyle(el).getPropertyValue('--name')` fails this way, regardless
of whether the underlying custom-property resolution (cascade, `var()`
chains, `@property` initial-values, cycle detection) is otherwise correct —
the value never reaches the JS side at all. A parallel effect: any test that
happens to *expect* `""` for a custom property (e.g. a guaranteed-invalid
case) passes by coincidence, not because the engine detected invalidity
(see `variables-substitute-guaranteed-invalid.html`'s note in
[BUG-384](BUG-384-FIXED.md)'s extension below — masked by a different bug in
this slice, but would pass-by-luck once unmasked).

## Масштаб находки

**WPT-RUN-3 срез 10 (`css/css-variables`, 2026-08-02)**: dominant or
contributing cause in at least 10 files — `variable-definition.html`
("(Computed Style)"/"(Cascading)" sections plus the CSSOM.setProperty
block), `variable-definition-cascading.html`, `variable-definition-keywords.html`,
`variable-substitution-variable-declaration.html` (the `--varN` subtests),
`variable-created-element.html`/`variable-created-document.html` (the `--c`
subtests), `variable-animation-from-to.html`/`-over-transition.html`/
`-to-only.html` (the `--value` "before" checks), plus masked-but-would-apply
in `variable-cycles.html` and `variables-substitute-guaranteed-invalid.html`
(both blocked earlier by [BUG-384](BUG-384-FIXED.md)). Not surveyed beyond
`css/css-variables` this slice, but the mechanism (a hand-written, fixed
property list with no custom-property loop) has zero dependency on the
category — any WPT test anywhere that reads a `--name` custom property back
through `getComputedStyle()` will hit this.

## .ini

Committed `.ini` under `tests/wpt/metadata/css/css-variables/` for every
file where this is the attributed (sole or contributing) cause — see file
list above; each `.ini` header cites BUG-499 alongside any co-attributed bug
for that file (BUG-493 for files that also read a standard, cache-timing-
sensitive property; BUG-384 for files masked by named-access-on-window).

## Срез 19 (`css/css-nesting`, 2026-08-03)

`nested-declarations-matching.html` — 11/11 subtests, the entire file — is a
CSS Nesting-specific stress test of `CSSNestedDeclarations`/specificity
behavior, but every assertion bottoms out in
`getComputedStyle(e).getPropertyValue('--x')` (or `--y`/`--z`/`--w`)
comparing against a literal `'PASS'`/`'FAIL'` string baked into the custom
property's value by the stylesheet author — a documented WPT idiom for this
spec (encode the expected pass/fail outcome as the property's own string
value) that happens to make this bug's `""` result indistinguishable from a
genuine `'FAIL'`. `.ini`:
`tests/wpt/metadata/css/css-nesting/nested-declarations-matching.html.ini`.

## Срез 25 (`css/css-properties-values-api`, 2026-08-03)

`registered-property-initial.html` (29 subtests) and `registered-property-computation.html`
(5 subtests): both call `getComputedStyle(target).getPropertyValue(name)`
directly on the registered custom property's own name (not a standard
property substituted via `var()`) — the exact mechanism this bug documents.
`.ini` under `tests/wpt/metadata/css/css-properties-values-api/` for both
files.

## Ревизия P3 2026-09-03: закрыто без правки кода

Заявленный дефект уже устранён — побочным эффектом [BUG-732](BUG-732-FIXED.md)
(FIXED 2026-08-10, «шесть базовых DOM/CSSOM-API отсутствуют в шиме»), который
явно перечисляет среди шести симптомов: «`getComputedStyle(el).getPropertyValue("--x")`
отдавал `""`». Custom properties публикуются отдельным снимком
(`collect_custom_properties` → `update_custom_properties`, натив
`_lumen_get_custom_property`), а шим (`web_api_shim_tail_b.js::_lumen_computed_property`)
роутит любое `--`-префиксное имя туда, а не в `_lumen_get_computed_style` — этот
баг диагностировал ровно последний код (до BUG-732 у роутинга не было), но не
переисследовал вопрос после того, как BUG-732 его закрыл заодно, под другим
номером.

Юнит-тест `dom::tests::v8_computedstyle::get_computed_style_custom_property`
уже кроет сценарий заявки и проходит на чистом `main` без правок
(`cargo test -p lumen-js --features v8-backend get_computed_style_custom` — 1/1 OK).
Живая проба (`--mcp-port`, `<div style="--x: 20px; width: var(--x);">`,
отдельный `eval()` после `navigate()`) подтверждает — причём снимок оказался
БОГАЧЕ, чем эта заявка требовала: не только листовое значение читается верно,
но и цепочка `var()`-в-`var()` между custom properties резолвится полностью
(`--a: var(--b); --b: 10px` → `getPropertyValue('--a')` = `"10px"`, не сырой
`"var(--b)"`):

```js
getComputedStyle(t1).getPropertyValue('width')  // → "20px"
getComputedStyle(t1).getPropertyValue('--x')    // → "20px" (была бы "" до BUG-732)
getComputedStyle(t2).getPropertyValue('--a')    // → "10px" (var-в-var тоже резолвится)
getComputedStyle(t1).getPropertyValue('--nope') // → ""     (необъявленная — по спеке)
```

Код не менялся, закрытие — устранение расхождения статуса. `.ini`-файлы,
перечисленные выше, не пересматривались — это отдельный вопрос живого WPT-прогона
(вне скоупа этой ревизии).
