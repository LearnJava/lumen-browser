//! Обход перекрытий cue-боксов (WebVTT §7.2 «Rules for updating the display
//! of WebVTT text tracks», срез VTT-RENDER-S6).
//!
//! Cue-боксы — абсолютные дети [`BoxRole::VttCueContainer`]; их `rect`
//! финальны к моменту вызова (`layout_dispatch::finish_after_match`,
//! после `lay_out_abs_children`). Алгоритм §7.2, шаги «overlap»/«switched»:
//! боксы собираются в список `boxes` в порядке следования; каждый бокс,
//! пересекающийся с уже поставленным, сдвигается по оси блока на высоту
//! пересёкшегося бокса (вниз при неотрицательной `line`, вверх при
//! отрицательной или `auto` — как §7.2 расставляет `snapToLines`), до
//! отсутствия пересечений. Выход за область видео (`title area` = rect
//! контейнера) отменяет направление один раз (флаг `switched`), при
//! повторном выходе бокс остаётся на исходном месте. Для cue с
//! `snapToLines === false` (`line` в процентах) сдвигов нет.
//!
//! Не §7.3 (регионы) и не вертикальный подъём по `vertical`-настройке:
//! ось блока берётся из `writing-mode` cue-бокса, направление — из того,
//! каким inset-свойством (`top`/`bottom`/`left`/`right` c единицей `lh`)
//! [`cue_box_css`] записал блочную геометрию.

use super::types::{BoxRole, LayoutBox};
use crate::style::{Length, LengthOrAuto, WritingMode};
use lumen_core::geom::Rect;

/// Физическая ось блока cue-бокса: горизонтальное письмо — вертикальная
/// ось Y, вертикальное — горизонтальная X (CSS Writing Modes L3 §6.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BlockAxis {
    X,
    Y,
}

/// Запускает обход перекрытий для контейнера cue: сдвигает его детей
/// (cue-боксы) по алгоритму §7.2. Вызывается из [`finish_after_match`]
/// для боксов с ролью [`BoxRole::VttCueContainer`]; на прочих боксах
/// вызов недопустим (дети — не cue).
pub(super) fn avoid_cue_overlap(container: &mut LayoutBox) {
    debug_assert!(
        container.origin.role == BoxRole::VttCueContainer,
        "avoid_cue_overlap is for VttCueContainer boxes only"
    );
    let bounds = container.rect;
    // `boxes` из §7.2 — rect уже поставленных боксов; сдвиг виден только
    // последующим боксам, порядок следования сохраняется.
    let mut placed: Vec<Rect> = Vec::new();
    for i in 0..container.children.len() {
        let start = container.children[i].rect;
        let mut final_rect = start;
        // §7.2: обход перекрытий применяется только при включённом
        // snap-to-lines; `None` — `line` в процентах (`snapToLines=false`)
        // или геометрия не от `cue_box_css` — бокс не двигаем.
        if let Some((axis, direction)) = cue_shift_axis(&container.children[i]) {
            final_rect = settle_box(start, bounds, placed.as_slice(), axis, direction);
        }
        placed.push(final_rect);
        if final_rect != start {
            let (dx, dy) = (final_rect.x - start.x, final_rect.y - start.y);
            super::shapes_floats::shift_tree(&mut container.children[i], dx, dy);
        }
    }
}

