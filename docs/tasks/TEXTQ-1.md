# TEXTQ-1 — глиф растрируется в точном размере в device px, тексель = пиксель

Дорожка TEXTQ (качество текста в живом окне), срез 1. Владелец — P1. Крейты — `lumen-paint`, `lumen-font`.

## Зачем

Текст в живом окне (wgpu) заметно мыльнее, чем в Chrome и чем наш же `--screenshot` (CPU). Три причины, все в GPU-пути:

1. **Bin-ы размеров.** Глиф растрируется только в `SIZE_BINS = [8,12,16,20,24,32,48,64]` CSS px (округление вверх, [shaders.rs:67](../../crates/engine/paint/src/renderer/shaders.rs#L67) `size_bin_for`), затем квад масштабируется на `adjusted_font_size / bin` ([glyph_raster.rs:652](../../crates/engine/paint/src/renderer/glyph_raster.rs#L652)). 13–15 px рисуются как 16 и ужимаются, 17 px — как 20, всё > 64 px растягивается. Атлас — `R8Unorm` с `Linear`-сэмплером ([construct.rs:532](../../crates/engine/paint/src/renderer/construct.rs#L532)) → мыло.
2. **DPR не учитывается.** Вершины в CSS px, viewport-uniform = surface / `scale_factor` ([renderer.rs:1007](../../crates/engine/paint/src/renderer.rs#L1007)). При 150 % масштабе Windows 16-px глиф растрируется в 16 px и растягивается GPU в 1,5 раза.
3. **Дробные позиции без привязки.** `push_glyph_quad` ([glyph_raster.rs:295](../../crates/engine/paint/src/renderer/glyph_raster.rs#L295)) ставит квад на дробные `cursor_x`/`baseline_y`; baseline тоже дробный (`rect.y + size × ascent_ratio`, [glyph_raster.rs:552](../../crates/engine/paint/src/renderer/glyph_raster.rs#L552)). Билинейная выборка размазывает каждый глиф на долю пикселя.

CPU-путь уже растрирует в точном размере и округляет origin глифа ([cpu_raster.rs:3326](../../crates/engine/paint/src/cpu_raster.rs#L3326), `:3354`) — поэтому скриншоты резче окна.

## Что сделать

1. **Замер «до».** Добавить в `graphic_tests/` страницу качества текста: системный `Arial` и `Segoe UI`, чёрный на белом и белый на тёмном, размеры 11, 12, 13, 14, 15, 16, 17, 20, 28, 72 px, строка с дробным `left` (например `left: 10.3px`). Внести её id в `GPU_CAPTURE` ([run.py:662](../../graphic_tests/run.py#L662)) — снимок идёт через gdigrab, то есть именно wgpu-путь. Порог по пикселям тексту против Edge не выдержать (ClearType) — тест информационный: записать diff% «до» в ROADMAP-строку TEXTQ-1. Проверка: `python graphic_tests/run.py --self-test`.
2. **Размер растеризации = точный размер в device px.** Удалить `SIZE_BINS`/`size_bin_for`. Размер растеризации `raster_px = adjusted_font_size × scale_factor`, квантованный до 1/4 px (`size_q = round(raster_px × 4)`, хранить в `AtlasKey` вместо `size_bin`, [atlas.rs:40](../../crates/engine/paint/src/atlas.rs#L40)). `Rasterizer::new(size_q as f32 / 4.0, upem)` в `rasterize_and_insert` ([glyph_raster.rs:983](../../crates/engine/paint/src/renderer/glyph_raster.rs#L983)).
3. **`scale_factor` до glyph-кода.** Протянуть DPR в `push_text_glyphs` и `push_text_glyphs_mixed` (`glyph_raster.rs:501`, `:792`) и во все их вызовы. Display-масштаб квада становится `1 / scale_factor` (bitmap в device px → вершины в CSS px).
4. **Привязка к пиксельной сетке.** Считать позицию пера в device px: `pen_dev = cursor_x × dpr`.
   - По Y: `baseline_dev = round(baseline_y × dpr)`, верх квада = `baseline_dev − top` (top у Bitmap — целое число пикселей).
   - По X: целая часть — `floor(pen_dev)`, дробная — фаза из 4 корзин (`0, .25, .5, .75`). Фаза входит в `AtlasKey` (новое поле `x_phase: u8`), глиф растрируется со сдвигом контура на `phase / 4` px.
   - В `lumen-font` добавить `Rasterizer::rasterize_offset(&self, glyph, dx: f32) -> Option<Bitmap>` (сдвиг точек контура до расчёта bbox). **`rasterize` не менять** — его выход — замороженный контракт CPU-эталонов ([subsystems/font.md](../../subsystems/font.md)); `rasterize(g)` ≡ `rasterize_offset(g, 0.0)`, это проверяет тест.
   - Итог: углы каждого текстового квада лежат на целых device px, и выборка из атласа идёт 1:1 (Linear-сэмплер на точных текселях — то же, что Nearest). Проверить, что `uv` указывают на центры текселей без полутексельного сдвига.
5. **Атлас.** Точные размеры × 4 фазы увеличивают число записей. Проверить поведение `GlyphRaster::OutOfSpace` ([glyph_raster.rs:968](../../crates/engine/paint/src/renderer/glyph_raster.rs#L968)) на странице с 5+ размерами шрифта; при нехватке поднять `ATLAS_DIM` ([shaders.rs:13](../../crates/engine/paint/src/renderer/shaders.rs#L13)) до 2048 (с учётом `limits`) и/или держать фазы только для размеров ≤ 48 device px (крупнее — одна фаза, разница не видна).
6. Обновить doc-комментарии, где упоминаются bin-ы, и [subsystems/font.md](../../subsystems/font.md)/`subsystems/paint.md` (если там описан атлас).

## Не трогать

- CPU-путь (`cpu_raster.rs`) и `Rasterizer::rasterize` — CPU-эталоны двигаться не должны.
- Гамму/контраст покрытия — это TEXTQ-2. Шейпинг — TEXTQ-4.

## Готово, когда

1. Юнит-тесты: `rasterize_offset(g, 0.0)` побайтно равен `rasterize(g)`; ключ атласа различает `size_q` и `x_phase`; при `scale_factor = 1.5` и `font_size = 16` глиф растрируется в 24 px; углы квадов из `push_text_glyphs` при dpr 1 и 1.5 кратны `1/dpr` CSS px (то есть целые device px).
2. Страница качества текста из п. 1 снята «до» и «после» (gdigrab), diff% против Edge записан в ROADMAP-строку; визуально 13–15 px и 72 px больше не мыльные. Скриншоты «до/после» приложить в `docs/tasks/` не нужно — достаточно цифр и описания.
3. Живая проверка на реальном сайте (`--maximized`, `LUMEN_NO_ADBLOCK=1`, см. [docs/automation.md](../automation.md)) при масштабе Windows 100 % и, если доступно, 125/150 %.
4. Полный `python graphic_tests/run.py --continue-on-fail`, затронутые эталоны (GPU_CAPTURE, `tests/snapshots`) перегенерированы в том же коммите.
5. `cargo clippy -p lumen-paint -p lumen-font --all-targets -- -D warnings` чисто.
