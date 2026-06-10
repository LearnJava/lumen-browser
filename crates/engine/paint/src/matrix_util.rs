//! Извлечение 2D-аффинной части из `Mat4` — общая утилита бэкендов.
//!
//! До PA-1 распаковка column-major `Mat4` в 6 аффинных компонент была
//! продублирована в `backends/femtovg_backend.rs` (PushTransform),
//! `cpu_raster.rs` (tiny-skia `Transform::from_row`) и `display_list.rs`
//! (сериализация) — см. docs/paint-pipeline-review-2026-06.md, Key finding 2.

use lumen_layout::Mat4;

/// Извлекает 2D-аффинные компоненты `[a, b, c, d, e, f]` из column-major
/// `Mat4`.
///
/// Семантика: `x' = a·x + c·y + e`, `y' = b·x + d·y + f` (CSS Transforms L1
/// `matrix(a, b, c, d, e, f)`). Раскладка `Mat4` column-major:
/// `a = m[0], b = m[1], c = m[4], d = m[5], e = m[12], f = m[13]`.
/// 3D-составляющие (z-строки/столбцы) отбрасываются — вызывающий код обязан
/// заранее проверить `Mat4::is_2d_affine`, если 3D-вход недопустим.
#[must_use]
pub fn mat4_to_2d_affine(m: &Mat4) -> [f32; 6] {
    let m = &m.0;
    [m[0], m[1], m[4], m[5], m[12], m[13]]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_extracts_unit_affine() {
        let [a, b, c, d, e, f] = mat4_to_2d_affine(&Mat4::IDENTITY);
        assert_eq!([a, b, c, d, e, f], [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
    }

    #[test]
    fn translation_lands_in_e_f() {
        let m = Mat4::translation_2d(50.0, -70.0);
        let [a, b, c, d, e, f] = mat4_to_2d_affine(&m);
        assert_eq!([a, b, c, d], [1.0, 0.0, 0.0, 1.0]);
        assert_eq!([e, f], [50.0, -70.0]);
    }

    #[test]
    fn scale_lands_in_a_d() {
        let m = Mat4::scale_2d(2.0, 0.5);
        let [a, b, c, d, e, f] = mat4_to_2d_affine(&m);
        assert_eq!([a, d], [2.0, 0.5]);
        assert_eq!([b, c, e, f], [0.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn from_2d_affine_round_trips() {
        let m = Mat4::from_2d_affine(2.0, 0.1, -0.1, 0.5, 10.0, -20.0);
        assert_eq!(mat4_to_2d_affine(&m), [2.0, 0.1, -0.1, 0.5, 10.0, -20.0]);
    }
}
