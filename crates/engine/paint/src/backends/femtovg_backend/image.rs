//! Image drawing: `<img>` / `background-image` placement, object-fit/
//! object-position (CSS Images L3 §5.5), downscale-resample on registration.
//!
//! Вырезано из `femtovg_backend.rs` (SPLIT-PR2 срез 3/4) без изменения
//! поведения — чисто перенос кода между модулями.

use super::*;

/// Решает, нужно ли area-averaged уменьшение для отрисовки изображения
/// `raw_w × raw_h` в прямоугольник `rect_w × rect_h` CSS-пикселей при device
/// `scale`, и если да — возвращает целевой размер в device-пикселях.
///
/// `Some((tw, th))` — целевой размер меньше исходного хотя бы по одной оси
/// (downscale): нужно пересэмплировать `resize_area_avg` чтобы избежать
/// алиасинга от билинейного сэмплинга femtovg (BUG-077). `None` — upscale или
/// точное совпадение: исходную текстуру можно сэмплить напрямую.
fn downscale_target(raw_w: u32, raw_h: u32, rect_w: f32, rect_h: f32, scale: f64) -> Option<(u32, u32)> {
    let tw = (f64::from(rect_w) * scale).round().max(1.0) as u32;
    let th = (f64::from(rect_h) * scale).round().max(1.0) as u32;
    if tw >= raw_w && th >= raw_h {
        None
    } else {
        Some((tw, th))
    }
}

/// Placement-rect для `<img>` (CSS Images L3 §5.5): куда внутри content box
/// `rect` рисуется текстура с учётом `object-fit` / `object-position`.
///
/// Чистая функция (без GL). `intrinsic` — натуральный размер декодированной
/// картинки (`raw_images`); `None` (нет raw-пикселей — текстура зарегистрирована
/// извне) → fit невозможен, возвращаем сам `rect` (историческое fill-поведение).
/// Возвращённый rect может выходить за `rect` (cover / none) — обрезку по
/// content box делает scissor в `draw_image_in_rect`.
fn image_placement(
    rect: Rect,
    intrinsic: Option<(u32, u32)>,
    fit: ObjectFit,
    position: ObjectPosition,
) -> Rect {
    match intrinsic {
        Some(size) => fit_image_rect(rect, size, fit, position),
        None => rect,
    }
}

/// Конвертирует `lumen_image::Image` в вектор `RGBA8` пикселей для femtovg.
pub(super) fn image_to_rgba8_vec(img: &Image) -> Vec<rgb::RGBA8> {
    use rgb::RGBA;
    // `to_rgba8` applies ICC colour management (ICC-3 matrix-shaper for RGB
    // profiles, gamut tone-mapping otherwise), so wide-gamut photos render
    // colour-correct in the live femtovg window. For images without a profile
    // it is a plain format conversion (no-op tone mapping).
    img.to_rgba8()
        .chunks_exact(4)
        .map(|px| RGBA { r: px[0], g: px[1], b: px[2], a: px[3] })
        .collect()
}

impl FemtovgBackend {
    /// Рисует изображение из зарегистрированного URL в content box `rect`
    /// с учётом `object-fit` / `object-position` (CSS Images L3 §5.5).
    ///
    /// Placement-rect считается `fit_image_rect` от intrinsic-размера
    /// декодированной картинки; для cover / none, когда placement выходит за
    /// `rect`, излишек срезается scissor-ом (spec: «clipped to the content
    /// box»). Ранее femtovg-бэкенд игнорировал fit/position и растягивал
    /// текстуру на весь `rect` (BUG-078). Downscale-ресэмпл (BUG-077)
    /// выполняется по placement-размеру, а не по box — для contain плитка
    /// меньше box, для cover больше.
    ///
    /// Intrinsic-размер неизвестен (нет raw-пикселей) → историческое
    /// fill-поведение. Не зарегистрировано вовсе — серый placeholder.
    pub(super) fn draw_image_in_rect(
        &mut self,
        rect: &Rect,
        src: &str,
        fit: ObjectFit,
        position: &ObjectPosition,
    ) {
        let placed = image_placement(
            *rect,
            self.raw_images.get(src).map(|raw| (raw.width, raw.height)),
            fit,
            *position,
        );
        if let Some(img_id) = self.resolve_image_for_rect(src, &placed) {
            self.canvas.save();
            self.canvas.intersect_scissor(rect.x, rect.y, rect.width, rect.height);
            let paint = femtovg::Paint::image(
                img_id,
                placed.x, placed.y, placed.width, placed.height,
                0.0, 1.0,
            );
            let mut path = femtovg::Path::new();
            path.rect(placed.x, placed.y, placed.width, placed.height);
            self.canvas.fill_path(&path, &paint);
            self.canvas.restore();
        } else {
            // Placeholder — светло-серый прямоугольник.
            self.draw_fill_rect(rect.x, rect.y, rect.width, rect.height, Color { r: 200, g: 200, b: 200, a: 255 });
        }
    }

