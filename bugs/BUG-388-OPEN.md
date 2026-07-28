# BUG-388 — `apply_forced_colors_mode` не форсирует `scrollbar-color` и `font-variant-emoji`

**Статус:** OPEN
**Компонент:** layout (`crates/engine/layout/src/style.rs:7996` —
`apply_forced_colors_mode`)
**Найден:** P2, WPT-VENDOR-forced-colors-mode (2026-07-28), прогон
`run_report.py --root forced-colors-mode` (тесты `forced-colors-mode-54.html`,
`forced-colors-mode-60.html`)

## Симптом

```
forced-colors-mode-54.html: assert_equals: expected "auto" but got ""
  -- getComputedStyle(div).scrollbarColor должен стать "auto" в forced-colors mode

forced-colors-mode-60.html: expected "text" but got ""
  -- getComputedStyle(div).fontVariantEmoji должен стать "text" в forced-colors mode
```

Оба теста — feature-detection в чистом виде: свойство просто не форсируется,
компьютед-значение остаётся таким же, как без forced colors.

Контраст с тем же прогоном: `forced-colors-mode-27.html` (PASS 1/1,
`html`/`head` → `CanvasText`) и `forced-colors-mode-41.html` (PASS 9/9 —
`accent-color`/`background-color`/`border-*-color`/`caret-color`/`color`/
`outline-color` все верно форсируются) показывают, что forced-colors mode в
Lumen реализован не заглушкой, а по существу правильно для основного набора
цветовых свойств — пропущены именно эти два.

## Причина

`apply_forced_colors_mode` (`style.rs:7996-8130+`) явно форсирует `color`,
`background-color`, `border-*-color`, `outline-color`, `caret-color`,
`box-shadow`/`text-shadow` (→ none), фон-изображения — но не трогает
`style.scrollbar_color` (поле `crates/engine/layout/src/style.rs:3603`) и не
трогает `font-variant-emoji` вовсе. По CSS Color Adjustment L1
(`#forced-colors-properties`) оба входят в список свойств, которые UA обязан
форсировать: `scrollbar-color` → `auto` (браузер не даёт странице раскрашивать
скроллбар нестандартными цветами, когда система уже задаёт палитру), а
`font-variant-emoji` → `text` (чтобы эмодзи не рисовались цветной картинкой
поверх принудительно-монохромной страницы).

## Как чинить

Добавить в `apply_forced_colors_mode` (после блока `outline_color`/`caret_color`,
`style.rs:~8075-8082`):

```rust
if style.forced_color_adjust != ForcedColorAdjust::PreserveParentColor {
    style.scrollbar_color = Some((/* auto-эквивалент */));
    // font-variant-emoji: FontVariantEmoji::Text, если поле уже есть в ComputedStyle
}
```

(если `font-variant-emoji` как свойство ещё не реализовано в
`css-parser`/`ComputedStyle` вовсе — сначала завести его как обычную CSS-задачу
P4, затем форсировать здесь).

Регрессия проверяется без WPT: `getComputedStyle(el).scrollbarColor === 'auto'`
и `getComputedStyle(el).fontVariantEmoji === 'text'` под forced-colors, при
любом авторском значении.

## Связанные

* Не связан с BUG-384 (named access) — та же категория, но независимый дефект:
  здесь тест исполняется и падает по существу, а не рушится на `ReferenceError`.
