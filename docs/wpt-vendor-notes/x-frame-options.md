# WPT vendor notes — `x-frame-options`

## Vendoring (`tests/wpt/VENDOR.md`)

Test category, added 2026-08-18 by the WPT-VENDOR backlog (`ROADMAP.md`
`WPT-VENDOR-x-frame-options`, `docs/wpt-status.md`), scope ⬜ (candidate —
tests the `X-Frame-Options` response header, which interacts with framing
and CSP `frame-ancestors`; no dedicated Lumen implementation surface, this
is a network/navigation-layer check).

Same pinned upstream commit `35be3b44`, `git sparse-checkout add
x-frame-options` at that commit, `LICENSE-WPT.md` copied from a sibling
category — 9 files (6 top-level `.html` tests + `META.yml`/`README.md` +
`support/` with `helper.sub.js` + 3 Python handlers). Cheap on every
predictor: 0 `name="variant"` hits, 0 `testdriver.js` hits, 0 `.https.`
files. Confirmed cheap in practice: 1 min 21 s wall-clock, single process.

### Run result

`run_report.py --all --root x-frame-options --recursive` (1 min 21 s,
single process): 6 glob ids, all 6 actually run by wptrunner —
**0/6 harness OK, 0/157 subtests**, a uniform TIMEOUT wall.

### Dominant (only) finding: reconfirms [BUG-480](../../bugs/BUG-480-OPEN.md)

Every one of the 157 subtests builds an `<iframe>` and drives it through
`support/helper.sub.js`'s `xfo_test()`: it either awaits a `message` event
posted by the framed page back to the parent (the "allowed" case — the
framed page loaded and ran its own script) or awaits the iframe's `load`
event and then asserts `iframe.contentDocument === null` (the "blocked"
case). Both paths require the framed document to run as a genuine nested
browsing context. Since `<iframe>` has none in Lumen (BUG-480:
`contentWindow`/`contentDocument` absent from the JS shim, no separate
`Document`/`Window` pair per frame), neither the child-to-parent
`postMessage` nor the iframe's own `load` event ever fires — every subtest
times out before the `X-Frame-Options`/CSP `frame-ancestors` logic under
test is even reached. No category-specific defect surfaces; the header
parsing/enforcement itself (`support/xfo.py`'s server-side echo, matched
against Lumen's actual header handling) is never exercised. No new
`BUG-NNN` filed — appended as a dedicated "WPT-VENDOR-x-frame-options"
section to BUG-480 instead.

## Прогон и находки (`docs/wpt-status.md`)

Скоуп ⬜ (кандидат) — тестирует заголовок `X-Frame-Options` и его
взаимодействие с CSP `frame-ancestors`, отдельной реализации в Lumen нет
(сетевой/навигационный слой). Вендорена целиком 2026-08-18 (коммит
`35be3b44`, `tests/wpt/x-frame-options/`, 9 файлов, 6 id, дёшево по всем
предикторам: 0 variant, 0 testdriver, 0 `.https.`).

`run_report.py --all --root x-frame-options --recursive` — 1 мин 21 с,
**0/6 harness OK, 0/157 сабтестов** — сплошная стена TIMEOUT. Единственная
причина — уже открытый [BUG-480](../../bugs/BUG-480-OPEN.md): каждый сабтест
строит `<iframe>` и ждёт либо `message` от вложенной страницы, либо
`load`-событие самого `<iframe>` — оба пути требуют настоящего вложенного
browsing context, которого у `<iframe>` в Lumen нет, поэтому логика
`X-Frame-Options`/`frame-ancestors` ни разу не была достигнута. Новых
номеров бага не заведено — добавлен раздел в BUG-480.