/// Расставляет один cue-бокс: повторяет сдвиг по оси блока на размер
/// пересёкшегося бокса до отсутствия пересечений; выход за `bounds`
/// разворачивает направление один раз (`switched`), второй выход
/// возвращает бокс на исходное место («иначе оставить на месте»).
fn settle_box(
    start: Rect,
    bounds: Rect,
    placed: &[Rect],
    axis: BlockAxis,
    initial_direction: f32,
) -> Rect {
    let mut direction = initial_direction;
    let mut switched = false;
    let mut current = start;
    loop {
        let Some(hit) = placed.iter().copied().find(|r| overlaps(*r, current)) else {
            return current;
        };
        // Шаг сдвига — размер пересёкшегося бокса по оси блока
        // (§7.2: «the height of the overlapping box»).
        let step = match axis {
            BlockAxis::X => hit.width,
            BlockAxis::Y => hit.height,
        };
        if step <= 0.0 {
            // Вырожденный бокс (нулевая высота): сдвиг не сдвинет —
            // оставляем как есть, чтобы не зациклиться.
            return current;
        }
        current = shifted(current, axis, direction * step);
        if !inside(bounds, current) {
            if switched {
                // §7.2: повторный выход за область видео — бокс
                // возвращается на исходное место.
                return start;
            }
            // §7.2: первый выход — флаг switched, направление
            // меняется на противоположное от исходной позиции.
            switched = true;
            direction = -direction;
            current = start;
        }
    }
}

/// Ось блока и направление сдвига cue-бокса по его computed-геометрии.
///
/// `lumen_dom::cue_box_css` пишет блочное смещение в `lh`-единицах:
/// `line >= 0` — в block-start inset (`top`/`left`/`right` по
/// `writing-mode`), `line < 0` и `auto` — в block-end inset; направление
/// сдвига — «в сторону block-end» для первого случая, «в сторону
/// block-start» для второго. `line` в процентах означает
/// `snapToLines === false` — `None` (сдвигов нет).
fn cue_shift_axis(b: &LayoutBox) -> Option<(BlockAxis, f32)> {
    let s = &*b.style;
    let lh = |v: &LengthOrAuto| matches!(v, LengthOrAuto::Length(Length::Lh(_)));
    match s.writing_mode {
        WritingMode::HorizontalTb => {
            // Ось блока — вертикаль, block-start = top. Порядок проверок
            // важен: `top: Nlh` и `bottom: Mlh` одновременно не встречаются
            // в геометрии от `cue_box_css`; первым проверяем block-start.
            if lh(&s.top) {
                Some((BlockAxis::Y, 1.0))
            } else if lh(&s.bottom) {
                Some((BlockAxis::Y, -1.0))
            } else {
                None
            }
        }
        // Ось блока — горизонталь; у vertical-lr/sideways-lr block-start =
        // left (блок наращивается вправо), у vertical-rl/sideways-rl —
        // right (влево).
        WritingMode::VerticalLr | WritingMode::SidewaysLr => {
            if lh(&s.left) {
                Some((BlockAxis::X, 1.0))
            } else if lh(&s.right) {
                Some((BlockAxis::X, -1.0))
            } else {
                None
            }
        }
        WritingMode::VerticalRl | WritingMode::SidewaysRl => {
            if lh(&s.right) {
                Some((BlockAxis::X, -1.0))
            } else if lh(&s.left) {
                Some((BlockAxis::X, 1.0))
            } else {
                None
            }
        }
    }
}

/// Пересечение интерьеров боксов (касание рёбрами не считается — после
/// сдвига на высоту пересёкшегося бокса его нижняя грань совпадает с
/// верхней гранью сдвинутого, что §7.2 раскладкой допускает).
fn overlaps(a: Rect, b: Rect) -> bool {
    a.x < b.x + b.width
        && b.x < a.x + a.width
        && a.y < b.y + b.height
        && b.y < a.y + a.height
}

/// `r` целиком внутри `bounds` (область видео — `title area` §7.2).
fn inside(bounds: Rect, r: Rect) -> bool {
    r.x >= bounds.x
        && r.y >= bounds.y
        && r.x + r.width <= bounds.x + bounds.width
        && r.y + r.height <= bounds.y + bounds.height
}

/// Сдвиг `r` по физической оси блока на `delta` (в пикселях оси).
fn shifted(r: Rect, axis: BlockAxis, delta: f32) -> Rect {
    match axis {
        BlockAxis::X => Rect::new(r.x + delta, r.y, r.width, r.height),
        BlockAxis::Y => Rect::new(r.x, r.y + delta, r.width, r.height),
    }
}
