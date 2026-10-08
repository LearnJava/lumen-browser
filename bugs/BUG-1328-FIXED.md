# BUG-1328 — `text-transform: full-width`, `full-size-kana`, `math-auto` не разбираются (значение отвергается)

**Статус:** FIXED 2026-10-08 (P6)
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 10, `css/css-text`, вторая половина)
**Область:** layout (`crates/engine/layout/src/style/apply/text.rs` — разбор `text-transform`; enum `TextTransform` и `TextTransform::apply`, `style/values/typography.rs`)

## Симптом

Проба (`run_report.py --all`, временный `test()`): `el.style.textTransform = 'full-width'` → `style.textTransform === ''` и `getComputedStyle().textTransform === 'none'`; то же для `full-size-kana`, `math-auto`. `uppercase`/`lowercase`/`capitalize` работают. `<span style="text-transform:full-width">abc</span>` в `--dump-layout` — `abc`, не `ａｂｃ`.

## Как найдено

WPT-RUN-14 срез 10, `css/css-text/text-transform/`: `text-transform-full-size-kana-009.html` (58 сабтестов, `expected "あ" but got ""`), `math/text-transform-math-auto-003.html` (112 сабтестов, `expected "𝐴" but got "A"`), `-002.html`, `text-transform-fullwidth-001…009` — 11 id / 170 сабтестов (два из них, `-006`/`-008`, с локальным Ahem pixel-identical — BUG-1273). `fullwidth-007` (`white-space: pre-wrap`) дополнительно требует U+0020 → U+3000 внутри сохранённых пробелов (см. BUG-1327).

## Что делать

Добавить значения CSS Text L3 §2.1 (`full-width`, `full-size-kana`) и L4 (`math-auto`) по грамматике `none | [capitalize | uppercase | lowercase] || full-width || full-size-kana`. В `apply`: ASCII → U+FF01…FF5E (пробел → U+3000, в том числе в сохранённых пробелах), малая кана → полноразмерная, ASCII-буквы → Mathematical Italic для `math-auto` (единственный символ внутри `<mi>`).

## Как проверить

`css/css-text/text-transform/text-transform-full-size-kana-009.html`, `text-transform-fullwidth-001.html`.

## Исправление

Разбор значений уже был (`TextTransformExtra::parse`, CSSOM — BUG-1325); не хватало самого преобразования текста.

- Новый модуль `style/values/text_transform_map.rs` — таблицы `full-width` (Unicode `<wide>`/`<narrow>`: ASCII и U+0020 → U+FF01…FF5E/U+3000, полуширинные катакана/хангыль/символы), `full-size-kana` (приложение G CSS Text L3), `math-auto` (italic mappings MathML Core); бинарный поиск.
- `TextTransformExtra::apply(case, s)`: регистр, затем `full-width` и `full-size-kana`; `math-auto` — только для текста из одного символа (приближение «содержимое `<mi>`»).
- `inline_build.rs`: все четыре точки преобразования зовут `apply`; ветка `white-space: pre/pre-wrap/break-spaces` **вообще не применяла `text-transform`** (даже `uppercase`) — теперь применяет.

WPT: reftest'ы `fullwidth-002/010`, `full-size-kana-001`, `math/math-auto-001/002` проходят. Остатки: [BUG-1464](BUG-1464-OPEN.md) (`Selection.toString()` — `kana-009`, `math-auto-003`), [BUG-1465](BUG-1465-OPEN.md) (схлопнутый пробел → U+3000: `fullwidth-001/006/008`). `fullwidth-007` (U+3000×3 корректны в дампе раскладки) всё ещё FAIL в reftest — расхождение сегментации в тесте и ссылке, не в значении.
