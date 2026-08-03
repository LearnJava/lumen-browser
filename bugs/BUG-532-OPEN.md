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
