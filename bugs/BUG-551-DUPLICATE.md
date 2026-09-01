# BUG-551: `Element.prototype.getClientRects()` missing entirely — only `getBoundingClientRect()` exists on elements

**Статус:** DUPLICATE → [BUG-478](BUG-478-OPEN.md)
**Тип:** дубликат [BUG-478](BUG-478-OPEN.md) — тот же отсутствующий `Element.prototype.getClientRects()`, заведённый другим срезом. Выживает первый по дате (BUG-478, 2026-08-02); уникальные замеры этой записи перенесены туда. Слит 2026-09-02 ре-триажем пула WPT-RUN-5/6.
**Дата:** 2026-08-03
**Компонент:** js (`crates/js/src/dom.rs` — element wrapper factory; `getClientRects` exists
only on `Range` objects, `dom.rs:6944`, and on the `_CaretPosition` stub, `dom.rs:10516`)
**Найден:** WPT-RUN-3 срез 33 (`ROADMAP.md`) — массовый прогон `css/css-sizing`

## Механизм

CSSOM View §Extensions to the `Element` interface defines both
`getBoundingClientRect()` (single rect) and `getClientRects()` (a
`DOMRectList` — one rect per CSS fragment/line box, so an inline element
split across lines returns multiple rects). Lumen's element wrapper factory
only implements the singular form. Grepping the whole engine for
`getClientRects`:

```
crates/js/src/dom.rs:6944:  getClientRects: function() { return [this.getBoundingClientRect()]; },
crates/js/src/dom.rs:10516: _CaretPosition.prototype.getClientRects = function() { return []; };
```

Both hits are on unrelated objects (`Range`, `CaretPosition`) — `Element`
itself has no `getClientRects` member at all, so `el.getClientRects()`
throws `TypeError: target.getClientRects is not a function`.

## Симптом

`css/css-sizing/contain-intrinsic-size/auto-010.html` ("Last remembered
size... takes all fragments into account", a multi-fragment/columns test)
calls `target.getClientRects()` to enumerate per-column fragments —
`promise_test: Unhandled rejection with value: object "TypeError:
target.getClientRects is not a function"`, no other assertion in the file
ever runs. 1 file / 2 subtests this slice (`Last remembered size supports multiple
fragments`, `Last remembered size is updated when 2nd fragment changes
size`).

## Что нужно

Add `getClientRects` to the `Element` wrapper alongside
`getBoundingClientRect`, backed by whatever per-fragment box list the layout
engine already tracks for inline/multicol fragmentation (if none is tracked
yet, a same-content single-fragment fallback returning `[this.
getBoundingClientRect()]` — the same trivial shape already used for `Range`
— would at least stop the `TypeError` and let the file reach real
assertions, though it wouldn't produce spec-correct multi-fragment results
for `column`-fragmented or multi-line inline content).

## .ini

Committed `.ini` for `contain-intrinsic-size/auto-010.html`'s two subtests,
`tests/wpt/metadata/css/css-sizing/contain-intrinsic-size/auto-010.html.ini`,
`expected: FAIL`.
