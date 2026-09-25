# BUG-627: `IntersectionObserver`'s `root` option (explicit root element) and `scrollMargin` option are entirely ignored — every observer intersects against the viewport regardless of `options.root`

**Статус:** FIXED 2026-09-25 (P3)
**Компонент:** js (`crates/js/src/dom.rs:7217-7276` —
`_lumen_deliver_intersection_observers`)
**Найден:** P2, WPT-VENDOR-intersection-observer, 2026-08-05

## Симптом

Confirmed live (`--mcp-live-port`, `eval`): `new IntersectionObserver(cb,
{root: someElement}).root` returns `undefined` (also covered by BUG-628 —
no getter at all), but more importantly `_lumen_deliver_intersection_observers`
(`dom.rs:7217-7276`) never reads `obs._options.root` anywhere in its body —
`rootTop`/`rootLeft`/`rootRight`/`rootBottom` (`dom.rs:7226-7227`) are
computed unconditionally from `_lumen_get_viewport_size()`, i.e. the
observer always treats the top-level viewport as the intersection root,
even when constructed with an explicit scrollable-ancestor `root` element.
`options.scrollMargin` (a newer addition to the spec, expanding the
*target's* bounds before intersecting) is likewise never referenced
anywhere in the shim.

## Масштаб

Explains a large share of the category's remaining, non-BUG-628 failures
once `takeRecords()` availability stops masking them:

- `root-*.html` family (`root-margin-root-element.html`,
  `same-document-root.html`, `same-document-with-document-root.html`,
  `unclipped-root.html`, `root-is-table-with-overflow-scroll.html`, …):
  `rootBounds` in delivered entries always reflects the viewport, never
  the configured root element's bounds, and intersection ratios are
  computed against the wrong container entirely for nested-scroller
  cases.
- All `scroll-margin-*` / `*-scroll-margin.html`-style tests
  (14 failures with the signature `assert_equals:
  IntersectionObserverEntryCount expected 1 but got 0`) — `scrollMargin`
  silently has zero effect, so entries that should cross a threshold
  because of the expanded target bounds never do.
- `cross-document-root.html`, `explicit-root-different-document.html`:
  an explicit `root` in a different document should make the observer
  always report non-intersecting; current code can't distinguish this
  case since `root` is never inspected.

## Fix shape

`_lumen_deliver_intersection_observers` needs an `obs._options.root`
branch: when set, resolve the root element's own bounding rect (and, if
it is itself scrollable, its scrollport/clip rect — not just its border
box) via the same native binding used for `rootLeft`/`rootTop`/etc.
instead of `_lumen_get_viewport_size()`, and expand it by the parsed
`rootMargin`. Separately, `scrollMargin` (an array of 1-4 length values
in the same shorthand grammar as `rootMargin`) needs to expand `ex/ey/
ew/eh` (the *target's* rect, `dom.rs:7236`) before the intersection
computation, not the root. Both are independent of BUG-628/BUG-626 but
touch the same delivery function — worth doing together.

## Исправление (P3, 2026-09-25)

`crates/js/src/shim/web_api_shim_mid_b4.js` — `_lumen_deliver_intersection_observers`
переписан по §3.2.10 «run the update intersection observations steps»:

- `_io_root_info` — корень наблюдения и «root intersection rectangle» (§2.2):
  неявный корень и `root: document` → вьюпорт; `root`-элемент с content clip
  (overflow ≠ visible — ровно те узлы, у которых есть запись в
  `_lumen_get_scroll_state`) → его padding box, иначе border box; поверх —
  `rootMargin`, а у прокручиваемого корня ещё и `scrollMargin`. `%` в margin
  разрешается от ширины/высоты этого прямоугольника (ожидания
  `root-margin-root-element.html`).
- `_io_compute` — §3.2.7 «compute the intersection»: цепочка containing block
  цели (`_io_cb_chain`: absolute прыгает к ближайшему positioned-предку, fixed
  уходит во вьюпорт), каждый clip-предок до корня режет цель своим padding
  box + `scrollMargin`; ось с `overflow: visible` (рядом с `clip`, CSS
  Overflow 3 §3.1) не режет. Корень не на цепочке → не пересекается (шаг 7);
  корень в другом документе (`createHTMLDocument`, фасад `contentDocument`)
  → не пересекается и `rootBounds === null` (шаг 6).
- Пересечение edge-inclusive (касание — `isIntersecting: true` с нулевой
  площадью), запись ставится в очередь при смене индекса порога или
  `isIntersecting` (`lastIndex`/`lastIntersecting`, §3.2.2
  `previousThresholdIndex`), а не только при пересечении порога по ratio.
  Цель, потерявшая бокс, теперь получает запись «не пересекается» (раньше —
  молча пропускалась).
- Неявный корень без clip-предков не читает computed style вовсе — чтение
  `position` включает кэш стилей до конца жизни страницы (BUG-935 S44), а
  lazy-load/аналитика — самый частый случай.

Регрессия — `crates/js/src/dom/tests/v8_bug627_io_root.rs` (геометрия
задаётся через `update_layout_rects` + `update_scroll_states`).

Живой WPT (`run_report.py --all --root intersection-observer --recursive
--processes 6`, dev-release): 129/143 harness OK, сабтесты 124/383 →
162/383; `.ini`-baseline переписан `--update-expected` (22 файла стали
чистыми). Два сабтеста перешли PASS → FAIL и записаны в baseline:
`isIntersecting-change-events.html` «Set scrollTop=100 and check for one new
notification.» и `scroll-margin-dynamic.html` «Test scroll margin
intersection after scrolling». Раньше они проходили случайно (вьюпорт вместо
корня), теперь упираются в [BUG-1166](BUG-1166-OPEN.md):
`getBoundingClientRect` не учитывает прокрутку контейнера, так что
`root.scrollTop = 100` не двигает цель ни для скрипта, ни для наблюдателя.
