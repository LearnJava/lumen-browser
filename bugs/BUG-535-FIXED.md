# BUG-535: `ruby-position: alternate` has no layout effect — annotations never flip over/under across stacked `<rtc>`s

**Статус:** FIXED 2026-09-26 (P1, GAP-RUBYBOX-2)
**Тип:** ДОРАБОТКА — `lay_out_ruby`/`RubyBox` (over/under-стекинг, `ruby-align`, `ruby-merge`) существуют, но не вызываются ни из одного места конвейера; `alternate` в мёртвом коде не даёт наблюдаемого эффекта. Перенесено в [GAP-RUBYBOX](../ROADMAP.md).
**Дата:** 2026-08-03
**Компонент:** layout (`crates/engine/layout/src/*` — ruby box layout)
**Найден:** P2, WPT-RUN-3 срез 26 (`css/css-ruby`) — массовый прогон

## Симптом

`ruby-position-alternate.html` sets `ruby-position` to `alternate`/
`alternate over`/`over alternate`/`alternate under`/`under alternate` on a
`<ruby>` with three stacked `<rtc>` annotation containers, then asserts (via
`getBoundingClientRect()`-based geometry helpers `assert_rt_is_over`/
`assert_rt_is_under`, not `getComputedStyle()`) that successive `<rtc>`s
alternate sides: first annotation over the base, second under, third over
again (or the mirrored under/over/under sequence for the `*under*` values).
All 7 subtests fail — the three annotations render in a fixed position
regardless of the `alternate` keyword.

This is distinct from the already-covered `ruby-*` gaps in the same slice:

- [BUG-472](BUG-472-OPEN.md) (computed style map) does **not** explain this
  file — the assertions read box geometry, not `getComputedStyle()` strings,
  and `ruby-position-valid.html`/`ruby-position.html` (parsing/basic
  positioning) both already pass 100%, so `ruby-position` **is** parsed and
  **is** applied for the plain `over`/`under`/`inter-character` values.
- [BUG-484](BUG-484-FIXED.md) (inline style setter validation) does not apply
  either — these are valid values being *set* successfully, just not
  producing the spec'd layout.

Confirmed by direct layout inspection (`lumen --dump-layout` on a minimal
`<ruby>`+`<rtc>` page): stacked `<rtc>` annotations are laid out as plain
stacked blocks in source order, with no alternation logic keyed off the
`ruby-position` computed value or the annotation's position among sibling
`<rtc>`s.

## Что нужно

Implement the CSS Ruby Layout `alternate` keyword: when `ruby-position`
computes to `alternate` (optionally combined with `over`/`under` to set the
starting side), successive ruby annotation containers of a ruby-base must
alternate sides (over/under) instead of all rendering on the same side.

## .ini

Committed `.ini` under `tests/wpt/metadata/css/css-ruby/` for
`ruby-position-alternate.html`, `expected: FAIL` on all 7 subtests.

## Исправление (2026-09-26, GAP-RUBYBOX-2, ветка `p1-gap-rubybox2`)

- `RubyPosition` получил `AlternateOver` (initial по спеке) / `AlternateUnder` / `InterCharacter`,
  `RubyPosition::parse` принимает полную грамматику `[ alternate || [ over | under ] ] | inter-character`
  (`crates/engine/layout/src/ruby.rs`).
- `build_ruby_box` (`box_tree/build.rs`) делит `<ruby>` на сегменты: база на каждый `<rb>` или прогон
  свободного текста, уровень аннотаций на каждый `<rtc>` (со своим `ruby-position`) и на подряд идущие `<rt>`.
  Форма пишется в `BoxKind::Ruby { shape }`.
- `resolve_level_sides`: `alternate`-уровень встаёт напротив стороны предыдущего уровня, первый — на сторону
  своего ключевого слова; `lay_out_ruby_segments`/`compose_levels` стекуют уровни наружу от базы.
- Попутно: `<rt>` оборачивается shrink-to-fit группой (раньше — блок шириной в строку, аннотации соседних
  ruby налезали), плавающий/abspos `<rt>` остаётся в базе — WPT `ruby-overhang-*-no-overlap`, `rt-display-blockified`.
- `getBoundingClientRect()` у `<ruby>` — прямоугольник уровня баз (`ruby_base_rect`), как в браузерах:
  на нём держатся `assert_rt_is_over/under` теста.

Проверка: сценарий `ruby-position-alternate.html` (все 7 подтестов) воспроизведён страницей с теми же
проверками через `lumen --dump-layout` — 7/7 PASS, затем полный `run_report.py --root css/css-ruby`: подтесты 55 → 59 из 87, `ruby-position-alternate.html` без `.ini` проходит; тесты `box_tree::tests::ruby_pipeline::rtc_levels_*`,
`ruby_client_rect_is_base_level_only`. Остаток: `inter-character` без принудительного `vertical-rl` у аннотации.
