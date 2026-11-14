# BUG-1776 — unicode-bidi-ua-rules: −1 подтест после merge `89500a780` (`<br>` без текста открывает пустую строку)

**Статус:** FIXED 2026-11-09 (P3)
**Заведён:** 2026-10-28 (P2, WPT-DROP-1027-S3)
**Область:** layout (`crates/engine/layout/src/scoped_collect.rs`; вторая сторона — `crates/engine/layout/src/box_tree/inline_build.rs`)

## Симптом

`/html/rendering/bidi-rendering/unicode-bidi-ua-rules.html`: 49/130 (старый бинарь, WPT-RUN-16) → 48/130
(текущий `main`, оба прогона). Потеря одного подтеста; сам тест и так проходит меньше половины.

## Бисект (WPT-DROP-1027-S3)

Окно `e3536bfb3`(49) … `89500a780`(48); сборки `5aae09dce` 49, `a06d378ef` 49, `d17e471ac` 49 →
первый плохой — `89500a780` «Merge branch 'main'». Из него в `main` попадает
`5fc170885 WIP BUG-1411: <br> без текста перед ним открывает пустую строку`
(`crates/engine/layout/src/box_tree/inline_build.rs` +54, `tests/br_lines.rs`). Подозреваемый
подтест — со строкой `<br>` внутри bidi-элемента: лишняя пустая строка сдвигает
порядок/результат сравнения.

## Воспроизведение

`tests/wpt/run_smoke.py --binary <lumen.exe> /html/rendering/bidi-rendering/unicode-bidi-ua-rules.html`;
бинари слота p2-work: `.tmp/bisect-bin/{d17e471ac,89500a780}/lumen.exe` (`lumen.py` из
`a76e4c7b9`). Ожидание 49/130, факт 48/130. Приоритет низкий (−1 подтест).

## Причина (2026-11-09)

Подтест — `UA stylesheet rule for unicode-bidi, for <br>`; терялся он не из-за лишней строки,
а из-за `getComputedStyle(br).unicodeBidi === ''` после третьего assert'а подтеста
(`dir="ltr"` → `dir="LtR"`, регистронезависимое значение). Столкнулись два механизма:

1. **BUG-935, слайс 70** — инкрементальный сбор computed-стилей (`ScopedCollection::plan`,
   `scoped_collect.rs`): узел со НЕизменившимся стилем попадает в `skips.own`, и его уже
   опубликованная полная запись стиля не пересобирается и не выселяется. `dir="LtR"`
   каскадируется в тот же computed-стиль, что и `dir="ltr"` (регистр значения не важен), поэтому
   запись `<br>` считалась неизменной.
2. **BUG-1411 WIP `5fc170885`** — `give_leading_breaks_a_line` (`inline_build.rs`): ведущий `<br>`
   без текста перед ним теперь конвертируется из Block в `InlineRun` с одним сегментом
   `{text: "", forced_break: true, source_node: br}` — ран публикует **сам себя** как сегмент.
   Сегментные записи собираются трёхсвойственной картой `inline_segment_style_map`
   (visibility, white-space, text-transform).

Мутация атрибута кладёт `<br>` в замыкание изменений (closure), поэтому ран пересобирается
(узел в closure исключает run-skip) — и `collect_computed_styles` писал в `fresh` сегментную
трёхсвойственную запись под тем же индексом узла. `out.extend(fresh)` у HashMap **заменяет**
значение: свежая сегментная карта затирала сохранённую полную запись. Дальнейшие чтения
`unicodeBidi` получали `''` — подтест падал на `dir=LtR`. До `5fc170885` у ведущего `<br>` не
было сегмента, публикующего тот же узел, — затирать было нечем, поэтому регресс проявился
именно с этим коммитом.

Изолирующая проба (бит-маска из 6 assert'ов подтеста, рендер `--screenshot`): `br` один в
`body` — маска `110111`, падает ровно assert №3 (`LtR`), сырое значение — пустая строка;
`span` без текста — все прочтения `''` (до-существующая деградация пустых inline-элементов,
не регресс); `area` — `111111`; та же маска после полного WPT-последовательного
`createElement`/`remove` — `110111` (последовательность не при чём).

## Фикс

`Plan::collect_computed_styles` (`crates/engine/layout/src/scoped_collect.rs`): перед
`out.extend(fresh)` выкидывать из `fresh` записи, которые план доказал неизменными
(`skips.own`) и которые уже опубликованы в `out`, — их полная запись переживает `extend` как
есть. Регрессионный тест `a_kept_own_entry_survives_a_rebuilt_inline_run_segment` в том же
файле (красный без фикса — sentinel-assert ловит именно затирание, не выселение).

## Проверка

- WPT `unicode-bidi-ua-rules`: **49/130** восстановлено, `for <br>` в списке UNEXPECTED-PASS
  (бинарь `dev-release`, `run_smoke.py`).
- Проба после фикса: `ALONE 111111 … SEQ 111111` (`SPAN` — прежние `000000`, до-существующие).
- `cargo clippy -p lumen-layout --all-targets -- -D warnings` — чисто;
  `cargo clippy --workspace --all-targets -- -D warnings` — чисто.
- `cargo test -p lumen-layout scoped_collect` (11, включая новый), `br_lines` (3) — зелёные.
- `scripts/scoped-test.sh` — зелёный, кроме двух падений чистого `main`, воспроизведённых
  байт-в-байт на нетронутом HEAD: `bug1191_boxless_computed_style` ([BUG-1784](BUG-1784-FIXED.md))
  и `cpu_snapshots_match_references` ([BUG-1236](BUG-1236-OPEN.md)).
- `graphic_tests/dump_golden.py` — все 12 дампов совпадают с эталоном.
