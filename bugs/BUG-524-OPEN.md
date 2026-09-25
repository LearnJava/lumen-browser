# BUG-524: CSS Scroll Anchoring (`overflow-anchor`) is entirely unimplemented
— property not parsed, no anchor-selection/adjustment logic anywhere in layout

**Статус:** OPEN (ДОРАБОТКА → CSS-SPECS.md)
**Дата:** 2026-08-03
**Компонент:** css-parser (property missing) + layout (no anchoring algorithm)
**Найден:** WPT-RUN-3 срез 24 (`ROADMAP.md`) — массовый прогон `css/css-scroll-anchoring`

## Механизм

`grep -rn "overflow-anchor\|overflow_anchor" crates/engine/css-parser/src/` —
zero hits: the property isn't parsed at all (any value, valid or not, is
silently accepted/ignored by the generic inline-style passthrough, i.e. the
[BUG-484](BUG-484-FIXED.md) pattern). `grep -rln "ScrollAnchor\|scroll.anchor"
crates/ -i` finds a single hit, a doc-comment in
`crates/engine/layout/src/style.rs:896` noting the engine has "no support for
`overflow-anchor`" — there is no anchor-node-selection algorithm, no
suppression-heuristic, no scroll-offset-adjustment-on-relayout logic anywhere
in `lumen-layout`. This is a whole CSS module absent, not a partial/buggy
implementation.

## Симптом

Every `css/css-scroll-anchoring` test that actually reaches the anchoring
behavior itself (as opposed to failing earlier on
[BUG-523](BUG-523-FIXED.md)'s async-scrollTop gap or
[BUG-525](BUG-525-FIXED.md)'s missing `document.scrollingElement`) would still
fail even with those two fixed: nothing in layout adjusts scroll position
when content shifts above the visible viewport, which is the entire premise
of the spec. Two direct `e.style['overflow-anchor'] = '...'` parsing
assertions also fail (`= "all"`/`= "auto none"` should be rejected as
invalid) — those are already covered by the generic BUG-484 pattern, not
listed as new.

Filed by track policy (same as BUG-507 `css-exclusions`/BUG-517
`css-rhythm`): a WPT category whose corresponding CSS module has zero
implementation gets one bug for "whole module absent", separate from the
narrower BUG-523/BUG-525 findings that happen to dominate the *raw* failure
count in this specific category's log.

## Фикс (не сделан)

Full CSS Scroll Anchoring L1 implementation: parse `overflow-anchor`
(`auto`/`none`), select a per-scroll-container anchor node on layout,
suppress adjustment on the heuristics the spec defines (position-change,
`overflow-anchor: none`, etc.), and apply the compensating scroll delta
during relayout. Sizeable layout feature — likely its own multi-slice task
once picked up, not a quick property-table addition.

**Срез P3 2026-09-07 (часть 1):** grammar/CSSOM-only slice landed —
`OverflowAnchor` (`auto`/`none`, `crates/engine/layout/src/style/values/misc.rs`),
`ComputedStyle::overflow_anchor` (non-inherited, initial `auto`), parsing in
`apply_decl_paint`, CSS-wide-keyword handling in `apply_css_wide_keyword`,
computed-value serialization in `computed_style_to_map`, and the
`setProperty` allow-list entry in `web_api_shim_mid.js`. No anchor-selection
algorithm yet — `overflow-anchor` has zero effect on scroll behavior. This
closes the two `= "all"`/`= "auto none"` parsing-rejection assertions
mentioned in Симптом above; the rest of the module (anchor node selection,
suppression heuristics, scroll-offset compensation on relayout) is still the
whole remaining scope.

## Ревизия P3 2026-09-25: переквалифицирован в ДОРАБОТКА → CSS-SPECS.md

Взят как следующий top-down пункт `STATUS-P3.md` (BUGS.md:61; строки выше —
DEBTOR-якоря ратчетов с остатком в доменах P1 и приостановленный пользователем
BUG-341). Оба условия теста ДОРАБОТКА (`docs/probe-method.md` §8) выполнены:

1. **Функциональности нет вовсе.** После среза 1 (грамматика + CSSOM)
   `grep -rn -i "scroll.anchor|anchor_node|overflow_anchor" crates --include=*.rs`
   вне `layout/src/style*` и `selector_query.rs` даёт ноль: значение
   `overflow-anchor` разбирается и сериализуется, но не читается ни одним
   потребителем — ни в layout, ни в shell-пути скролла.
2. **Объём — модель состояния, а не один член.** CSS Scroll Anchoring 1 требует
   хранить якорный узел на КАЖДЫЙ scroll-контейнер (включая вьюпорт), выбирать
   его обходом кандидатов с исключениями (`overflow-anchor: none`, абсолютные/
   fixed-боксы, полностью невидимые), вести эвристики подавления (изменение
   `position`/`top`/`transform`/размеров на цепочке якорь → контейнер) и
   применять компенсирующую дельту между layout и paint — то есть связывать
   результат релейаута со scroll-состоянием shell. Точечной правки в одном
   месте нет.

Это прямое продолжение уже записанного в разделе «Фикс (не сделан)» вывода
(«likely its own multi-slice task»). Заведено не в `ROADMAP.md`, а в
`CSS-SPECS.md` (строка `overflow-anchor`, 🟡) — тот же прецедент, что
BUG-491/492/495: P4 владеет этим файлом как очередью CSS-свойств. Статус
переведён в `OPEN (ДОРАБОТКА → CSS-SPECS.md)`; строка снята с `STATUS-P3.md`.
