# BUG-972: Scroll To Text Fragment (`:~:text=`) is not implemented at all

**Статус:** OPEN (ДОРАБОТКА → [STTF-1](../ROADMAP.md))
**Тип:** нереализованная функциональность, не дефект реализованного кода —
ведётся как задача `STTF-1` в [ROADMAP.md](../ROADMAP.md), P3 как баг не
берёт ([docs/probe-method.md §8](../docs/probe-method.md)).
**Компонент:** js/dom — нет ни URL fragment-directive парсинга, ни поиска по
тексту, ни `hidden="until-found"`/`beforematch`
**Найден:** P2, WPT-RUN-6 срез 56, живой пробой (`serve_wpt_like.py`,
`verify_slice56_gaps.py`)

## Симптом

`grep -rn "until-found\|beforematch\|fragment_directive\|FragmentDirective\|text_fragment" crates/`
(excluding `tests/wpt/`) finds **zero** implementation call sites:
`onbeforematch` exists only as a reflected global event-handler content
attribute (`web_api_shim_mid.js`'s generic `on*` list) — nothing ever
dispatches it — and there is no code anywhere that parses a URL's
`:~:text=...` fragment directive, searches the rendered text for a match, or
implements the "hidden until found" reveal algorithm for
`hidden="until-found"` (HTML LS §4.10.9 / WICG Scroll To Text Fragment /
CSS `content-visibility: hidden` interaction).

## Прямое измерение

`scroll-to-text-fragment/find-range-from-text-directive-no-reveal.html`
served unmodified through `serve_wpt_like.py` (dev-release, Linux, `main` =
`8a750386e`): `harness-complete status=2 tests=1 Text fragment with suffix
should only reveal the matching details element:2` (harness TIMEOUT, the
sole subtest stuck at TIMEOUT). The test sets `window.location.hash =
':~:text=abc,-def'` and `await`s a `toggle` event on a `<details>` element
that should open only because the text-fragment match lands inside it —
since nothing ever performs that match, the `toggle` never fires and the
`promise_test` hangs until the harness's own budget expires. Matches the
WPT-RUN-5 corpus TIMEOUT signature for this id.

## Масштаб

Not a point fix: implementing this needs (1) URL fragment-directive syntax
parsing (`:~:text=[prefix-,]textStart[,textEnd][,-suffix]`, percent-decoding,
multiple comma-separated directives), (2) a text-search algorithm over the
rendered DOM (word-boundary-aware, case-insensitive, crossing element
boundaries, skipping `display:none`), (3) `hidden="until-found"` as a new
DOM/CSS state distinct from plain `hidden` (must be revealable), (4) the
"reveal" algorithm — dispatch `beforematch` (cancelable), then remove the
`until-found` restriction, and for the closest ancestor `<details>` set
`.open = true` and fire `toggle`, (5) scrolling the matched range into view
and applying the `::target-text` highlight pseudo. Each is its own design
surface, not a one-line addition — meets both `docs/probe-method.md §8`
conditions for ДОРАБОТКА (absent wholesale, family-sized).

## Классификация WPT-RUN-6

Classified via `_exact_id_marker` in `timeout_audit.py`
(`scroll-to-text-fragment-missing`).
