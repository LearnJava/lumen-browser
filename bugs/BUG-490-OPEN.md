# BUG-490: `getComputedStyle(element, pseudoElt)` ignores the pseudo-element argument entirely

**Статус:** OPEN (ДОРАБОТКА → CSSOM-5)
**Тип:** доработка — функциональность (getComputedStyle для псевдоэлементов) отсутствует
целиком, а не сломана точечно; см. ревизию P3 2026-09-02 ниже
**Дата:** 2026-08-02
**Компонент:** js (`crates/js/src/dom.rs` — `WEB_API_SHIM`, `window.getComputedStyle`
at `dom.rs:12772-12802`, engine-agnostic, shared by both QuickJS and V8 per
`CLAUDE.md`)
**Найден:** WPT-RUN-3 срез 7 (`ROADMAP.md`) — массовый прогон `css/css-display`

## Механизм

`window.getComputedStyle = function(element, pseudoElt) { ... }`
(`dom.rs:12772`) resolves `nid` purely from `element.__nid__` — the
`pseudoElt` parameter is never read anywhere in the function body (Proxy
`get` trap or the no-Proxy fallback). The doc comment above it is explicit:
`// Pseudo-elements are not yet supported (ignored).` (`dom.rs:12771`).
`getComputedStyle(el, '::before')` therefore returns `el`'s own resolved
style, not `::before`'s.

Confirmed live via `--mcp-port` on a page with
`#t1::first-line { display: flex; font-size: 30px }` and no other style on
`#t1`: `getComputedStyle(el, '::first-line').display` → `"block"` (`el`'s
own default block display, i.e. the pseudo argument really is silently
dropped, exactly per the code comment) rather than `"inline"` (the correct
resolved value for the `::first-line` box per CSS Display L3 §placement).

**Open discrepancy, not yet resolved:** the actual wptrunner run of
`display-first-line-001.html`/`display-math-on-pseudo-elements-001.html`
observed the assertion failing with `got ""` (empty string), not `got
"block"` as the code path above predicts and as the live `--mcp-port` probe
reproduces. Both point at the same underlying gap (pseudo styling not
implemented), but the exact wrong-value shape differs between the headless
probe (local-file navigation) and the wptrunner-driven run (HTTP navigation
via the live window / `--bidi-port`) — worth a follow-up slice to pin down
whether that's a second, independent bug (e.g. a load-time race specific to
the live-window navigation path) or an artifact of the probe's local-file
navigation taking a different code path than HTTP navigation.

## Симптом

Any WPT assertion using the two-argument `getComputedStyle(el, pseudoElt)`
form to read a pseudo-element's resolved style fails — either with a wrong
(base-element) value or an empty string, depending on navigation path (see
above). Affects `::before`/`::after`/`::first-line`/`::first-letter`
lookups alike, since all four go through the same ignored parameter.

## Масштаб находки

3 files in this slice (`display-first-line-001.html`,
`display-first-letter-001.html`, `display-math-on-pseudo-elements-001.html`
— the latter's `::before`/`::after` checks). Will recur in any WPT category
that queries a pseudo-element's computed style via the standard two-argument
form — a common idiom for `::before`/`::after`/`::first-line`/
`::first-letter` conformance tests specifically (not pseudo-elements in
general — `::marker`, `::selection` etc. aren't queried this way as often).

## .ini

Committed `.ini` under `tests/wpt/metadata/css/css-display/` for the 3
attributed files.

## Ревизия P3 2026-09-02: переклассифицирован в ДОРАБОТКА → CSSOM-5

Investigated as the next top-down `STATUS-P3.md` item (BUGS.md:56, after
BUG-341 was skipped as user-paused and BUG-480/BUG-484 were skipped as
already-tagged ДОРАБОТКА). Traced the full path from `window.getComputedStyle`
(`crates/js/src/shim/web_api_shim_tail_b.js`, moved out of `dom.rs` by
SPLIT-JS3) down through the native binding
(`_lumen_get_computed_style`/`crates/js/src/v8_runtime/install/platform.rs::install_computed_styles`)
to where the underlying `computed_styles: HashMap<u32, HashMap<String,String>>`
snapshot is built (`lumen_layout::collect_computed_styles_rec`,
`crates/engine/layout/src/lib.rs`).

Two independent findings, both past the "does this need designing, not just
plumbing" line (`docs/probe-method.md` §8):

1. **No channel to any pseudo-element's own style exists.** The box tree
   already tags `::before`/`::after`/`::first-line` boxes with
   `origin.role == BoxRole::Pseudo(kind)` (`box_tree/types.rs`,
   `box_tree/pseudo_text.rs::split_first_line_boxes`), and both
   `computed_styles` and the separately-threaded `hit_test_tree` snapshot
   (`Arc<Mutex<Option<Arc<LayoutBox>>>>`, already pushed to the JS thread on
   every relayout for `elementFromPoint`) carry that box. A new native
   binding could walk `hit_test_tree` for `(host nid, pseudo kind)` and
   return that box's own `computed_style_to_map` — a contained, wireable fix
   for `::before`/`::after`/`::first-line`. But a non-floated `::first-letter`
   (`display-first-letter-001.html`'s `t3`/`t4`) never gets a
   `BoxRole::Pseudo` box at all: `extract_first_letter_float` only runs for
   the floated/`initial-letter` case; otherwise `apply_first_letter_pseudo`
   overrides `InlineSegment::style` in place, with no box to look up. There is
   nothing to read for that case, not just no reader for it.

2. **The property the vendored tests actually assert (`display`) is not "the
   resolved style" at all.** Per CSS Display L3 §placement / CSS
   Pseudo-Elements, `::first-letter`/`::first-line` restrict `display` to a
   value computed independently of the author's declared value: used value is
   `block` when the pseudo-element would be floated or positioned, `inline`
   otherwise (`::first-line` is always `inline` — it can never be floated as
   used). `display-first-letter-001.html` proves this directly: `#t1`
   (`float:left; display:flex`) and `#t2` (`float:left`, no display) both
   assert `"block"`, `#t3` (`display:flex`, not floated) and `#t4` (neither)
   both assert `"inline"` — the specified `flex` never survives in any case.
   `display-first-line-001.html` asserts `"inline"` for all four
   `float`/`display:flex` combinations. Reading back "whatever the box's
   `ComputedStyle::display` ended up being" (today's cascade output, which
   does try to honor the author's `display: flex`) would still be spec-wrong
   even with a working box lookup — this needs its own restricted-computed-value
   algorithm, not a lookup. `::before`/`::after` are unaffected by this
   restriction (`display-math-on-pseudo-elements-001.html` expects the
   author's `display: math`/`block math` to survive verbatim), so the
   algorithm is pseudo-kind-specific, not a blanket override.

Both conditions of the ДОРАБОТКА test (`docs/probe-method.md` §8) hold:
(1) the capability is absent outright, not broken — the code comment says so,
and finding 1 confirms there is genuinely nothing to wire for one of the four
pseudo-kinds; (2) the fix needs a new algorithm plus a per-pseudo storage/read
path threaded across the JS↔layout boundary, not a one-place patch. Filed as
[ROADMAP.md CSSOM-5](../ROADMAP.md) (owner P1, CSSOM track, after CSSOM-4).
Status flipped to `OPEN (ДОРАБОТКА → CSSOM-5)`; row moved out of `STATUS-P3.md`.
