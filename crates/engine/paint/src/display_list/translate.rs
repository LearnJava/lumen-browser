//! Сдвиг команды display list на `(dx, dy)` — нужен CPU-растру, когда группа
//! `PushTransform` рисуется в слой с началом не в `(0, 0)` (BUG-1288).

use super::*;

fn mv(r: &mut Rect, dx: f32, dy: f32) {
    r.x += dx;
    r.y += dy;
}

fn mv_opt(r: &mut Option<Rect>, dx: f32, dy: f32) {
    if let Some(r) = r {
        mv(r, dx, dy);
    }
}

fn mv_pts(pts: &mut [[f32; 2]], dx: f32, dy: f32) {
    for p in pts {
        p[0] += dx;
        p[1] += dy;
    }
}

impl DisplayCommand {
    /// Копия команды, у которой вся абсолютная геометрия сдвинута на `(dx, dy)`.
    ///
    /// Матрица вложенного `PushTransform` сопрягается сдвигом
    /// (`T(dx,dy) · M · T(-dx,-dy)`): и родительская система, и содержимое
    /// сдвигаются на один и тот же вектор. Относительные величины (проценты
    /// градиентов, `position` фона, inset-ы sticky) не меняются.
    pub(crate) fn translated(&self, dx: f32, dy: f32) -> DisplayCommand {
        let mut c = self.clone();
        c.translate_in_place(dx, dy);
        c
    }

    fn translate_in_place(&mut self, dx: f32, dy: f32) {
        match self {
            Self::FillRect { rect, .. }
            | Self::FillRoundedRect { rect, .. }
            | Self::DrawBorder { rect, .. }
            | Self::DrawOutline { rect, .. }
            | Self::DrawText { rect, .. }
            | Self::DrawTextStroke { rect, .. }
            | Self::DrawImage { rect, .. }
            | Self::LazyImageSlot { rect, .. }
            | Self::DrawLinearGradient { rect, .. }
            | Self::DrawRadialGradient { rect, .. }
            | Self::DrawConicGradient { rect, .. }
            | Self::PushClipRect { rect }
            | Self::PushClipRoundedRect { rect, .. }
            | Self::DrawLayerSnapshot { rect, .. }
            | Self::PushMaskImage { rect, .. }
            | Self::PushMaskLinearGradient { rect, .. }
            | Self::PushMaskRadialGradient { rect, .. }
            | Self::PushMaskConicGradient { rect, .. }
            | Self::PushMaskLayer { rect, .. } => mv(rect, dx, dy),
            Self::DrawBackgroundImage { rect, origin_rect, .. } => {
                mv(rect, dx, dy);
                mv(origin_rect, dx, dy);
            }
            Self::PushClipPath { shape } => match shape {
                ResolvedClipShape::Circle { cx, cy, .. }
                | ResolvedClipShape::Ellipse { cx, cy, .. } => {
                    *cx += dx;
                    *cy += dy;
                }
                ResolvedClipShape::Polygon { verts, .. } => {
                    for (x, y) in verts {
                        *x += dx;
                        *y += dy;
                    }
                }
            },
            Self::PushOpacity { bounds, .. } | Self::PushFilter { bounds, .. } => {
                mv_opt(bounds, dx, dy);
            }
            Self::PushBlendMode { bounds, .. } | Self::PushBackdropFilter { bounds, .. } => {
                mv(bounds, dx, dy);
            }
            Self::PushTransform { matrix } => {
                *matrix = Mat4::translation_2d(dx, dy)
                    .multiply(matrix)
                    .multiply(&Mat4::translation_2d(-dx, -dy));
            }
            Self::BeginStickyLayer { flow_rect, .. } => mv(flow_rect, dx, dy),
            Self::PushScrollLayer { clip_rect, .. } => mv(clip_rect, dx, dy),
            Self::DrawSvgPath { vertices, .. } => mv_pts(vertices, dx, dy),
            Self::DrawSvgFill { contours, .. } | Self::DrawSvgStroke { contours, .. } => {
                for contour in contours {
                    mv_pts(contour, dx, dy);
                }
            }
            Self::BoxModelOverlay { margin, border, padding, content } => {
                mv(margin, dx, dy);
                mv(border, dx, dy);
                mv(padding, dx, dy);
                mv(content, dx, dy);
            }
            Self::DrawScrollbar { track_rect, thumb_rect, .. } => {
                mv(track_rect, dx, dy);
                mv(thumb_rect, dx, dy);
            }
            Self::DrawCrossFade { dest, .. } => mv(dest, dx, dy),
            Self::DrawBorderImage { area, .. } => mv(area, dx, dy),
            Self::PopClip
            | Self::PopOpacity
            | Self::PopBlendMode
            | Self::PopMask
            | Self::PopMaskLayer
            | Self::PopTransform
            | Self::PopFilter
            | Self::PopBackdropFilter
            | Self::EndStickyLayer
            | Self::BeginFixedLayer
            | Self::EndFixedLayer
            | Self::BeginFixedBackground
            | Self::EndFixedBackground
            | Self::PopScrollLayer
            | Self::PageBreak => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_and_nested_matrix_shift() {
        let c = DisplayCommand::FillRect {
            rect: Rect::new(10.0, 20.0, 5.0, 6.0),
            color: Color { r: 1, g: 2, b: 3, a: 255 },
        };
        let DisplayCommand::FillRect { rect, .. } = c.translated(-4.0, 3.0) else {
            panic!("variant changed");
        };
        assert_eq!((rect.x, rect.y, rect.width, rect.height), (6.0, 23.0, 5.0, 6.0));

        // Образ точки q вложенной группы: p = M·q сдвигается на d, а q — тоже на d.
        let m = Mat4::scale_2d(2.0, 3.0);
        let DisplayCommand::PushTransform { matrix } =
            (DisplayCommand::PushTransform { matrix: m }).translated(-10.0, -20.0)
        else {
            panic!("variant changed");
        };
        let (px, py) = m.transform_point_2d(7.0, 9.0);
        let (nx, ny) = matrix.transform_point_2d(7.0 - 10.0, 9.0 - 20.0);
        assert!((nx - (px - 10.0)).abs() < 1e-4 && (ny - (py - 20.0)).abs() < 1e-4);
    }
}
