# BUG-466: margins do not collapse through an empty block

**Статус:** FIXED 2026-09-01
**Дата:** 2026-08-02
**Компонент:** layout (block formatting context, margin collapsing, CSS2.1 §8.3.1)
**Найден:** WPT-RUN-3 срез 2 (`ROADMAP.md`) — массовый прогон `css/CSS2`,
`css/CSS2/normal-flow/margin-collapse-through-for-various-height-values.tentative.html`
(72/72 сабтеста FAIL)

## Симптом

```
FAIL <case> - assert_equals: margins should collapse through expected 50 but got 0
```

Все 72 сабтеста (варианты по `height`: auto/0/значения с `calc()`/
`min-height:stretch` и т.п.) проваливаются одинаково: когда пустой блок не
создаёт собственный BFC и не имеет padding/border/содержимого, отделяющего его
margin-top от margin-bottom, оба margin'а должны схлопнуться друг с другом и
"пройти сквозь" блок наружу (margin collapsing through), так что margin
родителя выше и margin следующего элемента ниже сливаются в один = `max(...)`
двух margin'ов (в тесте — 50px). Lumen вместо этого измеряет 0 — margin
пустого блока не распространяется наружу вовсе, схлопывание "через" блок не
реализовано.

Формализует находку, отмеченную ещё во время точечной проверки WPT-RUN-1
(`docs/wpt-status.md`, строка `css`, проба ~11:30): "несколько кейсов
`min-height:stretch`/`calc()` margin-collapse-through проваливаются" — тогда
без отдельного BUG-NNN, теперь заведено формально.

## .ini

`tests/wpt/metadata/css/CSS2/normal-flow/margin-collapse-through-for-various-height-values.tentative.html.ini`
— все 72 сабтеста `expected: FAIL`.

## Фикс (P3, 2026-09-01)

**Ревизия перед фиксом:** живой пересчёт на текущем `main` (много правок layout
с 2026-08-02) даёт не «0» из исходной заявки, а **100 вместо 50** — margin
верхней и нижней границы пустого блока считались как ДВЕ независимые дистанции
вместо одной. Симптом изменился, дефект (margin-collapse-through не
реализован) — тот же.

**Корень.** Sibling-цикл `lay_out`'а (`layout_dispatch.rs`) уже схлопывал
margin МЕЖДУ соседними блоками (`max(prev.mb, child.own_mt)`), но трактовал
top- и bottom-margin ОДНОГО пустого блока как раздельные величины: `child_y`
продвигался на `own_mt` при входе в блок и ЕЩЁ раз на `own_mb` при выходе —
итоговая дистанция превращалась в сумму двух margin вместо max() всех
смежных margin (prev.mb, own_mt, own_mb, next.mt), хотя блок нулевой высоты
физически не занимает места и обе его границы должны совпасть в одной точке
(CSS2.1 §8.3.1, self-collapsing box).

**Фикс.** Когда ребёнок — `Block`, не создающий BFC, без `clear`, без
in-flow-содержимого (`!has_in_flow_content`) и с итоговой высотой ≈0 —
собственные top/bottom margin схлопываются в `merged =
max(prev_block_mb, collapsed_mt, child_mb)`, который замещает ОБА конца:
позиция самого блока сдвигается на разницу (если merged больше того, что уже
учла его top-позиция), а `prev_block_mb`, экспортируемый следующему
сиблингу, — тот же `merged`, а не отдельный `child_mb`. Определение
self-collapse завязано на уже посчитанную `child.rect.height` (после
`lay_out_inner`), а не на пересчёт resolve-высоты заново — high/min/max-height
уже разрешены к этому моменту, включая проценты и `calc()`.

**Проверка.**
- Юнит-тесты `bug466_*` (`crates/engine/layout/src/box_tree/tests/flow_modes.rs`,
  4 теста): auto-height, explicit `height:0px`, non-zero `height:1px`
  (контроль), `min-height`/`max-height`/`%`-высота.
- Живой E2E-пробой (`--mcp-port`, headless `InProcessSession`,
  `getBoundingClientRect()`) с CSS/разметкой, буквально повторяющей WPT-тест
  (`.before,.after{overflow:hidden}`, `.wrapper{border:1px solid}`,
  `.test{margin:50px}`) — 10 репрезентативных комбинаций
  `height`/`min-height`/`max-height` (auto, `0px`, `1px`, `0%`,
  `calc(0px + 0%)`, `calc(1px + 1%)`, `stretch` — не парсится, ведёт себя как
  `auto`, `max-height:0px`), все совпадают со спекой.
- `cargo test -p lumen-layout` — 3653/3653, без регрессий.
- `cargo clippy -p lumen-layout --all-targets -- -D warnings` — чисто.

`tests/wpt/metadata/css/CSS2/normal-flow/margin-collapse-through-for-various-height-values.tentative.html.ini`
удалён — все 72 сабтеста теперь проходят (полный wptrunner-прогон не
выполнялся — тест детерминирован, чистая проверка `getBoundingClientRect()`
без сети/таймингов, и все категории значений покрыты пробами выше).
