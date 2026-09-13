# BUG-532: CSS `zoom` property is not implemented at all

**Статус:** FIXED 2026-09-13 (P3)
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

2. **`getComputedStyle()` не "un-zoom"-ил значения (CSS Viewport L1 §5,
   `#zoom-om`) — ЗАКРЫТО этим срезом (часть 2, 2026-09-13).**
   `selector_query.rs::computed_style_to_map` делит на `style.effective_zoom`
   ровно те `Length::Px`-поля, что `cascade.rs::apply_zoom_to_lengths`
   зумит на входе — `margin`/`padding`/border-width/`font-size` (новые
   `unzoom_length_to_css`/`unzoom_length_or_auto_to_css`); `width`/`height`/
   `inset`/`min-*`/`max-*` намеренно не тронуты — те возвращают used value
   (зумленное) и на реальных браузерах, `computed-initial.html` явно
   исключает их из проверки равенства по этой причине.

   Живой прогон `run_smoke.py` (dev-release, 2026-09-13) на всей группе
   TIMEOUT/FAIL-файлов из §.ini: `computed-initial.html` 9/16 → **126/126**
   (`.ini` удалён), `computedStyle-zoom.html` 1/6 → **6/6** (кроме одного
   дублирующегося имени сабтеста — `.ini` сужен на 1 строку),
   `zoom-interpolation.html` 0/202 → **200/202** (`.ini` сужен на 200
   строк; 2 сабтеста реально не проверены из-за отдельного краша
   WPT-исполнителя на `:hover`-actions, не связано с un-zoom),
   `computed-border-width-keywords.html`/`computed-column-rule-width-keywords.html`/
   `computed-outline-width-keywords.html`/`font-size-keyword-system-font.html`/
   `scroll-top-test-with-zoom.html`/`widget.html`/
   `word-spacing-inherited-computed.html` — все **целиком PASS**, `.ini`
   удалены. `length-implicit-and-explicit-inheritance.html` 0/5 →
   **3/5** (`width`/`height` inherit остаются FAIL — не length-полей,
   вне области un-zoom). `text-indent-computed.html` 0/10 → **6/10**
   (`rem`/`inherit`-варианты остаются FAIL — `text-indent` не входит в
   `apply_zoom_to_lengths`, следующий кандидат для расширения).

   Живая проверка также вскрыла **три отдельных, не связанных с un-zoom
   дефекта**, вынесенные в собственные карточки, а не смешанные сюда
   (`docs/probe-method.md` §8): [BUG-1050](BUG-1050-OPEN.md)
   (`getComputedStyle()` пропускает несколько шортхендов/свойств и
   неверно сериализует `box-shadow`/`text-shadow`/`filter` —
   `svg-computed-style.html` 22/88), [BUG-1051](BUG-1051-OPEN.md)
   (`lh`/`ex`/`cap`/`ch`/`vh`/`vw` не совпадают со своей root-relative
   парой под `zoom` — `font-relative-units.html` 1/6,
   `relative-units.html` 1/6), [BUG-1052](BUG-1052-OPEN.md)
   (`zoom: calc(sign(...))` отклоняется парсером целиком —
   `zoom-with-sign-function.html` 0/2). Их `.ini` не тронуты этим
   срезом (`svg-computed-style.html.ini` переписан под текущий фактический
   результат — 66 FAIL — но карточка дефекта отдельная).

   `BUG-532` закрыт — оба открытых пункта карточки решены; `zoom-valid.html`
   в части 1, `getComputedStyle()`-un-zoom в части 2. Остаточные частные
   пробелы (text-indent rem/inherit, `:hover`-actions краш) не блокируют
   закрытие — не входят в исходный симптом карточки.
