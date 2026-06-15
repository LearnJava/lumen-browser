# BUG-077

**Статус:** FIXED 2026-06-09
**Компонент:** image/paint
**Файл:** `crates/engine/paint/src/backends/femtovg_backend.rs:554`

## Описание

femtovg-бэкенд (default) сэмплил полноразмерную текстуру билинейно → алиасинг при сильном downscale. Fix: храним декодированные пиксели (raw_images) и при downscale пересэмплируем resize_area_avg до device-размера, кешируя под "src@WxH" (зеркало wgpu Renderer). TEST-18: 25.73%→21.21%; остаток — расхождение ядра ресэмплинга (box-average vs Edge bicubic), класс AA-дивергенции
