# BUG-1296 — Растеризованный SVG-фон отличается от эталона узкой полосой градиента на границе плиток (размытая граница между `lime` и `aqua`)

**Статус:** FIXED 2026-10-19 (P3, CPU-растеризатор; остаток на GPU-бэкендах — [BUG-1649](BUG-1649-OPEN.md))
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 6, `css/css-backgrounds`)
**Область:** paint/image (`crates/engine/image/src/svg.rs` → `DrawBackgroundImage` в `cpu_raster.rs`: масштабирование растра SVG билинейно, а не перерастеризация в целевой размер)


## Симптом

`<div style="width:100px;height:100px;background:url(a.svg) no-repeat;background-size:contain">`, SVG 8×32 из двух прямоугольников `lime` (верх) / `aqua` (низ):

- эталон (те же `<div>` с `background-color`) — резкая граница на y = 16;
- Lumen — между зелёным и бирюзовым 4 строки градиента (`(0,255,112)…(0,255,207)`), `7 936` пикселей `lime` вместо `8 192`.

SVG-картинка растеризуется в собственный размер (8×32), потом растягивается билинейно до размера фона. У векторной картинки при `background-size` её нужно растеризовать заново в целевой размер.

## Как найдено

WPT-RUN-14 срез 6: `background-size/vector/*` — 104 упавших reftest с разницей ≤ 8 000 пикселей (98 `thick`, 6 `thin-only`; ещё 69 id с большей разницей — геометрия, [BUG-1295](BUG-1295-OPEN.md)). Полоса промежуточного цвета подтверждена пробой на двух (`wide--contain--nonpercent-width-nonpercent-height.html`, `background-size-vector-001.html`: 32 различающихся пикселя в двух строках); для остальных 12 с абсолютными `width`/`height` в корне SVG причина предположена, не проверена каждая. Класс отличен от шва AA [BUG-1249](BUG-1249-FIXED.md): здесь размывается содержимое картинки, а не край бокса.

## Что делать

Перерастеризовывать SVG-фон в `size × device_scale` слоя (кэш по паре «файл, размер»); не масштабировать растр.

## Как проверить

`background-size/vector/background-size-vector-0{01…18,21…29}.html`, `wide--*nonpercent-width-nonpercent-height*.html`; проба в тексте выше (две строки пикселей: до и после y = 16).

## Исправление

`lumen_image::Image` получил поле `vector_source: Option<Arc<VectorSource>>` (`crates/engine/image/src/lib.rs`): `decode_svg` кладёт туда байты документа, растровые форматы — `None`. Новая `lumen_image::rasterize_svg(&VectorSource, w, h)` (`svg.rs`) заново рисует документ ровно в `w×h` (тот же `render_tree`, что и `decode_svg`, масштаб по размеру `usvg::Tree`).

`rasterize_background_image` (`crates/engine/paint/src/cpu_raster.rs`) для каждой плитки, у которой округлённый размер на устройстве отличается от размера растра, берёт `vector_tile` — перерисовку источника в размер плитки (последняя перерисовка кэшируется на время одной команды: плитки одного фона одного размера), и блитит её 1:1. Растровые картинки и SVG с плиткой собственного размера идут прежним путём — пиксели эталонов графических тестов не меняются (единственная страница с SVG-фоном, `154-*`, рисует его в собственном размере).

**Тесты:** `cpu_raster::tests::draw_background_image_svg_is_rerasterized_at_tile_size` (граница `lime`/`aqua` остаётся резкой в строках 49/50 при `contain` 8×32 → 25×100), `svg::tests::rasterize_svg_redraws_at_requested_size`.

**Не измерено:** прогон `background-size/vector/*` через `run_report.py` не делался (диск D заполнен, `dev-release` не собрать) — число закрытых reftest неизвестно. `<img src=*.svg>` с масштабированием тем же `vector_source` не перерисовывается (`DrawImage`), это отдельный путь.
