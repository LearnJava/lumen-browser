# BUG-496: `HTMLElement.dataset` (DOMStringMap) entirely unimplemented

**Статус:** OPEN
**Дата:** 2026-08-02
**Компонент:** js (`crates/js/src/dom.rs` — no `dataset` accessor anywhere
in `WEB_API_SHIM`)
**Найден:** WPT-RUN-3 срез 9 (`ROADMAP.md`) — массовый прогон `css/css-backgrounds`

## Механизм

`grep -n '"dataset"\|\.dataset\b' crates/js/src/dom.rs` returns **zero
matches**. `HTMLElement.prototype.dataset` (the `DOMStringMap` live view
over `data-*` attributes, spec: HTML §3.2.6.6) is not defined at all — not
a getter returning an empty object, not present as a property. Any script
that reads `element.dataset.foo` crashes with a `TypeError` before it can
do anything else.

## Симптом

Reproduced by the WPT test that surfaced this:

```js
let divs = document.querySelectorAll('div');
for (let div of divs) {
  // ...
  assert_equals(style.getPropertyValue(prop), div.dataset.expected);
  //                                                ^^^^^^^ TypeError here
}
```

`border-width-cssom.html` (`css/css-backgrounds`) sets `data-expected="1px"`
on three `<div>`s and reads it back via `div.dataset.expected` inside its
one `<script>` block, before any `test()` call. The script throws
synchronously (`Cannot read properties of undefined (reading 'expected')`
— `dataset` itself is `undefined`, not an object missing the `expected`
key), so **no test ever registers** and wptrunner reports the whole
harness as **TIMEOUT** with zero subtests, not a clean FAIL. Confirmed via
a dedicated re-run (`run_smoke.py`, `--processes=1`) to rule out the
srez-8-documented `--processes=N` interleaved-log gotcha — this is a real,
reproducible crash, not a parallel-run artifact.

Note the test's actual assertions (`border-*-width: thin/medium/thick` →
`1px`/`3px`/`5px`) are almost certainly correct per a source check of
`style.rs:22034-22036` — fixing this bug alone would very likely turn this
file green, no separate `border-width` keyword-resolution defect involved.

## Масштаб находки

Confirmed via source grep for the whole workspace (not scoped to one
category) — `dataset` is a very commonly used DOM API (any page/test using
`data-*` attributes touches it), so this is a broad, cross-cutting gap
similar in shape to [BUG-480](BUG-480-OPEN.md) (`<iframe>` browsing
context) or [BUG-482](BUG-482-OPEN.md) (`offsetParent`/`scrollingElement`)
— not yet measured beyond the one file that happened to surface it this
slice.

## .ini

Committed `.ini` for `border-width-cssom.html` under
`tests/wpt/metadata/css/css-backgrounds/` (`expected: TIMEOUT`, whole
harness — no subtests ever register).
