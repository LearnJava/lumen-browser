//! `feImage href="#id"` (Filter Effects L1 §«feImage»): элемент документа
//! рисуется «как у `<use>»` — в user space ссылающегося элемента, со сдвигом в
//! начало подобласти примитива.

use lumen_layout::FeElementPicture;

use super::{Geometry, Img};

/// Рисует `picture` в рабочую область `w × h`; начало user space ставится в левый
/// верхний угол подобласти `origin` (в px рабочей области, может быть отрицательным:
/// подобласть шире холста). Вне подобласти результат обрежет вызывающий.
pub(super) fn render(picture: &FeElementPicture, g: &Geometry, origin: (i32, i32), w: usize, h: usize) -> Img {
    let mut out = Img::new(w, h);
    if w == 0 || h == 0 {
        return out;
    }
    let boxes = picture.positioned([g.sx, 0.0, 0.0, g.sy, origin.0 as f32, origin.1 as f32]);
    let commands = crate::display_list::boxes_commands(&boxes);
    let images = std::collections::HashMap::new();
    let Ok(pixmap) = crate::cpu_raster::rasterize_cpu_pixmap(w as u32, h as u32, &commands, &images, None, None)
    else {
        return out;
    };
    for (o, p) in out.d.iter_mut().zip(pixmap.data().chunks_exact(4)) {
        *o = [
            f32::from(p[0]) / 255.0,
            f32::from(p[1]) / 255.0,
            f32::from(p[2]) / 255.0,
            f32::from(p[3]) / 255.0,
        ];
    }
    out
}
