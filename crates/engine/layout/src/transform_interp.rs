//! CSS Transforms L2 §15 — интерполяция `transform` между двумя значениями, заданными
//! строками (BUG-1293).
//!
//! Вход и выход — текст CSS: так функцию зовёт JS-слой (`_wa_motion_try`), у которого
//! есть только текст обоих концов. Результат — resolved value (`matrix()` / `matrix3d()` /
//! `none`), то есть то, что отдаёт `getComputedStyle().transform`.
//!
//! Алгоритм повторяет Blink (`TransformOperations::Blend`): общий префикс списков, чьи
//! функции одного семейства (`translate*`, `scale*`, `rotate*`, `skew*`, `perspective`,
//! `matrix*`), интерполируется функция с функцией; недостающая функция более короткого
//! списка — единичная того же семейства (так `none → rotate(90deg)` вращает с 0°, а не по
//! кратчайшему пути матрицы). Остаток списков складывается в две матрицы, они
//! декомпозируются (§15.2, 3D unmatrix) и интерполируются покомпонентно; кватернионы — по
//! slerp. Если матрица не раскладывается, пара не интерполируется: `None`, вызывающий
//! делает дискретный перелом.

use crate::property_trees::{compute_local_transform, Mat4, TransformRef};
use crate::style::{Length, TransformFn};

/// Семейство функции: пары одного семейства интерполируются функция с функцией.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Family {
    Translate,
    Scale,
    Rotate,
    Skew,
    Perspective,
    Matrix,
}

