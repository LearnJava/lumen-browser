# BUG-388 — `apply_forced_colors_mode` не форсирует `scrollbar-color` и `font-variant-emoji`

**Статус:** FIXED 2026-08-10
**Компонент:** layout (`crates/engine/layout/src/style.rs` —
`apply_forced_colors_mode`; `computed_style_to_map` в `selector_query.rs`),
css-parser (список распознаваемых свойств)
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

## Причина

Три независимых дефекта, наложившихся в один симптом:

1. **`apply_forced_colors_mode` не трогал ни одного из двух свойств.** По CSS
   Color Adjustment L1 §3.1 UA обязан форсировать `scrollbar-color` → `auto`
   («scrollbar-color computes to auto») и — «If font-variant-emoji computes to
   `normal` or `unicode`, UAs should force any emoji on the page to its
   monochrome variant … by forcing the computed value … to `text`». Явный
   `emoji` (автор просит цвет намеренно) и уже монохромный `text` остаются как
   есть — ровно этого требует `forced-colors-mode-60.html`.
2. **`font-variant-emoji` не существовал как свойство** — ни в
   `ComputedStyle`, ни в парсере, ни в списке распознаваемых имён
   `css-parser`. Форсировать было нечего.
3. **Ни `scrollbar-color`, ни `font-variant-emoji` не попадали в
   `computed_style_to_map`** — сериализатор, из которого `getComputedStyle()`
   читает значения. Это и давало литеральное `""`: даже **вне** forced-colors
   mode страница со `scrollbar-color: green red` получала от `getComputedStyle`
   пустую строку вместо пары цветов, а по умолчанию — вместо `auto`. Заявка
   называла причиной только п.1; п.3 — отдельный дефект того же симптома,
   живущий на всех страницах, а не только под forced colors.

## Фикс

* `FontVariantEmoji` (`normal | text | emoji | unicode`) + поле
  `ComputedStyle::font_variant_emoji`, наследуемое: initial, `compute_style`,
  inherit-all-хелпер, `merge_pseudo_inherited`, CSS-wide-keyword-ветка.
* Парсинг longhand-а; обе компоненты shorthand-а `font-variant`
  (`font-variant: small-caps unicode`); `font` сбрасывает emoji-компоненту в
  initial, как и все прочие longhand-ы `font-variant` (CSS Cascade L4 §3.1).
  Имя добавлено в список распознаваемых свойств `css-parser`.
* `apply_forced_colors_mode`: `scrollbar_color = None` (`None` **и есть**
  представление `auto` у этого поля) и `Normal|Unicode → Text`. Оба форсируются
  и при `forced-color-adjust: preserve-parent-color` — §3.2 выводит из-под
  форсирования только `color`; оба сохраняются при `forced-color-adjust: none`.
* `computed_style_to_map`: ключи `scrollbar-color` (`auto` / пара `rgb()`) и
  `font-variant-emoji`; `font-variant` теперь сериализуется склейкой обеих
  реализованных компонент, а не одной caps.

**Ограничение, зафиксированное в CSS-SPECS/CAPABILITIES:** `font-variant-emoji`
🟡 — на выбор глифа не влияет. Presentation selection (variation selectors
VS15/VS16, curated emoji-fallback в `femtovg_backend`) свойство не читает, так
что `font-variant-emoji: text` не сделает цветной эмодзи монохромным.
Реализовано ради вычисляемого значения, которого требует §3.1.

## Проверка

18 юнит-тестов (`lumen-layout`): форсирование обоих свойств, поведение под
`forced-color-adjust: none` / `preserve-parent-color`, парсинг всех keyword-ов,
наследование, обе ветки shorthand-ов, CSS-wide-keywords, сериализация в
`computed_style_to_map`.

Живая проба через `--mcp-live-port` (`getComputedStyle` после загрузки) на
странице со `scrollbar-color: green red; font-variant-emoji: unicode`:

```
scrollbarColor:    "rgb(0, 128, 0) rgb(255, 0, 0)"   (было "")
fontVariantEmoji:  "unicode"                          (было "")
```

## Почему WPT-тесты 54/60 при этом остались красными

Прогон `run_report.py --all --root forced-colors-mode --recursive` после фикса —
те же две ошибки. Обе упираются в барьеры **вне** этого бага, вскрытые той же
пробой:

* **[BUG-755](BUG-755-OPEN.md)** — forced-colors mode вообще нельзя включить в
  автоматическом прогоне: `A11yPrefs::open_in_memory()`, дефолт `false`, ни
  CLI-флага, ни BiDi/MCP-ручки. `forced_colors_active()` в прогоне всегда
  `false`, так что форсировать движку нечего.
* **[BUG-443](BUG-443-FIXED.md)/[BUG-555](BUG-555-OPEN.md)** — `getComputedStyle()`,
  вызванный из инлайнового `<script>` во время разбора (а тесты категории
  читают стиль именно так), детерминированно возвращает `""` для **любого**
  свойства, включая `color`/`display`. Подтверждено пробой: тот же элемент даёт
  `color: ""` в парс-тайме и `color: "rgb(255, 0, 0)"` после загрузки.

Из-за второго барьера «зелёные» `forced-colors-mode-27.html` (1/1) и
`-41.html` (9/9), на которые ссылалась заявка, зеленеют **вхолостую**: их
ассерты — `assert_equals(html_color, div_color)` и
`assert_not_equals(value, "rgb(0, 128, 0)")` — выполняются и на паре пустых
строк. Вывод заявки «основной набор цветов форсируется правильно» верен по коду
(юнит-тесты `forced_colors_*` в `style.rs`), но получен не из этих прогонов.

## Связанные

* Не связан с BUG-384 (named access) — та же категория, но независимый дефект.
* [BUG-472](BUG-472-OPEN.md) — общий корень п.3: `getComputedStyle` не резолвер,
  а lookup по заранее собранной карте, поэтому любое свойство вне
  `computed_style_to_map` читается как `""`.
