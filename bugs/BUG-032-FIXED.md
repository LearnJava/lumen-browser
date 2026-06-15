# BUG-032

**Статус:** FIXED 2026-05-22
**Компонент:** paint/image

## Описание

object-fit image quality ~16%: area averaging заменяет bilinear при downscale

## Детали

TEST-19 (object-fit), TEST-18 (images): пиксельная разница ~16% при большом коэффициенте уменьшения (~4.7x, 852×725 → 180×120).

### Что сделано (2026-05-21)

1. **CPU-side bilinear resize** — реализован в `lumen-image/src/lib.rs`:
   - `Image::to_rgba8()` — конвертация любого формата в RGBA8
   - `pub fn resize_bilinear(src: &Image, dst_w: u32, dst_h: u32) -> Image` — 4-tap bilnear с half-pixel offset
   - В `renderer.rs` добавлен pre-pass перед render loop: для каждого `DrawImage` вызывается `ensure_image_gpu_key()`, которая создаёт CPU-ресайзированную текстуру и кеширует под ключом `"src@WxH"`.
   - Разделение на `compute_image_gpu_key(&self)` (иммутабельный) + `ensure_image_gpu_key(&mut self)` (мутабельный pre-pass) обязательно — иначе borrow-checker блокирует (в render loop `parsed_faces: Vec<Option<ParsedFace<'_>>>` держит `&self.faces`).

2. **Результат:** минимальное улучшение: TEST-18 14.68% → 14.44%, TEST-19 16.14% → 16.54% (шум, не улучшение).

### Почему не помогло

CPU bilinear ≈ GPU bilinear — оба делают 4-выборки. При коэффициенте уменьшения 4.7x область покрытия одного выходного пикселя = 4.7×4.7 = ~22 исходных пикселей, из которых bilinear учитывает лишь 4. Антиалиасинг не обеспечивается.

Edge/Chrome используют **Skia**, который при downscale применяет **Lanczos-3** (или area averaging) — усредняет все пиксели в области покрытия. Поэтому разные браузеры дают одинаковый результат: они используют одну библиотеку (Skia).

Дополнительная причина: текстуры загружаются как `Rgba8Unorm` (linear), хотя PNG-файлы хранят sRGB. Блендинг в linear-пространстве при правильных финальных значениях дал бы совпадение, но sRGB→linear конвертация при загрузке не выполняется → цветовые ошибки ~2-5%.

### Что нужно сделать

1. **[Приоритет 1] Area averaging (box filter) для downscale:**
   ```rust
   // Заменить resize_bilinear на resize_area_avg для случаев (dst < src)
   pub fn resize_area_avg(src: &Image, dst_w: u32, dst_h: u32) -> Image;
   // Алгоритм: для каждого dst-пикселя вычислить float-прямоугольник в src-координатах,
   // усреднить все целые пиксели + частичные веса по краям.
   ```
   Ожидаемый результат: совпадение с Edge ~2-4% (только sRGB-девиация останется).

2. **[Приоритет 2] sRGB при загрузке текстур:**
   Изменить формат текстуры с `Rgba8Unorm` на `Rgba8UnormSrgb` в `renderer.rs` → wgpu автоматически конвертирует sRGB→linear при sampling. Требует также перевода surface в sRGB (`TextureFormat::Bgra8UnormSrgb`). Запланировано на Phase 3+.

### Файлы

- `crates/engine/image/src/lib.rs` — `to_rgba8()`, `resize_bilinear()`
- `crates/engine/paint/src/renderer.rs` — pre-pass, `ensure_image_gpu_key()`, `compute_image_gpu_key()`, `make_gpu_image_entry()`
