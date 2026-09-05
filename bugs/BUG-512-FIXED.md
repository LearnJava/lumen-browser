# BUG-512: `forced-color-adjust` CSS property not implemented at all

**Статус:** FIXED 2026-09-05 (дрейф трекера)
**Дата:** 2026-08-03
**Компонент:** css-parser + layout (`grep -rn "forced-color-adjust\|
forced_color_adjust" crates/` — zero hits anywhere in the workspace)
**Найден:** WPT-RUN-3 срез 21 (`ROADMAP.md`) — массовый прогон `css/css-forced-color-adjust`

## Механизм

`forced-color-adjust` (CSS Color Adjustment Module Level 1 /
Forced Colors Mode, `https://drafts.csswg.org/css-color-adjust-1/#forced`)
lets a page opt an element out of a user's forced-colors (high-contrast)
mode. Unlike [BUG-511](BUG-511-OPEN.md)'s `link-parameters`, this is a
real, shipped property (Chromium/Firefox/Safari all support it) — a
genuine engine gap, not a never-shipped-draft filing. The property is
entirely absent from `ComputedStyle` and the parser's known-property table.

## Симптом

```
FAIL Property forced-color-adjust has initial value auto
  assert_true: forced-color-adjust doesn't seem to be supported in the
  computed style expected true got false
FAIL Property forced-color-adjust value 'preserve-parent-color'
  assert_true: forced-color-adjust doesn't seem to be supported in the
  computed style expected true got false
```

## Масштаб находки

2 files / 5 subtests: `inheritance.html` (2), `parsing/forced-color-adjust-computed.html`
(3). `parsing/forced-color-adjust-invalid.html` (6 subtests) fails on the
separate, generic inline-`style`-setter gap ([BUG-484](BUG-484-OPEN.md) —
invalid values like `"auto auto"`/`"1"`/`"default"` are accepted instead of
rejected) rather than on this property specifically.
`parsing/forced-color-adjust-valid.html` (3/3) passes: valid values
round-trip through `element.style` even without real validation, per
BUG-484's mechanism.

Since forced-colors mode itself (media feature `forced-colors`, the
system-color palette swap) is a larger, separate subsystem not evidenced
anywhere in this crate either, this property gap likely sits on top of a
wider absence — not investigated further in this slice, scope was the
`css-forced-color-adjust` WPT category only.

## .ini

Committed `.ini` under `tests/wpt/metadata/css/css-forced-color-adjust/` for
`inheritance.html` and `parsing/forced-color-adjust-computed.html`,
`expected: FAIL` per subtest.

## Ревизия P3 2026-09-05 — уже реализовано, дрейф трекера

Заявка сама себя опровергает датами: `df2d10f33` («Добавить forced-color-adjust
в ComputedStyle [P4]», 2026-05-21) и `230388e36` («Реализовать Forced Colors
Mode (CSS Color Adjust L1 §3) end-to-end», 2026-07-04) — оба предки коммита
`58aab00c2`, которым сам этот WPT-прогон (WPT-RUN-3 срез 21) был выполнен
2026-08-03 (`git merge-base --is-ancestor 230388e36 58aab00c2` → да). То есть
`grep`, на котором основана заявка, был либо запущен не в той рабочей копии,
либо в чужом каталоге — свойство уже существовало на момент подачи.

Текущая ревизия кода (та же ветка main, без правок) подтверждает полную
реализацию:
- `ForcedColorAdjust` (`auto` / `none` / `preserve-parent-color`) —
  `crates/engine/layout/src/style/values/typography.rs`;
- поле `ComputedStyle::forced_color_adjust`, помечено **inherited**
  (`style/cascade.rs:242-243` — doc-комментарий на самом поле в
  `computed.rs:358`, гласящий «NOT inherited», устарел и поправлен этой
  ревизией, но на поведение не влиял: реальное значение наследования задаёт
  `cascade.rs`, а не комментарий);
- парсинг — `style/apply/paint.rs`, CSS-wide keyword (`inherit`/`initial`/
  `unset`/`revert`) — `style/apply/css_wide.rs`;
- computed-value путь (`getComputedStyle()`) — `selector_query.rs::computed_style_to_map`;
- применение при реальном Forced Colors Mode — `style/adjust.rs::apply_forced_colors_mode`;
- `forced-color-adjust` в `SUPPORTED_PROPERTIES` (`css-parser/src/lib.rs`) —
  `CSS.supports()`-гейт не блокирует.

Пять существующих юнит-тестов (`style/tests/color.rs::forced_color_adjust_*`)
точно зеркалят пять сабтестов заявки — initial value `auto`, три валидных
значения (`auto`/`none`/`preserve-parent-color`), наследование
(`forced_color_adjust_inherited`, div `none` → потомок span тоже `none`):

```
cargo test -p lumen-layout --lib forced_color_adjust
running 5 tests
test style::tests::color::forced_color_adjust_initial_auto ... ok
test style::tests::color::forced_color_adjust_none ... ok
test style::tests::color::forced_color_adjust_preserve_parent_color ... ok
test style::tests::color::forced_color_adjust_invalid_ignored ... ok
test style::tests::color::forced_color_adjust_inherited ... ok
```

Точечного P3-дефекта не найдено. `parsing/forced-color-adjust-invalid.html`
(6 сабтестов) по-прежнему числится за [BUG-484](BUG-484-OPEN.md) (generic
inline-`style`-setter не валидирует значения) — не этим багом, не тронут.

`.ini` `inheritance.html`/`parsing/forced-color-adjust-computed.html` удалены
целиком — оба файла несли только `expected: FAIL` для уже проходящего
поведения.