fn family(f: &TransformFn) -> Family {
    use TransformFn::*;
    match f {
        Translate(..) | TranslateX(_) | TranslateY(_) | TranslateZ(_) | Translate3d(..) | TranslateLp(..) => {
            Family::Translate
        }
        Scale(..) | ScaleX(_) | ScaleY(_) | ScaleZ(_) | Scale3d(..) => Family::Scale,
        Rotate(_) | RotateX(_) | RotateY(_) | RotateZ(_) | Rotate3d(..) => Family::Rotate,
        SkewX(_) | SkewY(_) => Family::Skew,
        Perspective(_) => Family::Perspective,
        Matrix(_) | Matrix3d(_) => Family::Matrix,
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// `translate*()` как тройка (x, y, z): x и y — `<length-percentage>`.
fn translate_parts(f: Option<&TransformFn>) -> (Length, Length, f32) {
    use TransformFn::*;
    match f {
        Some(Translate(x, y)) => (Length::Px(*x), Length::Px(*y), 0.0),
        Some(TranslateX(x)) => (Length::Px(*x), Length::Px(0.0), 0.0),
        Some(TranslateY(y)) => (Length::Px(0.0), Length::Px(*y), 0.0),
        Some(TranslateZ(z)) => (Length::Px(0.0), Length::Px(0.0), *z),
        Some(Translate3d(x, y, z)) => (Length::Px(*x), Length::Px(*y), *z),
        Some(TranslateLp(x, y, z)) => (x.clone(), y.clone(), *z),
        _ => (Length::Px(0.0), Length::Px(0.0), 0.0),
    }
}

/// `scale*()` как тройка (sx, sy, sz).
fn scale_parts(f: Option<&TransformFn>) -> [f32; 3] {
    use TransformFn::*;
    match f {
        Some(Scale(x, y)) => [*x, *y, 1.0],
        Some(ScaleX(x)) => [*x, 1.0, 1.0],
        Some(ScaleY(y)) => [1.0, *y, 1.0],
        Some(ScaleZ(z)) => [1.0, 1.0, *z],
        Some(Scale3d(x, y, z)) => [*x, *y, *z],
        _ => [1.0, 1.0, 1.0],
    }
}

/// `rotate*()` как ось и угол (радианы).
fn rotate_parts(f: Option<&TransformFn>) -> ([f32; 3], f32) {
    use TransformFn::*;
    match f {
        Some(Rotate(a)) | Some(RotateZ(a)) => ([0.0, 0.0, 1.0], *a),
        Some(RotateX(a)) => ([1.0, 0.0, 0.0], *a),
        Some(RotateY(a)) => ([0.0, 1.0, 0.0], *a),
        Some(Rotate3d(x, y, z, a)) => ([*x, *y, *z], *a),
        _ => ([0.0, 0.0, 1.0], 0.0),
    }
}

fn skew_parts(f: Option<&TransformFn>) -> (f32, f32) {
    match f {
        Some(TransformFn::SkewX(a)) => (*a, 0.0),
        Some(TransformFn::SkewY(a)) => (0.0, *a),
        _ => (0.0, 0.0),
    }
}

fn normalize3(v: [f32; 3]) -> Option<[f32; 3]> {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    (len > 0.0 && len.is_finite()).then(|| [v[0] / len, v[1] / len, v[2] / len])
}

fn matrix_of(f: &TransformFn, r: TransformRef) -> Mat4 {
    compute_local_transform(std::slice::from_ref(f), (0.0, 0.0, 0.0), r)
}

fn matrix_of_list(fns: &[TransformFn], r: TransformRef) -> Mat4 {
    compute_local_transform(fns, (0.0, 0.0, 0.0), r)
}

/// Интерполяция пары функций одного семейства (`None` — единичная функция).
fn blend_pair(a: Option<&TransformFn>, b: Option<&TransformFn>, t: f32, r: TransformRef) -> Option<Mat4> {
    let fam = family(a.or(b)?);
    match fam {
        Family::Translate => {
            let (ax, ay, az) = translate_parts(a);
            let (bx, by, bz) = translate_parts(b);
            let f = TransformFn::TranslateLp(
                crate::animation::blend_length(&ax, &bx, t),
                crate::animation::blend_length(&ay, &by, t),
                lerp(az, bz, t),
            );
            Some(matrix_of(&f, r))
        }
        Family::Scale => {
            let (sa, sb) = (scale_parts(a), scale_parts(b));
            Some(matrix_of(
                &TransformFn::Scale3d(lerp(sa[0], sb[0], t), lerp(sa[1], sb[1], t), lerp(sa[2], sb[2], t)),
                r,
            ))
        }
        Family::Skew => {
            let ((ax, ay), (bx, by)) = (skew_parts(a), skew_parts(b));
            let sx = Mat4::skew_x(lerp(ax, bx, t));
            let sy = Mat4::skew_y(lerp(ay, by, t));
            Some(sx.multiply(&sy))
        }
        Family::Perspective => {
            // §15.2: перспектива интерполируется по обратной величине (1/d).
            let inv = |f: Option<&TransformFn>| match f {
                Some(TransformFn::Perspective(d)) if *d > 0.0 => 1.0 / *d,
                _ => 0.0,
            };
            let p = lerp(inv(a), inv(b), t);
            // No perspective (`none`, or an overshoot below zero): the identity, not 1/0.
            Some(if p > 0.0 { Mat4::perspective(1.0 / p) } else { Mat4::IDENTITY })
        }
        Family::Rotate => {
            let ((axa, aa), (axb, ab)) = (rotate_parts(a), rotate_parts(b));
            // Нулевой угол не задаёт оси: берётся ось другого конца.
            let (axa, axb) = match (aa == 0.0, ab == 0.0) {
                (true, false) => (axb, axb),
                (false, true) => (axa, axa),
                _ => (axa, axb),
            };
            let (na, nb) = (normalize3(axa)?, normalize3(axb)?);
            if na == nb {
                return Some(Mat4::rotate_3d(na[0], na[1], na[2], lerp(aa, ab, t)));
            }
            let ma = Mat4::rotate_3d(na[0], na[1], na[2], aa);
            let mb = Mat4::rotate_3d(nb[0], nb[1], nb[2], ab);
            blend_matrices(&ma, &mb, t)
        }
        Family::Matrix => {
            let ma = a.map(|f| matrix_of(f, r)).unwrap_or(Mat4::IDENTITY);
            let mb = b.map(|f| matrix_of(f, r)).unwrap_or(Mat4::IDENTITY);
            blend_matrices(&ma, &mb, t)
        }
    }
}

// ─── декомпозиция 4×4 (CSS Transforms L2 §15.2) ─────────────────────────────

/// Разложенная матрица: перенос, масштаб, скос (xy, xz, yz), перспектива, кватернион.
struct Decomposed {
    translate: [f64; 3],
    scale: [f64; 3],
    skew: [f64; 3],
    perspective: [f64; 4],
    quat: [f64; 4],
}

fn dot3(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn combine3(a: [f64; 3], b: [f64; 3], sa: f64, sb: f64) -> [f64; 3] {
    [a[0] * sa + b[0] * sb, a[1] * sa + b[1] * sb, a[2] * sa + b[2] * sb]
}

fn len3(a: [f64; 3]) -> f64 {
    dot3(a, a).sqrt()
}

fn scale3(a: [f64; 3], k: f64) -> [f64; 3] {
    [a[0] * k, a[1] * k, a[2] * k]
}

/// Определитель 4×4 (column-major, любая раскладка: det(Mᵀ) = det(M)).
fn det4(m: &[f64; 16]) -> f64 {
    let s0 = m[0] * m[5] - m[4] * m[1];
    let s1 = m[0] * m[9] - m[8] * m[1];
    let s2 = m[0] * m[13] - m[12] * m[1];
    let s3 = m[4] * m[9] - m[8] * m[5];
    let s4 = m[4] * m[13] - m[12] * m[5];
    let s5 = m[8] * m[13] - m[12] * m[9];
    let c5 = m[10] * m[15] - m[14] * m[11];
    let c4 = m[6] * m[15] - m[14] * m[7];
    let c3 = m[6] * m[11] - m[10] * m[7];
    let c2 = m[2] * m[15] - m[14] * m[3];
    let c1 = m[2] * m[11] - m[10] * m[3];
    let c0 = m[2] * m[7] - m[6] * m[3];
    s0 * c5 - s1 * c4 + s2 * c3 + s3 * c2 - s4 * c1 + s5 * c0
}

/// Обратная к 4×4 (column-major) или `None` для вырожденной.
fn inverse4(m: &[f64; 16]) -> Option<[f64; 16]> {
    let s0 = m[0] * m[5] - m[4] * m[1];
    let s1 = m[0] * m[9] - m[8] * m[1];
    let s2 = m[0] * m[13] - m[12] * m[1];
    let s3 = m[4] * m[9] - m[8] * m[5];
    let s4 = m[4] * m[13] - m[12] * m[5];
    let s5 = m[8] * m[13] - m[12] * m[9];
    let c5 = m[10] * m[15] - m[14] * m[11];
    let c4 = m[6] * m[15] - m[14] * m[7];
    let c3 = m[6] * m[11] - m[10] * m[7];
    let c2 = m[2] * m[15] - m[14] * m[3];
    let c1 = m[2] * m[11] - m[10] * m[3];
    let c0 = m[2] * m[7] - m[6] * m[3];
    let det = s0 * c5 - s1 * c4 + s2 * c3 + s3 * c2 - s4 * c1 + s5 * c0;
    if det.abs() < 1e-12 || !det.is_finite() {
        return None;
    }
    let id = 1.0 / det;
    Some([
        (m[5] * c5 - m[9] * c4 + m[13] * c3) * id,
        (-m[1] * c5 + m[9] * c2 - m[13] * c1) * id,
        (m[1] * c4 - m[5] * c2 + m[13] * c0) * id,
        (-m[1] * c3 + m[5] * c1 - m[9] * c0) * id,
        (-m[4] * c5 + m[8] * c4 - m[12] * c3) * id,
        (m[0] * c5 - m[8] * c2 + m[12] * c1) * id,
        (-m[0] * c4 + m[4] * c2 - m[12] * c0) * id,
        (m[0] * c3 - m[4] * c1 + m[8] * c0) * id,
        (m[7] * s5 - m[11] * s4 + m[15] * s3) * id,
        (-m[3] * s5 + m[11] * s2 - m[15] * s1) * id,
        (m[3] * s4 - m[7] * s2 + m[15] * s0) * id,
        (-m[3] * s3 + m[7] * s1 - m[11] * s0) * id,
        (-m[6] * s5 + m[10] * s4 - m[14] * s3) * id,
        (m[2] * s5 - m[10] * s2 + m[14] * s1) * id,
        (-m[2] * s4 + m[6] * s2 - m[14] * s0) * id,
        (m[2] * s3 - m[6] * s1 + m[10] * s0) * id,
    ])
}

fn decompose(src: &Mat4) -> Option<Decomposed> {
    let mut m: [f64; 16] = src.0.map(f64::from);
    if m.iter().any(|v| !v.is_finite()) || m[15] == 0.0 {
        return None;
    }
    let w = m[15];
    for v in &mut m {
        *v /= w;
    }
    let mut pm = m;
    pm[3] = 0.0;
    pm[7] = 0.0;
    pm[11] = 0.0;
    pm[15] = 1.0;
    if det4(&pm).abs() < 1e-12 {
        return None;
    }
    let perspective = if m[3] != 0.0 || m[7] != 0.0 || m[11] != 0.0 {
        // perspective = transpose(inverse(pm)) · rhs, rhs = (m3, m7, m11, m15).
        let inv = inverse4(&pm)?;
        let rhs = [m[3], m[7], m[11], m[15]];
        // transpose(inv) · rhs: компонент j = Σ_i inv[j*4+i] * rhs[i] (column-major).
        let mut p = [0.0; 4];
        for (j, pj) in p.iter_mut().enumerate() {
            *pj = (0..4).map(|i| inv[j * 4 + i] * rhs[i]).sum();
        }
        p
    } else {
        [0.0, 0.0, 0.0, 1.0]
    };
    let translate = [m[12], m[13], m[14]];
    // row[i] — i-й столбец верхнего 3×3 (так в спецификации: matrix[i][0..2]).
    let mut row = [
        [m[0], m[1], m[2]],
        [m[4], m[5], m[6]],
        [m[8], m[9], m[10]],
    ];
    let mut scale = [0.0; 3];
    let mut skew = [0.0; 3];
    scale[0] = len3(row[0]);
    row[0] = scale3(row[0], 1.0 / scale[0]);
    skew[0] = dot3(row[0], row[1]);
    row[1] = combine3(row[1], row[0], 1.0, -skew[0]);
    scale[1] = len3(row[1]);
    row[1] = scale3(row[1], 1.0 / scale[1]);
    skew[0] /= scale[1];
    skew[1] = dot3(row[0], row[2]);
    row[2] = combine3(row[2], row[0], 1.0, -skew[1]);
    skew[2] = dot3(row[1], row[2]);
    row[2] = combine3(row[2], row[1], 1.0, -skew[2]);
    scale[2] = len3(row[2]);
    row[2] = scale3(row[2], 1.0 / scale[2]);
    skew[1] /= scale[2];
    skew[2] /= scale[2];
    if dot3(row[0], cross3(row[1], row[2])) < 0.0 {
        for k in 0..3 {
            scale[k] = -scale[k];
            row[k] = scale3(row[k], -1.0);
        }
    }
    if scale.iter().any(|s| !s.is_finite() || *s == 0.0) {
        return None;
    }
    let mut q = [
        0.5 * (1.0 + row[0][0] - row[1][1] - row[2][2]).max(0.0).sqrt(),
        0.5 * (1.0 - row[0][0] + row[1][1] - row[2][2]).max(0.0).sqrt(),
        0.5 * (1.0 - row[0][0] - row[1][1] + row[2][2]).max(0.0).sqrt(),
        0.5 * (1.0 + row[0][0] + row[1][1] + row[2][2]).max(0.0).sqrt(),
    ];
    if row[2][1] > row[1][2] {
        q[0] = -q[0];
    }
    if row[0][2] > row[2][0] {
        q[1] = -q[1];
    }
    if row[1][0] > row[0][1] {
        q[2] = -q[2];
    }
    Some(Decomposed { translate, scale, skew, perspective, quat: q })
}

fn slerp(a: [f64; 4], b: [f64; 4], t: f64) -> [f64; 4] {
    let mut dot: f64 = (0..4).map(|i| a[i] * b[i]).sum();
    dot = dot.clamp(-1.0, 1.0);
    if dot > 0.9995 {
        let mut o = [0.0; 4];
        for i in 0..4 {
            o[i] = a[i] + (b[i] - a[i]) * t;
        }
        let n: f64 = o.iter().map(|v| v * v).sum::<f64>().sqrt();
        return if n > 0.0 { o.map(|v| v / n) } else { a };
    }
    let theta = dot.acos();
    let (s0, s1) = (((1.0 - t) * theta).sin() / theta.sin(), (t * theta).sin() / theta.sin());
    let mut o = [0.0; 4];
    for i in 0..4 {
        o[i] = a[i] * s0 + b[i] * s1;
    }
    o
}

fn recompose(d: &Decomposed) -> Mat4 {
    let mul = |a: &[f64; 16], b: &[f64; 16]| -> [f64; 16] {
        let mut o = [0.0; 16];
        for c in 0..4 {
            for r in 0..4 {
                o[c * 4 + r] = (0..4).map(|k| a[k * 4 + r] * b[c * 4 + k]).sum();
            }
        }
        o
    };
    let mut m: [f64; 16] = [
        1.0, 0.0, 0.0, 0.0, //
        0.0, 1.0, 0.0, 0.0, //
        0.0, 0.0, 1.0, 0.0, //
        0.0, 0.0, 0.0, 1.0,
    ];
    m[3] = d.perspective[0];
    m[7] = d.perspective[1];
    m[11] = d.perspective[2];
    m[15] = d.perspective[3];
    let mut tr = [
        1.0, 0.0, 0.0, 0.0, //
        0.0, 1.0, 0.0, 0.0, //
        0.0, 0.0, 1.0, 0.0, //
        0.0, 0.0, 0.0, 1.0,
    ];
    tr[12] = d.translate[0];
    tr[13] = d.translate[1];
    tr[14] = d.translate[2];
    m = mul(&m, &tr);
    let [x, y, z, w] = d.quat;
    let rot = [
        1.0 - 2.0 * (y * y + z * z),
        2.0 * (x * y + z * w),
        2.0 * (x * z - y * w),
        0.0,
        2.0 * (x * y - z * w),
        1.0 - 2.0 * (x * x + z * z),
        2.0 * (y * z + x * w),
        0.0,
        2.0 * (x * z + y * w),
        2.0 * (y * z - x * w),
        1.0 - 2.0 * (x * x + y * y),
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
    ];
    m = mul(&m, &rot);
    let ident = [
        1.0, 0.0, 0.0, 0.0, //
        0.0, 1.0, 0.0, 0.0, //
        0.0, 0.0, 1.0, 0.0, //
        0.0, 0.0, 0.0, 1.0,
    ];
    if d.skew[2] != 0.0 {
        let mut s = ident;
        s[9] = d.skew[2]; // [col 2][row 1]
        m = mul(&m, &s);
    }
    if d.skew[1] != 0.0 {
        let mut s = ident;
        s[8] = d.skew[1]; // [col 2][row 0]
        m = mul(&m, &s);
    }
    if d.skew[0] != 0.0 {
        let mut s = ident;
        s[4] = d.skew[0]; // [col 1][row 0]
        m = mul(&m, &s);
    }
    let mut sc = ident;
    sc[0] = d.scale[0];
    sc[5] = d.scale[1];
    sc[10] = d.scale[2];
    m = mul(&m, &sc);
    Mat4(m.map(|v| v as f32))
}

/// Интерполяция двух матриц через декомпозицию; `None` — одна из них не раскладывается.
fn blend_matrices(a: &Mat4, b: &Mat4, t: f32) -> Option<Mat4> {
    if a == b {
        return Some(*a);
    }
    let (da, db) = (decompose(a)?, decompose(b)?);
    let t = f64::from(t);
    let mix = |x: f64, y: f64| x + (y - x) * t;
    let d = Decomposed {
        translate: [0, 1, 2].map(|i| mix(da.translate[i], db.translate[i])),
        scale: [0, 1, 2].map(|i| mix(da.scale[i], db.scale[i])),
        skew: [0, 1, 2].map(|i| mix(da.skew[i], db.skew[i])),
        perspective: [0, 1, 2, 3].map(|i| mix(da.perspective[i], db.perspective[i])),
        quat: slerp(da.quat, db.quat, t),
    };
    Some(recompose(&d))
}

fn parse(s: &str) -> Option<Vec<TransformFn>> {
    let s = s.trim();
    if s.eq_ignore_ascii_case("none") {
        return Some(Vec::new());
    }
    let list = crate::style::parse_transform_list(s);
    (!list.is_empty()).then_some(list)
}

/// Resolved value матрицы, как её отдаёт `getComputedStyle()`.
fn format_matrix(m: &Mat4) -> String {
    // Шум округления f64 (1e-9 у `x`/`y` кватерниона 2D-поворота) не должен превращать
    // плоскую матрицу в `matrix3d()`: привязка к нулю идёт до проверки плоскости.
    let a = m.0.map(|v| if v.abs() < 1e-6 { 0.0 } else { v });
    let num = |v: f32| v.to_string();
    let flat = a[2] == 0.0
        && a[3] == 0.0
        && a[6] == 0.0
        && a[7] == 0.0
        && a[8] == 0.0
        && a[9] == 0.0
        && a[10] == 1.0
        && a[11] == 0.0
        && a[14] == 0.0
        && a[15] == 1.0;
    let pick: &[usize] = if flat { &[0, 1, 4, 5, 12, 13] } else { &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15] };
    let body = pick.iter().map(|&i| num(a[i])).collect::<Vec<_>>().join(", ");
    format!("{}({body})", if flat { "matrix" } else { "matrix3d" })
}

/// Resolved value одного значения `transform` (`rotate(30deg)` → `matrix(…)`, `none`).
/// `w`/`h` — border box элемента для процентов в `translate*()`. `None` — не разобралось.
pub fn resolve_transform_value(value: &str, w: f32, h: f32) -> Option<String> {
    let list = parse(value)?;
    if list.is_empty() {
        return Some("none".to_string());
    }
    let r = TransformRef { width: w, height: h, em: 16.0 };
    Some(format_matrix(&matrix_of_list(&list, r)))
}

/// Интерполирует `from` → `to` при прогрессе `t`; `w`/`h` — border box элемента для
/// процентов. `None` — конец не разобрался или матрицы не раскладываются.
pub fn interpolate_transform_value(from: &str, to: &str, t: f64, w: f32, h: f32) -> Option<String> {
    let (a, b) = (parse(from)?, parse(to)?);
    if a.is_empty() && b.is_empty() {
        return Some("none".to_string());
    }
    let r = TransformRef { width: w, height: h, em: 16.0 };
    let t = t as f32;
    let n = a.len().max(b.len());
    let mut k = 0;
    while k < n {
        match (a.get(k), b.get(k)) {
            (Some(x), Some(y)) if family(x) != family(y) => break,
            _ => k += 1,
        }
    }
    let mut m = Mat4::IDENTITY;
    for i in 0..k {
        m = m.multiply(&blend_pair(a.get(i), b.get(i), t, r)?);
    }
    if k < n {
        let rest = |l: &[TransformFn]| matrix_of_list(l.get(k..).unwrap_or(&[]), r);
        m = m.multiply(&blend_matrices(&rest(&a), &rest(&b), t)?);
    }
    Some(format_matrix(&m))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nums(s: &str) -> Vec<f32> {
        let body = s.split('(').nth(1).unwrap().trim_end_matches(')');
        body.split(',').map(|v| v.trim().parse().unwrap()).collect()
    }

    fn close(a: &[f32], b: &[f32]) -> bool {
        a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-3)
    }

    #[test]
    fn translate_midpoint() {
        let r = interpolate_transform_value("translateX(0px)", "translateX(100px)", 0.5, 0.0, 0.0).unwrap();
        assert!(close(&nums(&r), &[1.0, 0.0, 0.0, 1.0, 50.0, 0.0]), "{r}");
    }

    #[test]
    fn rotate_from_none_goes_through_the_angle_not_the_shortest_path() {
        let r = interpolate_transform_value("none", "rotate(900deg)", 0.25, 0.0, 0.0).unwrap();
        let a = 225f32.to_radians();
        assert!(close(&nums(&r), &[a.cos(), a.sin(), -a.sin(), a.cos(), 0.0, 0.0]), "{r}");
    }

    #[test]
    fn shorter_list_is_padded_with_identity() {
        let r = interpolate_transform_value("scaleZ(2)", "scaleZ(2) perspective(500px)", 0.5, 0.0, 0.0).unwrap();
        assert!(r.starts_with("matrix3d("), "{r}");
    }

    #[test]
    fn percent_translate_uses_the_box() {
        let r = interpolate_transform_value("translateY(70%)", "translateY(90%)", 0.75, 100.0, 200.0).unwrap();
        assert!(close(&nums(&r), &[1.0, 0.0, 0.0, 1.0, 0.0, 170.0]), "{r}");
    }

    #[test]
    fn singular_matrix_pair_does_not_interpolate() {
        let r = interpolate_transform_value("matrix3d(2,0,0,0,0,2,0,0,0,0,0,0,0,0,0,1)", "matrix(3,0,0,3,0,0)", 0.5, 0.0, 0.0);
        assert!(r.is_none(), "{r:?}");
    }

    #[test]
    fn rotate3d_with_different_axes_slerps() {
        let r = interpolate_transform_value("rotateX(0deg)", "rotateY(900deg)", -1.0, 0.0, 0.0).unwrap();
        assert!(r.starts_with("matrix3d("), "{r}");
    }

    #[test]
    fn serialised_list_round_trips_through_the_interpolator() {
        let list = crate::style::parse_transform_list("translate(200px) rotate(720deg) translateY(70%)");
        let text = crate::selector_query::transform_fns_to_list_css(&list);
        assert_eq!(crate::style::parse_transform_list(&text), list, "{text}");
        let r = interpolate_transform_value("none", &text, 0.25, 0.0, 100.0).unwrap();
        // rotate(180deg) after translate(50px); the 70% of a 100px box moves with it.
        assert!(close(&nums(&r), &[-1.0, 0.0, 0.0, -1.0, 50.0, -17.5]), "{r}");
    }

    #[test]
    fn a_planar_pair_across_families_stays_a_2d_matrix() {
        let r = interpolate_transform_value("translateX(10px)", "rotate(90deg)", 0.5, 0.0, 0.0).unwrap();
        assert!(r.starts_with("matrix("), "{r}");
    }

    #[test]
    fn perspective_shrinking_to_none_never_prints_infinity() {
        for t in [0.0, 1.0, 1.5, -0.5] {
            let r = interpolate_transform_value("none", "perspective(500px)", t, 0.0, 0.0).unwrap();
            assert!(!r.contains("inf") && !r.contains("NaN"), "t={t}: {r}");
            let r = interpolate_transform_value("perspective(500px)", "none", t, 0.0, 0.0).unwrap();
            assert!(!r.contains("inf") && !r.contains("NaN"), "t={t}: {r}");
        }
    }

    #[test]
    fn resolve_collapses_a_list_to_a_matrix() {
        assert_eq!(resolve_transform_value("none", 0.0, 0.0).as_deref(), Some("none"));
        let r = resolve_transform_value("translate(5px, 6px)", 0.0, 0.0).unwrap();
        assert!(close(&nums(&r), &[1.0, 0.0, 0.0, 1.0, 5.0, 6.0]), "{r}");
    }
}
