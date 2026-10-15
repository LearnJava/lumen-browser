# TEXTQ-6 — синтетический жирный и курсив в GPU-пути

Дорожка TEXTQ, срез 6. Владелец — P1. Крейт — `lumen-paint`. **Зависит от:** TEXTQ-1.

## Зачем

GPU-путь использует `font_weight`/`font_style` только для выбора face ([renderer.rs:282](../../crates/engine/paint/src/renderer.rs#L282), в `DrawText` — `font_weight: _`, `font_style: _`, [renderer.rs:1661](../../crates/engine/paint/src/renderer.rs#L1661)). Если в семействе нет жирного или курсивного начертания (частый случай для `@font-face` с одним файлом и для части системных шрифтов), страница получает обычное начертание. CPU-путь синтезирует оба ([cpu_raster.rs:3270](../../crates/engine/paint/src/cpu_raster.rs#L3270): `synth_bold = font_weight >= 600`, `bold_offset = (font_size / 24).clamp(0.5, 2.0)`, `synth_italic`), femtovg — тоже (`backends/femtovg_backend/text.rs:235`).

## Что сделать

1. Условие синтеза — как в CPU-пути, но только когда выбранный face сам не жирный/не курсивный (проверить, что CPU-путь делает так же; если нет — исправить в обоих, CSS Fonts 4 §5.5 `font-synthesis`, учесть свойство `font-synthesis`, если оно уже есть в `ComputedStyle`).
2. Жирный: второй квад того же глифа со сдвигом на `bold_offset` (в device px, привязанный к сетке после TEXTQ-1), advance не меняется — как CPU-путь.
3. Курсив: наклон квада в вершинах (`x += (baseline − y) × tan(14°)`), без перерастеризации.
4. Синтетический жирный в атласе не кэшировать отдельно — это тот же глиф.

## Готово, когда

1. Юнит-тест: `font-weight: 700` для face без bold даёт два квада на глиф; для face с собственным bold — один. Курсив — квад с наклоном.
2. Графический тест (новый или существующий из `GPU_CAPTURE`) с `@font-face` из одного regular-файла и `<b>`/`<i>` — совпадает по виду с CPU-путём.
3. Полный `python graphic_tests/run.py --continue-on-fail`, затронутые эталоны перегенерированы в том же коммите.
4. `cargo clippy -p lumen-paint --all-targets -- -D warnings` чисто.