    /// Рисует `background-image: url(...)` с учётом `background-size`,
    /// `background-position`, `background-repeat`, `background-origin` и
    /// `background-clip` (CSS Backgrounds L3 §3.3–3.5/§3.7/§3.8).
    ///
    /// `rect` — painting area (`background-clip`): плитки клипируются по ней.
    /// `origin_rect` — positioning area (`background-origin`): относительно неё
    /// считаются размер плитки и её позиция. Ранее femtovg-бэкенд игнорировал
    /// всё это и растягивал картинку на весь `rect` (BUG-095) — теперь
    /// геометрия плиток зеркалит wgpu `Renderer`.
    ///
    /// Незарегистрированный `src` → визуальный no-op (в отличие от `<img>`,
    /// фоновая картинка не рисует серый placeholder).
    pub(super) fn draw_background_image(
        &mut self,
        rect: &Rect,
        origin_rect: &Rect,
        src: &str,
        size: BackgroundSize,
        position: &ObjectPosition,
        repeat: BackgroundRepeat,
    ) {
        let (img_w, img_h) = match self.raw_images.get(src) {
            Some(raw) => (raw.width as f32, raw.height as f32),
            None => return,
        };
        if img_w <= 0.0 || img_h <= 0.0 {
            return;
        }

        let (tile_w, tile_h, tile_x_start, tile_y_start, repeat_x, repeat_y, step_x, step_y) = bg_tile_geometry(
            size,
            position,
            repeat,
            img_w,
            img_h,
            origin_rect.width,
            origin_rect.height,
            origin_rect.x,
            origin_rect.y,
        );
        if tile_w <= 0.0 || tile_h <= 0.0 {
            return;
        }

        // Разрешаем текстуру под размер плитки (area-averaged downscale при
        // уменьшении, как в draw_image_in_rect — BUG-077).
        let tile_rect = Rect::new(0.0, 0.0, tile_w, tile_h);
        let Some(img_id) = self.resolve_image_for_rect(src, &tile_rect) else {
            return;
        };

        // Плитки клипируются по painting area через scissor (пересекает
        // активный clip-стек, например overflow:hidden контейнер).
        self.canvas.save();
        self.canvas.intersect_scissor(rect.x, rect.y, rect.width, rect.height);

        let x_end = rect.x + rect.width;
        let y_end = rect.y + rect.height;
        let mut ty = tile_y_start;
        loop {
            if ty >= y_end {
                break;
            }
            if ty + tile_h > rect.y {
                let mut tx = tile_x_start;
                loop {
                    if tx >= x_end {
                        break;
                    }
                    if tx + tile_w > rect.x {
                        let paint =
                            femtovg::Paint::image(img_id, tx, ty, tile_w, tile_h, 0.0, 1.0);
                        let mut path = femtovg::Path::new();
                        path.rect(tx, ty, tile_w, tile_h);
                        self.canvas.fill_path(&path, &paint);
                    }
                    if !repeat_x {
                        break;
                    }
                    tx += step_x;
                }
            }
            if !repeat_y {
                break;
            }
            ty += step_y;
        }
        self.canvas.restore();
    }

