# BUG-1445 — Вложенные правила, где `&` не стоит первым (`.t &`, `div &`, `.r&`, `:not(&)`), и вложенный голый тип (`.p { div {…} }`) отбрасываются; `&` на верхнем уровне и в `@supports selector(&)` не работают

**Статус:** FIXED 2026-10-21 (P3)
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 20, `css/selectors` + `css-pseudo` + `css-nesting` + `css-namespaces` + `css-cascade`)
**Область:** css-parser/layout (вложенные правила: `crates/engine/css-parser/src/parser/` — разбор nested rule; `&` вне первого компаунда)

## Симптом

Правила, у которых `&` идёт не в начале, не применяются: `.q { .t & { … } }`, `.q { div & {…} }`, `.q { :is(.x) & {…} }`, `.q { .t > & {…} }`, `.q { & + & {…} }`, `.q { .r& {…} }`, `.q { :not(&) {…} }`. Вложенный селектор, начинающийся с ТИПА (`.p { div { … } }`, `:root { div { … } }`), тоже отбрасывается, а `.p { & div {…} }` работает. `CSS.supports('selector(&)')` и `@supports selector(&)` ложны (см. BUG-1441); верхнеуровневый `&` (должен вести себя как `:scope`, т.е. корень) не проверялся. Работают: `& .x`, `&.r`, `.x`, `#id`, `:is(...)`, `[attr]`, `&::before`, вложенные `@media` и `@supports (…)`. Последствие — 24 id среза (`css-nesting/*`: 28 из 36 сабтестов, 12 reftest), в том числе `nesting-type-selector`, `nesting-basic`, `implicit-nesting`, `supports-rule`, `nest-containing-forgiving`, `contextually-invalid-selectors-001…003`; кластер собран по каталогу `css-nesting/`, пробой подтверждены только строки таблицы.

## Проба

`getComputedStyle(f).color`, ожидается `rgb(0, 128, 0)`:

| правило | `f` | у нас |
|---|---|---|
| `.q{ .x{color:green} }` | `<div class=q><i class=x id=f>` | зелёный |
| `.q{ & .x{…} }` | то же | зелёный |
| `.q{ &.r{…} }` | `<div class="q r" id=f>` | зелёный |
| `.q{ i{…} }` | `<div class=q><i id=f>` | **чёрный** |
| `.q{ & i{…} }` | то же | зелёный |
| `:root{ div{…} }` | `<div>` | **не применяется** |
| `.q{ .t &{…} }` | `<div class=t><div class=q id=f>` | **чёрный** |
| `.q{ div &{…} }`, `.q{ :is(.x) &{…} }`, `.q{ .t > &{…} }` | аналогично | **чёрный** |
| `.q{ .r&{…} }` | `<div class="q r" id=f>` | **чёрный** |
| `.q{ :not(&){…} }` | `<div class=q><b id=f>` | **чёрный** |
| `.q{ @supports selector(&){color:green} }` | `<div class=q id=f>` | **чёрный** |

## Как найдено

WPT-RUN-14 срез 20: `css-nesting/nesting-type-selector.html`, `nesting-basic.html`, `supports-rule.html`, `contextually-invalid-selectors-001.html` (+ `002`, `003`).

## Что делать

Вложенный селектор без `&` в начале разбирать как `& <селектор>` (Nesting 1 §3.1: неявный `&` + потомок), независимо от того, с чего он начинается — типа, класса или идентификатора. `&` в любом компаунде цепочки подставляет родителя как `:is(<родитель>)`. `&` на верхнем уровне — `:scope`. `CSS.supports('selector(&)')` → `true`.

## Как проверить

Таблица выше; `css/css-nesting/nesting-type-selector.html`, `css/css-nesting/nesting-basic.html`.

## Исправление

К моменту взятия бóльшая часть таблицы уже работала (`nesting_amp.rs`: `&` не в голове prelude, вложенный голый тип). Оставались четыре случая, все закрыты 2026-10-21:

- `& + &`, `&.a &` — второй `&` в цепочке после ведущего `&` не разбирался (`parse_nested_rule_amp` ждал список селекторов). Теперь prelude с двумя и более `&` идёт общим путём `parse_nested_rule_with_extra_amp` → `parse_nested_rule_with_inner_amp` (`crates/engine/css-parser/src/parser/nesting_amp.rs`): `&` подставляется как `:is(<родитель>)` по всей цепочке.
- `&` на верхнем уровне и в `selector(&)` — `parse_simple_selector` (`parser/selectors.rs`) теперь принимает `&` как `:where(:scope)` (нулевая специфичность, Nesting L1 §3.2); раньше правило отбрасывалось, `@supports selector(&)` и `CSS.supports('selector(&)')` были ложны (и вложенный `@supports selector(&)`).
- Проверено на собранном бинаре (`--dump-display-list` по таблице проб: все одиннадцать строк зелёные, `& + &` красит только второго брата), `css/css-nesting` 102→103 из 117 сабтестов (`top-level-parent-pseudo-specificity` проходит; `.ini` обновлены).

Тесты: `parser/tests/nesting.rs` (`nested_second_amp_in_tail_is_substituted`, `nested_amp_not_at_head_and_leading_combinator`, `top_level_amp_is_scope`, `supports_selector_amp_is_supported`).
