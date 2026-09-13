# BUG-532: CSS `zoom` property is not implemented at all

**Статус:** OPEN
**Дата:** 2026-08-03
**Компонент:** css-parser / layout (`crates/engine/css-parser/src/*.rs`, `crates/engine/layout/src/style.rs`)
**Найден:** P2, WPT-RUN-3 срез 25 (`css/css-viewport`) — массовый прогон, 22 testharness id,
15/22 harness OK, 16/284 сабтестов

## Симптом

```
grep -rn '"zoom"' crates/engine/css-parser/src/*.rs crates/engine/layout/src/style.rs
# ноль совпадений
```

`zoom/parsing/zoom-valid.html` пары `test_valid_value("zoom", …)`/`test_invalid_value("zoom", …)`
(https://drafts.csswg.org/css-viewport/#zoom-property) дают 9/16: все 9 "should be valid" случаев
проходят тривиально (движок принимает произвольное значение нераспознанного свойства без
валидации), все 7 "should be invalid" случаев (`auto`, `reset`, `document`, отрицательные числа)
не отклоняются тем же путём — ComputedStyle не содержит поля `zoom`, парсер не распознаёт
свойство вовсе, поэтому ни один вызов `getComputedStyle().zoom` не отражает реальное состояние,
и никакого визуального/лейаутного эффекта `zoom: N` не производит.

## Влияние

Вся категория `css/css-viewport` построена вокруг этого одного свойства — 202 сабтеста
`zoom-interpolation.html` (WAAPI-интерполяция `zoom` между значениями), 6 `computedStyle-zoom.html`,
и 15 файлов `zoom/*.html` (влияние `zoom` на relative units — `em`/`rem`/`ex`/`ch`, компонуемые
computed-значения ширины бордеров/аутлайнов по ключевым словам под зумом, наследование, `scrollTop`
под зумом, SVG). Ни один из этих сценариев не может пройти, пока свойство не реализовано целиком
(parse → `ComputedStyle::zoom` → влияние на used values relative units → paint scale).

7 файлов TIMEOUT вместо FAIL (`zoom/computed-border-width-keywords.html`,
`zoom/computed-column-rule-width-keywords.html`, `zoom/computed-initial.html`,
`zoom/computed-outline-width-keywords.html`, `zoom/font-relative-units.html`,
`zoom/svg-computed-style.html`, `zoom/zoom-with-sign-function.html`) — не разобрано, чем именно
эти семь отличаются от остальных 8 (тоже полагающихся на `zoom`, но доходящих до harness OK);
возможный кандидат — общий хелпер, специфичный для этой подгруппы, не проверен.

## .ini

`tests/wpt/metadata/css/css-viewport/` — `expected: FAIL` на все сабтесты
`zoom-interpolation.html`/`computedStyle-zoom.html`/`zoom/*.html` (кроме `font-relative-units-with-zoom.html`,
которая проходит целиком — 7/7, не использует `zoom:` напрямую), `expected: TIMEOUT` на семь
файлов выше. `zoom-valid.html` — только 7 `test_invalid_value`-сабтестов помечены FAIL (9
`test_valid_value` реально проходят и не нуждаются в `.ini`).

## Срез P3 2026-09-13

Заголовок и §Симптом устарели: `zoom` **реализован** отдельным треком между
находкой этого бага (2026-08-03) и этим срезом — `ComputedStyle::effective_zoom`,
парсинг/каскадное умножение, масштабирование `font-size` и абсолютных длин
блочной модели, влито 2026-08-10 (`ae521c1f5` и последующие, `CSS-SPECS.md`
строка `zoom`). `grep -rn '"zoom"'` из §Симптом больше не даёт ноль совпадений
(`crates/engine/layout/src/style/cascade.rs::parse_zoom`).

Оставшаяся часть этой карточки, ещё не закрытая:

1. **`zoom-valid.html` (CSSOM-валидация) — ЗАКРЫТО этим срезом.** Хотя
   layout-эффект уже был реализован, инлайновый `style`-сеттер (`element.style.zoom = …`,
   `crates/js/src/shim/web_api_shim_mid.js::_lumen_canonicalize_longhand`) не знал
   про `zoom` вовсе — принимал любую строку verbatim (BUG-484's старый механизм,
   не покрытый CSSOM-2, потому что `zoom` не входит в `css-parser`'s
   `SUPPORTED_PROPERTIES`/`apply_declaration` — он парсится отдельным пре-пассом
   в `cascade.rs`, а не через обычный declaration-match). Добавлен
   `_lumen_css_canonical_zoom` (грамматика CSS Viewport L1 §5: `normal | <number [0,∞]> |
   <percentage [0,∞]>`, без `auto`/`reset`/`document` — те последние два `cascade.rs::parse_zoom`
   по-прежнему толерантно принимает для реального рендеринга, но CSSOM-грамматика
   их отклоняет per spec) и `"zoom"` добавлен в `SUPPORTED_PROPERTIES`
   (`CSS.supports()`/`@supports`). `zoom-valid.html`: 9/16 → **16/16**, `.ini`
   удалён (нёс только `FAIL` на теперь проходящие сабтесты).

2. **Открыто, не тронуто этим срезом — getComputedStyle() не "un-zoom"-ит
   значения (CSS Viewport L1 §5, `#zoom-om`).** `computed-initial.html`
   (`assert_equals: expected "16px" but got "160px"` под `zoom: 10` — сам
   реальный WPT-прогон `run_smoke.py`, не гипотеза) и `margin.html` (reftest,
   тоже FAIL, не изолирован) показывают, что `getComputedStyle()` отдаёт
   **зумленное** used-value вместо "как будто zoom не применялся" —
   спека требует обратного масштабирования (`selector_query.rs::computed_style_to_map`
   ничего не делает с `effective_zoom`). Это отдельный, более крупный кусок работы
   (алгоритм un-zoom для каждого свойства-длины в `getComputedStyle`, а не только
   для `font-size`) — не входит в объём этого среза, `zoom-interpolation.html`
   (202 сабтеста) и оставшиеся `zoom/*.html`/семь TIMEOUT-файлов из §Влияние
   тоже не перепроверены. `BUG-532` остаётся `OPEN`, сужен до этого пункта.