    /// Возвращает femtovg `ImageId` для отрисовки `src` в `rect`, при сильном
    /// уменьшении подменяя исходную текстуру area-averaged уменьшенной копией.
    ///
    /// femtovg сэмплит текстуру билинейно — при downscale в несколько раз это
    /// даёт алиасинг (BUG-077): один выходной пиксель усредняет лишь 2×2
    /// соседей вместо всей покрываемой области. Зеркалим `Renderer` (wgpu):
    /// если целевой размер в device-пикселях (`rect × scale`) меньше исходного
    /// хотя бы по одной оси — пересэмплируем `resize_area_avg` до этого размера
    /// и кешируем под `"src@WxH"`. Upscale/точное совпадение → исходная текстура
    /// (билинейная фильтрация femtovg здесь корректна). Если у `src` нет
    /// декодированных пикселей (не зарегистрирован) — возвращаем то, что есть в
    /// кеше текстур, либо `None` (рисуется placeholder).
    fn resolve_image_for_rect(&mut self, src: &str, rect: &Rect) -> Option<femtovg::ImageId> {
        let (rw, rh) = match self.raw_images.get(src) {
            Some(raw) => (raw.width, raw.height),
            None => return self.images.get(src).copied(),
        };

        let (tw, th) = match downscale_target(rw, rh, rect.width, rect.height, self.scale) {
            Some(target) => target,
            // Не downscale (upscale или точное совпадение) — отдаём исходник.
            None => return self.images.get(src).copied(),
        };

        let key = format!("{src}@{tw}x{th}");
        // BUG-272 срез 18: варианты живут в LRU-ограниченном кэше, а не в
        // `self.images`; попадание помечает ключ как недавно использованный.
        if let Some(id) = self.resized_variants.get(&key) {
            return Some(id);
        }

        let raw = self.raw_images.get(src)?.clone();
        let resized = resize_area_avg(&raw, tw, th);
        let rgba = image_to_rgba8_vec(&resized);
        let img = imgref::ImgRef::new(&rgba, resized.width as usize, resized.height as usize);
        let id = self
            .canvas
            .create_image(femtovg::ImageSource::Rgba(img), femtovg::ImageFlags::empty())
            .ok()?;
        // Вставка сверх ёмкости вытесняет LRU-вариант; его текстуру удаляем после
        // следующего flush (могла быть отрисована ранее в этом кадре).
        if let Some(evicted) = self.resized_variants.insert(key, id) {
            self.resized_variant_pending_delete.push(evicted);
        }
        Some(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_layout::PositionComponent;

    #[test]
    fn downscale_target_triggers_on_large_downscale() {
        // 800×600 source drawn into 200×150 CSS px at scale 1 → downscale to 200×150.
        assert_eq!(downscale_target(800, 600, 200.0, 150.0, 1.0), Some((200, 150)));
    }

    #[test]
    fn downscale_target_none_on_upscale_or_exact() {
        // Exact match → no resample.
        assert_eq!(downscale_target(100, 100, 100.0, 100.0, 1.0), None);
        // Upscale in both axes → no resample (bilinear upscale by femtovg is fine).
        assert_eq!(downscale_target(100, 100, 300.0, 300.0, 1.0), None);
    }

    #[test]
    fn downscale_target_triggers_when_one_axis_shrinks() {
        // Squished horizontally only → still area-average (the shrunk axis aliases).
        assert_eq!(downscale_target(400, 100, 100.0, 100.0, 1.0), Some((100, 100)));
    }

    #[test]
    fn downscale_target_accounts_for_device_scale() {
        // 2× HiDPI: 200 CSS px → 400 device px, so a 300px source is upscaled, not down.
        assert_eq!(downscale_target(300, 300, 200.0, 200.0, 2.0), None);
        // But a 500px source into 200 CSS px @2× = 400 device px → downscale to 400.
        assert_eq!(downscale_target(500, 500, 200.0, 200.0, 2.0), Some((400, 400)));
    }

    #[test]
    fn image_placement_contain_letterboxes_landscape_image() {
        // 200×100 image in 180×120 box, contain → scale 0.9, 180×90, centered vertically.
        let rect = Rect::new(10.0, 20.0, 180.0, 120.0);
        let placed = image_placement(rect, Some((200, 100)), ObjectFit::Contain, ObjectPosition::default());
        assert_eq!((placed.x, placed.y, placed.width, placed.height), (10.0, 35.0, 180.0, 90.0));
    }

    /// Покомпонентное сравнение rect-а с допуском на float-погрешность
    /// (cover-scale считается делением, точные значения недостижимы).
    fn assert_rect_close(r: Rect, expected: (f32, f32, f32, f32)) {
        let (x, y, w, h) = expected;
        for (got, want) in [(r.x, x), (r.y, y), (r.width, w), (r.height, h)] {
            assert!((got - want).abs() < 1e-3, "got {r:?}, expected {expected:?}");
        }
    }

    #[test]
    fn image_placement_cover_overflows_box() {
        // 200×100 image in 180×120 box, cover → scale 1.2, 240×120, overflows horizontally
        // (clip is the caller's scissor by the content box).
        let rect = Rect::new(0.0, 0.0, 180.0, 120.0);
        let placed = image_placement(rect, Some((200, 100)), ObjectFit::Cover, ObjectPosition::default());
        assert_rect_close(placed, (-30.0, 0.0, 240.0, 120.0));
    }

    #[test]
    fn image_placement_position_right_bottom_with_cover() {
        // object-position: right bottom (100% 100%) shifts the overflow fully to the left/top.
        let rect = Rect::new(0.0, 0.0, 180.0, 120.0);
        let pos = ObjectPosition {
            x: PositionComponent::Percent(1.0),
            y: PositionComponent::Percent(1.0),
        };
        let placed = image_placement(rect, Some((200, 100)), ObjectFit::Cover, pos);
        assert_rect_close(placed, (-60.0, 0.0, 240.0, 120.0));
    }

    #[test]
    fn image_placement_unknown_intrinsic_falls_back_to_fill() {
        // No raw pixels (externally registered texture) → historical stretch-to-box.
        let rect = Rect::new(5.0, 5.0, 180.0, 120.0);
        let placed = image_placement(rect, None, ObjectFit::Contain, ObjectPosition::default());
        assert_eq!((placed.x, placed.y, placed.width, placed.height), (5.0, 5.0, 180.0, 120.0));
    }

    #[test]
    fn image_to_rgba8_vec_passthrough_rgba() {
        use lumen_image::{Image, PixelFormat};
        let img = Image { width: 1, height: 1, format: PixelFormat::Rgba8, data: vec![10, 20, 30, 200], icc_profile: None };
        let out = image_to_rgba8_vec(&img);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].r, 10);
        assert_eq!(out[0].a, 200);
    }

    #[test]
    fn image_to_rgba8_vec_expands_rgb() {
        use lumen_image::{Image, PixelFormat};
        let img = Image { width: 1, height: 1, format: PixelFormat::Rgb8, data: vec![10, 20, 30], icc_profile: None };
        let out = image_to_rgba8_vec(&img);
        assert_eq!(out[0].a, 255);
    }

    #[test]
    fn image_to_rgba8_vec_expands_gray8() {
        use lumen_image::{Image, PixelFormat};
        let img = Image { width: 1, height: 1, format: PixelFormat::Gray8, data: vec![128], icc_profile: None };
        let out = image_to_rgba8_vec(&img);
        assert_eq!(out[0].r, 128);
        assert_eq!(out[0].g, 128);
    }

    #[test]
    fn image_to_rgba8_vec_expands_gray_alpha() {
        use lumen_image::{Image, PixelFormat};
        let img = Image { width: 1, height: 1, format: PixelFormat::GrayAlpha8, data: vec![100, 200], icc_profile: None };
        let out = image_to_rgba8_vec(&img);
        assert_eq!(out[0].r, 100);
        assert_eq!(out[0].a, 200);
    }
}
