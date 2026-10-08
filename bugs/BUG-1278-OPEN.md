# BUG-1278 — `getComputedStyle()` не отдаёт свойства CSS UI: `caret-color`, `accent-color`, `outline-offset`, `outline`, `resize`, `user-select`, `appearance`, `field-sizing`

**Статус:** OPEN
**Заведён:** 2026-10-05 (P2, WPT-RUN-14 срез 3, `css/css-ui`)
**Область:** layout (`crates/engine/layout/src/selector_query.rs::computed_style_to_map`)

## Симптом

Проба (`--dump-layout`, элемент со всеми свойствами в `style`): `getComputedStyle(el).getPropertyValue(p)` возвращает
`""` для `caret-color`, `accent-color`, `outline-offset`, шорткода `outline`, `resize`, `user-select`, `appearance`,
`-webkit-appearance`, `field-sizing`; `p in getComputedStyle(el)` (camelCase) — `false`. Для `outline-width`,
`outline-color`, `outline-style`, `cursor`, `text-overflow`, `pointer-events` значения есть.

Раскладка эти свойства знает: в `--dump-layout` того же элемента `outline-offset=4.00 accent=#ff0000ff cursor=Pointer`.
Не хватает строк в рукописной карте `computed_style_to_map` (тот же механизм, что [BUG-472](BUG-472-OPEN.md) и
[BUG-1254](BUG-1254-FIXED.md) у flex-свойств).

## Как найдено

WPT-RUN-14 срез 3: 27 testharness-id `css/css-ui`, 152 упавших сабтеста с `"… doesn't seem to be supported in the
computed style"` или `but got ""` (`inheritance.html` — 23, `parsing/canonical-order-outline-sub-properties-001.html` —
26, `parsing/caret-color-computed.html` — 12, `caret-color-0NN.html`, `parsing/{outline-offset,resize,user-select,
field-sizing}-computed.html`, `webkit-appearance-*`, `appearance-initial-value-001.html`, `accent-color-computed.html`
(harness ERROR)).

## Зелёные тесты, которые держатся на этом дефекте

`animation/outline-offset-interpolation.html` (120/120), `caret-color-interpolation.html` (204/204),
`accent-color-interpolation.html` (204/204), `outline-offset-composition.html` (40/40),
`caret-color-composition.html` (20/20) проходят пусто: `interpolation-testcommon.js` сравнивает
`getComputedStyle` цели и эталона, и обе стороны — `""`. После правки эти 588 сабтестов, скорее всего,
покраснеют на интерполяции (как `outline-width`/`outline-color`, где значение есть и не анимируется —
[BUG-1234](BUG-1234-OPEN.md)). Это прогресс, а не регресс.

## Как проверить

`run_report.py --root css/css-ui --recursive` (или `run_corpus.py --prefixes css/css-ui`) — указанные файлы.
Сериализация шорткода `outline` — в каноническом порядке `<width> <style> <color>` (`canonical-order-outline-*`).
