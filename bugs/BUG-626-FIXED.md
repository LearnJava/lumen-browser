# BUG-626: `IntersectionObserver` constructor and `observe()` perform no argument validation — invalid `threshold`/`rootMargin` are silently accepted, `observe(nonElement)` silently no-ops

**Статус:** FIXED 2026-09-25 (P3)
**Компонент:** js (`crates/js/src/dom.rs:7180-7193` — `IntersectionObserver`
constructor and `.prototype.observe`)
**Найден:** P2, WPT-VENDOR-intersection-observer, 2026-08-05

## Симптом

Confirmed live (`--mcp-live-port`, `eval`) — all four should throw per spec,
none do:

```js
new IntersectionObserver(function(){}, {threshold: [1.1]});     // should throw RangeError (out of [0,1])
new IntersectionObserver(function(){}, {threshold: ["foo"]});   // should throw TypeError (not a double)
new IntersectionObserver(function(){}, {rootMargin: "2em"});    // should throw SyntaxError (only px/% allowed)
var o = new IntersectionObserver(function(){}, {});
o.observe("foo");                                                // should throw TypeError (not an Element)
```

`dom.rs:7180-7185` stores `options` verbatim with no validation at all.
`dom.rs:7186-7193` (`observe`) checks `target.__nid__ === undefined` and
silently `return`s instead of throwing — the WPT test's own comment
(`observer-exceptions.html`) documents the spec-required
`TypeError`/`RangeError`/`SyntaxError` outcomes for each case.

## Масштаб

Reproduced by `intersection-observer/observer-exceptions.html`: all 9
subtests FAIL (`assert_throws_js`/`assert_throws_dom` all report "no
exception thrown" instead of the constructor/`observe()` call actually
throwing). Independent of BUG-628/BUG-627 — this is a missing-validation
gap, not a missing-getter or missing-root-support gap, though it lives in
the same constructor/`observe()` code.

## Fix shape

- Constructor: validate `options.threshold` — if array, every element must
  be a finite number in `[0, 1]` (else `RangeError`) and non-numeric
  entries must throw `TypeError` before the `RangeError` check (per WPT's
  ordering, non-numeric first).
- Validate `options.rootMargin` against the CSS `<length-percentage>`
  grammar restricted to `px`/`%` units, exactly 1-4 components, no
  `calc()`/`!important` — else `DOMException("SYNTAX_ERR")`.
- `observe(target)`: throw `TypeError` when `target` is not an `Element`
  (i.e. not `target instanceof Element` in spec terms — here, no
  `__nid__`/not an element-shaped object) instead of the current silent
  `return`.

## Исправление (P3, 2026-09-25)

`crates/js/src/shim/web_api_shim_mid_b4.js` — `IntersectionObserver`:

- Порядок проверок как у WebIDL + §2.2: сначала конверсии аргументов
  (callback не функция → `TypeError`; `options` не объект → `TypeError`;
  `root` не Element/Document/null → `TypeError`; `threshold` —
  `(double or sequence<double>)`, итерируемое раскрывается в список, любое
  неконечное значение → `TypeError`), затем шаги конструктора: `rootMargin` и
  `scrollMargin` через «parse a margin» (1–4 токена px/%) → `SyntaxError`
  `DOMException`, порог вне [0, 1] → `RangeError`. Поэтому
  `threshold: [2, "foo"]` даёт `TypeError`, а не `RangeError`.
- Невалидный margin больше не подменяется молча на `0px` (заглушка BUG-628).
- `observe(target)`: не-объект, объект без `__nid__` или узел с `nodeType` ≠ 1
  → `TypeError`. Прокси `{__nid__}` ленивых картинок
  (`_lumen_init_lazy_images`) не несёт `nodeType` и по-прежнему принимается.
- `crates/js/src/frame_bridge.rs` `docFacade`: фасад `contentDocument`
  чужого фрейма не имеет `nodeType`, и `root: iframe.contentDocument` начал бы
  бросать `TypeError` (так упали `document-scrolling-element-root.html` и
  `iframe-root-with-overflow-propagation.html` в первом прогоне). Фасад
  получил неперечислимый `__bid__` — тот же маркер, что у фасадов элементов, —
  и конструктор принимает его как Document.

Регрессия — `crates/js/src/dom/tests/v8_bug626_io_validation.rs` (4 теста:
порог, margin, callback/root, observe).

Живой WPT (`run_report.py --all --root intersection-observer --recursive
--processes 6 --check`, dev-release): 129/143 harness OK, сабтесты
114/381 → 124/381, 0 регрессий. `observer-exceptions.html` 9/9 — `.ini`
удалён; в `idlharness.window.js.ini` снято ожидание FAIL для
«calling observe(Element) … with too few arguments must throw TypeError».
