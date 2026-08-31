use super::*;

/// Сколько вершин держит [`SvgShapeCache`], прежде чем сбросить себя целиком.
/// Та же политика и тот же порядок, что у [`COVERAGE_CACHE_MAX_VERTS`]:
/// страница со статичными иконками занимает единицы килобайт, а сброс нужен
/// патологии — потоку НОВЫХ фигур каждый кадр.
const SVG_SHAPE_CACHE_MAX_VERTS: usize = 1 << 20;

/// Готовая фигура SVG: `[x, y, z, cov]` в CSS px, без цвета. Цвет домножается
/// на `cov` при укладке в вершины, поэтому одна и та же геометрия обслуживает
/// иконки любого цвета.
type SvgShapeVerts = std::sync::Arc<Vec<[f32; 4]>>;

/// Мемоизация ЦЕЛОЙ команды SVG-супа (BUG-405 срез 12) — тесселяции, укладки,
/// матрицы и сглаживания разом.
///
/// Срез 9 закэшировал последний шаг команды (`coverage_quads`), и разбивка
/// `svg-sub` показала, что осталось: из 17.9 мкс команды `DrawSvgStroke` на
/// прокрутке `lenta.ru` 8.3 мкс — тесселяция, 6.5 — укладка 340 вершин супа со
/// сдвигом и матрицей, 1.5 — хэш и побитовое сравнение ключа ИЗ ЭТИХ 340
/// вершин, 0.0 — сам пересчёт покрытия (кэш попадает). То есть 90% работы
/// команды существует ради вычисления ключа к уже посчитанному ответу.
///
/// Ключ здесь — ВХОД команды (10.9 точки контуров на команду против 340 вершин
/// супа), а не её промежуточный результат: контуры, параметры обводки, сдвиг,
/// накопленная матрица, `dpr` и флаг сглаживания. Всё, что влияет на вершины,
/// в ключе есть, поэтому попадание возвращает ровно те вершины, которые вернул
/// бы пересчёт, и сравнение — побитовое (`f32::to_bits`), как в срезе 9:
/// коллизия хэша даёт промах, а не чужие пиксели.
#[derive(Default)]
pub(crate) struct SvgShapeCache {
    /// Хэш ключа → ключи с таким хэшом и посчитанные по ним вершины.
    buckets: std::collections::HashMap<u64, Vec<(Vec<u32>, SvgShapeVerts)>>,
    /// Переиспользуемый буфер под ключ — иначе каждая команда аллоцировала бы.
    scratch: Vec<u32>,
    /// Сколько слов ключей и вершин суммарно хранится.
    stored: usize,
    /// Сколько команд вернули готовую фигуру.
    pub(crate) hits: u64,
    /// Сколько команд пересчитали фигуру.
    pub(crate) misses: u64,
}

impl SvgShapeCache {
    /// Вершины фигуры по ключу: готовые из кэша либо посчитанные `compute`.
    fn shape(&mut self, key: &[u32], compute: impl FnOnce() -> Vec<[f32; 4]>) -> SvgShapeVerts {
        let h = bits_hash(key);
        if let Some(bucket) = self.buckets.get(&h)
            && let Some((_, verts)) = bucket.iter().find(|(k, _)| k.as_slice() == key)
        {
            self.hits += 1;
            return std::sync::Arc::clone(verts);
        }
        self.misses += 1;
        let verts = std::sync::Arc::new(compute());
        let cost = key.len() + verts.len();
        if self.stored + cost > SVG_SHAPE_CACHE_MAX_VERTS {
            self.buckets.clear();
            self.stored = 0;
        }
        self.stored += cost;
        self.buckets.entry(h).or_default().push((key.to_vec(), std::sync::Arc::clone(&verts)));
        verts
    }
}

/// Ключ фигуры SVG для [`SvgShapeCache`] — всё, от чего зависят её вершины,
/// в битовом виде. Пишется в переиспользуемый буфер `out`.
///
/// `stroke` в ключе обязателен: одни и те же контуры под заливкой и под
/// обводкой дают разные супы, а всё остальное у них совпадает.
#[allow(clippy::too_many_arguments)]
fn build_svg_shape_key(
    out: &mut Vec<u32>,
    stroke: bool,
    contours: &[Vec<[f32; 2]>],
    params: Option<&crate::svg_path::StrokeParams>,
    dx: f32,
    dy: f32,
    m: Option<&Mat4>,
    dpr: f32,
    aa: bool,
) {
    out.clear();
    out.push(u32::from(stroke) | (u32::from(aa) << 1));
    out.push(dx.to_bits());
    out.push(dy.to_bits());
    out.push(dpr.to_bits());
    match m {
        Some(m) => {
            out.push(1);
            out.extend(m.0.iter().map(|v| v.to_bits()));
        }
        None => out.push(0),
    }
    match params {
        Some(p) => {
            out.push(1);
            out.push(p.half_width.to_bits());
            out.push(p.linecap as u32);
            out.push(p.linejoin as u32);
            out.push(p.miterlimit.to_bits());
            out.push(p.dashoffset.to_bits());
            out.push(p.dasharray.len() as u32);
            out.extend(p.dasharray.iter().map(|v| v.to_bits()));
        }
        None => out.push(0),
    }
    out.push(contours.len() as u32);
    for c in contours {
        out.push(c.len() as u32);
        for p in c {
            out.push(p[0].to_bits());
            out.push(p[1].to_bits());
        }
    }
}

/// Посчитать вершины фигуры SVG из готового супа — тот же путь, что был до
/// среза 12, только с белым цветом: покрытие остаётся в альфе, а настоящий
/// цвет домножается на него при укладке, поэтому результат от цвета не зависит.
///
/// `1.0 * cov == cov` и `c.a * 1.0 == c.a` точны в IEEE-754, поэтому вершины
/// побитово те же, что дал бы прежний путь с настоящим цветом.
fn compute_svg_shape(
    tris: &[[f32; 2]],
    dx: f32,
    dy: f32,
    m: Option<&Mat4>,
    dpr: f32,
    aa: bool,
    coverage: Option<&mut CoverageCache>,
) -> Vec<[f32; 4]> {
    const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    let t_push = (crate::frame_log_level() >= 3).then(std::time::Instant::now);
    let mut verts: Vec<FillVertex> = tris
        .iter()
        .map(|[x, y]| FillVertex { pos: [x + dx, y + dy], z: 0.0, color: WHITE })
        .collect();
    if let Some(m) = m {
        apply_affine_to_verts(&mut verts, m);
    }
    if let Some(t0) = t_push {
        sub_add(&SVG_SUB.push, t0);
    }
    if aa {
        antialias_fill_soup(&mut verts, 0, WHITE, dpr, coverage);
    }
    verts.iter().map(|v| [v.pos[0], v.pos[1], v.z, v.color[3]]).collect()
}

/// Вершины фигуры одной команды `DrawSvgFill`/`DrawSvgStroke`: готовые из
/// [`SvgShapeCache`] либо посчитанные и запомненные (BUG-405 срез 12).
///
/// `shapes_enabled == false` (инстансный рычаг или `LUMEN_NO_SVG_SHAPE_CACHE=1`)
/// — плечо отката: та же самая функция подсчёта, но без обращения к кэшу,
/// поэтому A/B сравнивает мемоизацию, а не два разных пути укладки.
#[allow(clippy::too_many_arguments)]
pub(crate) fn svg_shape_verts(
    shapes: &mut SvgShapeCache,
    coverage: &mut CoverageCache,
    shapes_enabled: bool,
    coverage_enabled: bool,
    contours: &[Vec<[f32; 2]>],
    params: Option<&crate::svg_path::StrokeParams>,
    dx: f32,
    dy: f32,
    m: Option<&Mat4>,
    dpr: f32,
    cmd_log: bool,
) -> SvgShapeVerts {
    use std::sync::atomic::Ordering::Relaxed;
    let aa = !svg_aa_disabled();
    if cmd_log {
        SVG_SUB.calls.fetch_add(1, Relaxed);
    }
    let tessellate = || {
        let t_tess = cmd_log.then(std::time::Instant::now);
        let tris = match params {
            Some(p) => crate::svg_path::tessellate_stroke_ex(contours, p),
            None => crate::svg_path::tessellate_fill(contours),
        };
        if let Some(t0) = t_tess {
            sub_add(&SVG_SUB.tess, t0);
            count_svg_soup(tris.len(), contours);
        }
        tris
    };
    if !shapes_enabled || svg_shape_cache_disabled() {
        let tris = tessellate();
        let arm = coverage_cache_arm(coverage, coverage_enabled);
        return std::sync::Arc::new(compute_svg_shape(&tris, dx, dy, m, dpr, aa, arm));
    }
    // Буфер ключа одалживается у кэша и возвращается обратно: ключ строится на
    // каждую команду, включая попадающую, и аллокация на команду съела бы часть
    // того, ради чего срез делается.
    let mut key = std::mem::take(&mut shapes.scratch);
    let t_key = cmd_log.then(std::time::Instant::now);
    build_svg_shape_key(&mut key, params.is_some(), contours, params, dx, dy, m, dpr, aa);
    if let Some(t0) = t_key {
        sub_add(&SVG_SUB.key, t0);
    }
    let before = shapes.hits;
    let t_look = cmd_log.then(std::time::Instant::now);
    let verts = shapes.shape(&key, || {
        let tris = tessellate();
        let arm = coverage_cache_arm(coverage, coverage_enabled);
        compute_svg_shape(&tris, dx, dy, m, dpr, aa, arm)
    });
    if let Some(t0) = t_look {
        sub_add(&SVG_SUB.look, t0);
    }
    if cmd_log {
        let slot = if shapes.hits > before { &SVG_SUB.hit } else { &SVG_SUB.miss };
        slot.fetch_add(1, Relaxed);
    }
    shapes.scratch = key;
    verts
}

/// Уложить готовую фигуру в вершины заливки, домножив её покрытие на цвет
/// команды. Цвет — единственное, что фигура не помнит.
pub(crate) fn emit_svg_shape(
    fill_vertices: &mut Vec<FillVertex>,
    shape: &[[f32; 4]],
    color: [f32; 4],
    cmd_log: bool,
) {
    let t_emit = cmd_log.then(std::time::Instant::now);
    fill_vertices.reserve(shape.len());
    for q in shape {
        fill_vertices.push(FillVertex {
            pos: [q[0], q[1]],
            z: q[2],
            color: [color[0], color[1], color[2], color[3] * q[3]],
        });
    }
    if let Some(t0) = t_emit {
        sub_add(&SVG_SUB.emit, t0);
        SVG_SUB.emitv.fetch_add(shape.len() as u64, std::sync::atomic::Ordering::Relaxed);
    }
}

/// `true`, если мемоизация фигур SVG отключена (`LUMEN_NO_SVG_SHAPE_CACHE=1`) —
/// рычаг отката BUG-405 срез 12 к пересчёту фигуры на каждую команду.
fn svg_shape_cache_disabled() -> bool {
    use std::sync::OnceLock;
    static OFF: OnceLock<bool> = OnceLock::new();
    *OFF.get_or_init(|| std::env::var("LUMEN_NO_SVG_SHAPE_CACHE").is_ok_and(|v| v == "1"))
}

/// FNV-1a по словам ключа — та же дешёвая свёртка, что у [`soup_hash`];
/// коллизия отсеивается побитовым сравнением самого ключа.
pub(crate) fn bits_hash(key: &[u32]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325_u64;
    for w in key {
        h = (h ^ u64::from(*w)).wrapping_mul(0x100_0000_01b3);
    }
    h
}

/// Мемоизация [`crate::svg_path::coverage_quads`] по треугольному супу
/// (BUG-405 срез 9).
///
/// `coverage_quads` — CPU-растеризация покрытия, 58 мкс на команду
/// `DrawSvgStroke` при 16 РАЗНЫХ супах на 704 вызова за прогон прокрутки
/// `lenta.ru`: иконки шапки не двигаются, и 98% вызовов пересчитывают уже
/// посчитанное. Функция чистая, поэтому кэш — точная мемоизация, а не
/// приближение: ключ сравнивается побитово (`f32::to_bits`), так что коллизия
/// хэша даёт промах, а не чужие пиксели, и попадание возвращает те самые
/// вершины, которые вернул бы пересчёт.
///
/// Ключ — суп В АБСОЛЮТНЫХ device-координатах, без нормализации сдвигом. Она
/// дала бы попадания ещё и у движущихся фигур, но растеризация не
/// инвариантна к сдвигу побитово (интерполяция кромки округляется иначе на
/// больших координатах), а на замеренной странице нормализация не даёт ни
/// одного лишнего попадания — те же 16 супов.
/// Запись [`CoverageCache`]: суп-ключ в битовом виде и посчитанное по нему
/// покрытие. Ключ хранится целиком, чтобы совпадение хэша проверялось
/// побитовым сравнением, а не принималось на веру.
type CoverageEntry = (Vec<[u32; 2]>, std::sync::Arc<Vec<crate::svg_path::CoverageVertex>>);

#[derive(Default)]
pub(crate) struct CoverageCache {
    /// Хэш супа → супы с таким хэшом (в битовом виде) и их покрытие.
    buckets: std::collections::HashMap<u64, Vec<CoverageEntry>>,
    /// Сколько вершин суммарно хранится — счётчик для [`COVERAGE_CACHE_MAX_VERTS`].
    stored_verts: usize,
    /// Сколько вызовов вернули готовое покрытие.
    pub(crate) hits: u64,
    /// Сколько вызовов пересчитали покрытие.
    pub(crate) misses: u64,
}

impl CoverageCache {
    /// Покрытие для `soup`: готовое из кэша либо посчитанное и запомненное.
    fn coverage(&mut self, soup: &[[f32; 2]]) -> std::sync::Arc<Vec<crate::svg_path::CoverageVertex>> {
        // Срез 12: цена ключа (хэш + побитовое сравнение) считается отдельно от
        // цены промаха — попадание платит только её, и лечится она иначе.
        let log = crate::frame_log_level() >= 3;
        let t_key = log.then(std::time::Instant::now);
        let h = soup_hash(soup);
        let found = self
            .buckets
            .get(&h)
            .and_then(|bucket| bucket.iter().find(|(key, _)| soup_bits_eq(key, soup)))
            .map(|(_, quads)| std::sync::Arc::clone(quads));
        if let Some(t0) = t_key {
            sub_add(&SVG_SUB.key, t0);
        }
        if let Some(quads) = found {
            self.hits += 1;
            return quads;
        }
        self.misses += 1;
        let t_calc = log.then(std::time::Instant::now);
        let quads = std::sync::Arc::new(crate::svg_path::coverage_quads(soup));
        if let Some(t0) = t_calc {
            sub_add(&SVG_SUB.calc, t0);
        }
        let cost = soup.len() + quads.len();
        if self.stored_verts + cost > COVERAGE_CACHE_MAX_VERTS {
            self.buckets.clear();
            self.stored_verts = 0;
        }
        self.stored_verts += cost;
        let key: Vec<[u32; 2]> = soup.iter().map(|p| [p[0].to_bits(), p[1].to_bits()]).collect();
        self.buckets.entry(h).or_default().push((key, std::sync::Arc::clone(&quads)));
        quads
    }
}

/// FNV-1a по битам вершин супа. Криптостойкость не нужна — коллизия отсеивается
/// побитовым сравнением в [`soup_bits_eq`]; нужна дешевизна, потому что хэш
/// считается на КАЖДЫЙ вызов, в том числе попадающий.
fn soup_hash(soup: &[[f32; 2]]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325_u64;
    for p in soup {
        h = (h ^ u64::from(p[0].to_bits())).wrapping_mul(0x100_0000_01b3);
        h = (h ^ u64::from(p[1].to_bits())).wrapping_mul(0x100_0000_01b3);
    }
    h
}

/// Побитовое равенство запомненного ключа и входного супа. Именно побитовое:
/// одинаковые биты — одинаковый результат чистой функции, включая `NaN` и
/// `-0.0`, которые обычное `==` рассудило бы неверно в обе стороны.
fn soup_bits_eq(key: &[[u32; 2]], soup: &[[f32; 2]]) -> bool {
    key.len() == soup.len()
        && key
            .iter()
            .zip(soup)
            .all(|(k, p)| k[0] == p[0].to_bits() && k[1] == p[1].to_bits())
}

/// Плечо кэша покрытия для вызова [`antialias_fill_soup`]: `None` — считать
/// заново, как до среза 9. `enabled` — инстансный рычаг
/// ([`Renderer::set_coverage_cache_enabled`]), поверх него рычаг процесса
/// `LUMEN_NO_COVERAGE_CACHE=1`.
pub(crate) fn coverage_cache_arm(cache: &mut CoverageCache, enabled: bool) -> Option<&mut CoverageCache> {
    (enabled && !coverage_cache_disabled()).then_some(cache)
}

/// `true`, если кэш покрытия отключён (`LUMEN_NO_COVERAGE_CACHE=1`) — рычаг
/// отката BUG-405 срез 9 к пересчёту на каждую фигуру и A/B-плечо для проверки
/// пикселей.
fn coverage_cache_disabled() -> bool {
    use std::sync::OnceLock;
    static OFF: OnceLock<bool> = OnceLock::new();
    *OFF.get_or_init(|| std::env::var("LUMEN_NO_COVERAGE_CACHE").is_ok_and(|v| v == "1"))
}

pub(crate) fn antialias_fill_soup(
    fill_vertices: &mut Vec<FillVertex>,
    v_start: usize,
    color: [f32; 4],
    dpr: f32,
    cache: Option<&mut CoverageCache>,
) {
    if dpr <= 0.0 || !dpr.is_finite() || v_start >= fill_vertices.len() {
        return;
    }
    let log = crate::frame_log_level() >= 3;
    let t_soup = log.then(std::time::Instant::now);
    let soup: Vec<[f32; 2]> = fill_vertices[v_start..]
        .iter()
        .map(|v| [v.pos[0] * dpr, v.pos[1] * dpr])
        .collect();
    if let Some(t0) = t_soup {
        sub_add(&SVG_SUB.soup, t0);
    }
    let fresh;
    let quads: &[crate::svg_path::CoverageVertex] = match cache {
        Some(cache) => {
            fresh = cache.coverage(&soup);
            fresh.as_slice()
        }
        None => {
            fresh = std::sync::Arc::new(crate::svg_path::coverage_quads(&soup));
            fresh.as_slice()
        }
    };
    if quads.is_empty() {
        return;
    }
    fill_vertices.truncate(v_start);
    fill_vertices.reserve(quads.len());
    for q in quads {
        fill_vertices.push(FillVertex {
            pos: [q.pos[0] / dpr, q.pos[1] / dpr],
            z: 0.0,
            color: [color[0], color[1], color[2], color[3] * q.cov],
        });
    }
}

/// `true`, если 2D-affine матрица `PushTransform` уводит кромки квада с осей
/// растровой сетки (rotate/skew/matrix с ненулевыми `b`/`c`).
///
/// Только у такого квада кромка перестаёт быть пиксельной границей и требует
/// покрытия ([`antialias_fill_soup`], BUG-277 срез 13). Чисто осевые матрицы
/// (translate/scale/flip) оставляют прежний путь побитово нетронутым, а 3D и
/// перспектива исключены: [`antialias_fill_soup`] не переносит спроецированный
/// `z`, нужный для depth-теста под `preserve-3d`.
pub(crate) fn rotates_axes_2d(m: &Mat4) -> bool {
    const EPS: f32 = 1e-4;
    m.is_2d_affine() && (m.0[1].abs() > EPS || m.0[4].abs() > EPS)
}

/// Применяет матрицу `PushTransform` к pos-полям вершин.
///
/// 2D affine (`m.is_2d_affine()`) — быстрый путь: x' = a·x + c·y + e
/// (побитово идентично старому 2D-конвейеру; z остаётся 0.0 по умолчанию).
/// Иначе (CSS Transforms L2: 3D rotate/translate/scale, `perspective()`,
/// `matrix3d`) — полная 4×4 проекция с перспективным делением через
/// `Mat4::project_point_z`: возвращает (x', y', z'), где z' сохраняется
/// через `VertexPos::set_depth` для GPU depth testing.
/// FillVertex/TextVertex/ImageVertex/RRectVertex реализуют set_depth и
/// получают корректную глубину для cross-type occlusion под preserve-3d;
/// CircleVertex и GradVertex используют no-op (depth=0.0, painter's order).
pub(crate) fn apply_affine_to_verts<V: VertexPos>(verts: &mut [V], m: &Mat4) {
    if m.is_2d_affine() {
        let a = m.0[0];
        let b = m.0[1];
        let c = m.0[4];
        let d = m.0[5];
        let e = m.0[12];
        let f = m.0[13];
        for v in verts {
            let p = v.pos_mut();
            let x = p[0];
            let y = p[1];
            p[0] = a * x + c * y + e;
            p[1] = b * x + d * y + f;
            // z stays 0.0 (2D affine: depth=0.5 in shader, painter's order applies)
        }
    } else {
        // CSS Transforms L2 — 3D/perspective transform: preserve z for depth testing.
        for v in verts {
            let (x, y, z) = {
                let p = v.pos_mut();
                m.project_point_z(p[0], p[1], 0.0)
            };
            {
                let p = v.pos_mut();
                p[0] = x;
                p[1] = y;
            }
            v.set_depth(z);
        }
    }
}

/// Эмитирует квад для SDF-круга.
///
/// Quad расширяется на 0.5 CSS-px в каждую сторону от `rect`, чтобы шейдер
/// мог рисовать внешнюю половину 1px AA-полосы (Skia-compatible linear AA).
/// UV = ±1 соответствует CSS_radius + 0.5 px от центра.
fn push_circle_quad(out: &mut Vec<CircleVertex>, rect: Rect, color: [f32; 4]) {
    let radius_px = rect.width * 0.5;
    let x0 = rect.x - 0.5;
    let y0 = rect.y - 0.5;
    let x1 = rect.x + rect.width + 0.5;
    let y1 = rect.y + rect.height + 0.5;
    out.extend_from_slice(&[
        CircleVertex { pos: [x0, y0], uv: [-1.0, -1.0], color, radius_px },
        CircleVertex { pos: [x1, y0], uv: [ 1.0, -1.0], color, radius_px },
        CircleVertex { pos: [x1, y1], uv: [ 1.0,  1.0], color, radius_px },
        CircleVertex { pos: [x0, y0], uv: [-1.0, -1.0], color, radius_px },
        CircleVertex { pos: [x1, y1], uv: [ 1.0,  1.0], color, radius_px },
        CircleVertex { pos: [x0, y1], uv: [-1.0,  1.0], color, radius_px },
    ]);
}

/// Применяет 2D аффинное преобразование к pos-полям CircleVertex.
/// UV-координаты не затрагиваются — они описывают относительную позицию
/// внутри квада, а не мировые координаты.
pub(crate) fn apply_affine_to_circle_verts(verts: &mut [CircleVertex], m: &Mat4) {
    apply_affine_to_verts(verts, m);
}

/// Emits 6 `RRectClipVertex` (two triangles) for the rounded-clip composite of
/// one offscreen level: the quad covers exactly the clip rect, `uv` samples the
/// level texture at the same screen position, and the contour parameters are
/// constant across the quad so the fragment shader can evaluate `sdf_rrect`.
///
/// `vw`/`vh` — viewport in CSS px (`surface / dpr`), the space the level's own
/// vertices were mapped from, so `uv = css / viewport` hits the same texel.
/// Radii go through the same [`CornerRadii::clamped_to_box`] as the fill path —
/// the clip contour must not drift from the container's painted contour.
pub(crate) fn push_rrect_clip_quad(
    out: &mut Vec<RRectClipVertex>,
    rect: Rect,
    radii: CornerRadii,
    vw: f32,
    vh: f32,
) {
    let x0 = rect.x;
    let y0 = rect.y;
    let x1 = rect.x + rect.width;
    let y1 = rect.y + rect.height;
    let center = [(x0 + x1) * 0.5, (y0 + y1) * 0.5];
    let half_size = [rect.width * 0.5, rect.height * 0.5];
    let radii = radii.clamped_to_box(rect.width, rect.height);
    let radii_x = [radii.tl,   radii.tr,   radii.br,   radii.bl  ];
    let radii_y = [radii.tl_y, radii.tr_y, radii.br_y, radii.bl_y];
    let v = |px: f32, py: f32| RRectClipVertex {
        pos: [px / vw * 2.0 - 1.0, 1.0 - py / vh * 2.0],
        uv: [px / vw, py / vh],
        world_pos: [px, py],
        center,
        half_size,
        radii_x,
        radii_y,
    };
    out.extend_from_slice(&[
        v(x0, y0), v(x1, y0), v(x1, y1),
        v(x0, y0), v(x1, y1), v(x0, y1),
    ]);
}

/// Emits 6 [`PathClipVertex`] (two triangles) for the clip-shape composite quad.
/// The quad is the shape's screen-space bounding box; the exact contour is
/// carved per-fragment from the uniform (`PATH_CLIP_SHADER_SRC`).
pub(crate) fn push_path_clip_quad(out: &mut Vec<PathClipVertex>, rect: Rect, vw: f32, vh: f32) {
    let x0 = rect.x;
    let y0 = rect.y;
    let x1 = rect.x + rect.width;
    let y1 = rect.y + rect.height;
    let v = |px: f32, py: f32| PathClipVertex {
        pos: [px / vw * 2.0 - 1.0, 1.0 - py / vh * 2.0],
        uv: [px / vw, py / vh],
        world_pos: [px, py],
    };
    out.extend_from_slice(&[
        v(x0, y0), v(x1, y0), v(x1, y1),
        v(x0, y0), v(x1, y1), v(x0, y1),
    ]);
}

/// Сдвигает форму клипа на scroll-смещение слоя (page px), как
/// `translate_rect` делает с прямоугольниками.
pub(crate) fn translate_clip_shape(shape: &ResolvedClipShape, dx: f32, dy: f32) -> ResolvedClipShape {
    match shape {
        ResolvedClipShape::Circle { cx, cy, r } => {
            ResolvedClipShape::Circle { cx: cx + dx, cy: cy + dy, r: *r }
        }
        ResolvedClipShape::Ellipse { cx, cy, rx, ry } => {
            ResolvedClipShape::Ellipse { cx: cx + dx, cy: cy + dy, rx: *rx, ry: *ry }
        }
        ResolvedClipShape::Polygon { verts, even_odd } => ResolvedClipShape::Polygon {
            verts: verts.iter().map(|(x, y)| (x + dx, y + dy)).collect(),
            even_odd: *even_odd,
        },
    }
}

/// Переводит форму `clip-path` (page px, до transform элемента) в параметры
/// шейдера в экранных CSS px под накопленным трансформом `m`.
///
/// Полигон трансформируется по вершинам — аффинный образ полигона это полигон,
/// поэтому поворот/скос/масштаб точны. Круг/эллипс образуют аффинно-отображённый
/// единичный круг: `screen = M·u + c`, где `M = A·diag(rx, ry)`, а шейдеру
/// нужна `M⁻¹`.
///
/// `None` (→ исторический bbox-клип) когда: форма вырождена; вершин больше
/// [`PATH_CLIP_MAX_VERTS`]; трансформ не 2D-аффинный (3D/перспектива — форма
/// в экранном пространстве уже не эта); матрица вырождена (нулевая площадь).
pub(crate) fn path_clip_params(shape: &ResolvedClipShape, m: Option<&Mat4>) -> Option<PathClipParamsCpu> {
    // Column-major 2D affine: screen.x = a·x + c·y + e, screen.y = b·x + d·y + f.
    let (a, b, c, d, e, f) = match m {
        None => (1.0, 0.0, 0.0, 1.0, 0.0, 0.0),
        Some(m) if m.is_2d_affine() => (m.0[0], m.0[1], m.0[4], m.0[5], m.0[12], m.0[13]),
        Some(_) => return None,
    };
    let mut p = PathClipParamsCpu {
        header: [0; 4],
        center: [0.0; 4],
        inv_m: [0.0; 4],
        verts: [[0.0; 4]; PATH_CLIP_MAX_VERTS / 2],
    };
    match shape {
        ResolvedClipShape::Circle { cx, cy, r } if *r > 0.0 => {
            ellipse_params(&mut p, (a, b, c, d, e, f), (*cx, *cy), (*r, *r))?;
        }
        ResolvedClipShape::Ellipse { cx, cy, rx, ry } if *rx > 0.0 && *ry > 0.0 => {
            ellipse_params(&mut p, (a, b, c, d, e, f), (*cx, *cy), (*rx, *ry))?;
        }
        ResolvedClipShape::Polygon { verts, even_odd } if verts.len() >= 3 => {
            if verts.len() > PATH_CLIP_MAX_VERTS {
                return None;
            }
            p.header = [1, verts.len() as u32, u32::from(*even_odd), 0];
            for (i, (x, y)) in verts.iter().enumerate() {
                let sx = a * x + c * y + e;
                let sy = b * x + d * y + f;
                let slot = &mut p.verts[i / 2];
                if i % 2 == 0 {
                    slot[0] = sx;
                    slot[1] = sy;
                } else {
                    slot[2] = sx;
                    slot[3] = sy;
                }
            }
        }
        // Вырожденная форма (нулевой радиус, <3 вершин) не клиппит ничего
        // осмысленного — оставляем bbox-путь, как было до среза.
        _ => return None,
    }
    Some(p)
}

/// Заполняет параметры аффинно-отображённого круга: центр в экранных px и
/// `M⁻¹` для `M = A·diag(rx, ry)`. `None` — вырожденная матрица.
fn ellipse_params(
    p: &mut PathClipParamsCpu,
    (a, b, c, d, e, f): (f32, f32, f32, f32, f32, f32),
    (cx, cy): (f32, f32),
    (rx, ry): (f32, f32),
) -> Option<()> {
    // M = A·diag(rx, ry) = [[a·rx, c·ry], [b·rx, d·ry]].
    let (m00, m01, m10, m11) = (a * rx, c * ry, b * rx, d * ry);
    let det = m00 * m11 - m01 * m10;
    if det.abs() < 1e-9 {
        return None;
    }
    let inv = 1.0 / det;
    p.header = [0, 0, 0, 0];
    p.center = [a * cx + c * cy + e, b * cx + d * cy + f, 0.0, 0.0];
    p.inv_m = [m11 * inv, -m01 * inv, -m10 * inv, m00 * inv];
    Some(())
}

/// Параметры точного клипа для прямоугольного `overflow: hidden`, чей бокс
/// повёрнут/скошен накопленным трансформом (BUG-277 срез 14).
///
/// `PushClipRect` кладёт в `clip_stack` AABB трансформированного прямоугольника
/// (`apply_transform_to_clip`, политика BUG-140), а scissor у wgpu и не умеет
/// иначе. Пока матрица осевая, AABB *и есть* сам клип; под `rotate`/`skew` он
/// становится описанной рамкой, и ребёнок протекает в четыре угловых
/// треугольника между рамкой и настоящим квадрилатералом. Здесь тот же
/// прямоугольник отдаётся уже существующей машинерии точного клипа
/// ([`path_clip_params`], срез 8) как четырёхвершинный полигон — контур
/// вычисляется в шейдере покрытия, поэтому кромка ещё и сглаживается.
///
/// `None` — трансформа нет, он осевой (AABB точен, прежний путь остаётся
/// побитово тем же), прямоугольник вырожден или клип отключён
/// [`rot_clip_disabled`].
pub(crate) fn rotated_rect_clip_params(rect: Rect, m: Option<&Mat4>) -> Option<PathClipParamsCpu> {
    let m = m?;
    if rot_clip_disabled() || !rotates_axes_2d(m) || rect.width <= 0.0 || rect.height <= 0.0 {
        return None;
    }
    let (x0, y0) = (rect.x, rect.y);
    let (x1, y1) = (rect.x + rect.width, rect.y + rect.height);
    let shape = ResolvedClipShape::Polygon {
        verts: vec![(x0, y0), (x1, y0), (x1, y1), (x0, y1)],
        even_odd: false,
    };
    path_clip_params(&shape, Some(m))
}

/// Per-axis scale of an accumulated transform, or `None` if it rotates/skews.
///
/// A rounded clip is only a rounded rect in screen space while the transform
/// keeps the axes: `PushClipRoundedRect` under a rotation falls back to the
/// bbox-only scissor clip (the historic behaviour) instead of masking with a
/// contour that no longer matches the box.
pub(crate) fn axis_aligned_scale(m: Option<&Mat4>) -> Option<(f32, f32)> {
    let Some(m) = m else { return Some((1.0, 1.0)) };
    if !m.is_2d_affine() {
        return None;
    }
    // Column-major 2D affine: pos.x = m[0]·x + m[4]·y + m[12].
    // Off-diagonal terms m[1] (shear y-from-x) and m[4] (shear x-from-y)
    // must vanish for the box to stay axis-aligned.
    const EPS: f32 = 1e-4;
    if m.0[1].abs() > EPS || m.0[4].abs() > EPS {
        return None;
    }
    Some((m.0[0].abs(), m.0[5].abs()))
}

/// Emits 6 `RRectVertex` (two triangles) for a rounded rect quad.
/// Per-vertex `center`, `half_size`, and `radii` are constant across the quad so
/// the fragment shader can evaluate the SDF at each fragment position.
///
/// Radii are reduced by the CSS Backgrounds L3 §5.5 overlap factor here, through
/// the same [`CornerRadii::clamped_to_box`] the CPU rasterizer uses — a single
/// factor across all four corners, not a per-axis `min` (which would turn a
/// `border-radius: 999px` pill into a full ellipse).
pub(crate) fn push_rrect_quad(out: &mut Vec<RRectVertex>, rect: Rect, color: [f32; 4], radii: CornerRadii) {
    let x0 = rect.x;
    let y0 = rect.y;
    let x1 = rect.x + rect.width;
    let y1 = rect.y + rect.height;
    let center = [(x0 + x1) * 0.5, (y0 + y1) * 0.5];
    let half_size = [rect.width * 0.5, rect.height * 0.5];
    let radii = radii.clamped_to_box(rect.width, rect.height);
    let radii_x = [radii.tl,   radii.tr,   radii.br,   radii.bl  ];
    let radii_y = [radii.tl_y, radii.tr_y, radii.br_y, radii.bl_y];
    let v = |px: f32, py: f32| RRectVertex { pos: [px, py], z: 0.0, color, center, half_size, radii_x, radii_y };
    out.extend_from_slice(&[
        v(x0, y0), v(x1, y0), v(x1, y1),
        v(x0, y0), v(x1, y1), v(x0, y1),
    ]);
}

/// Кладёт квад аналитической размытой тени (BUG-405 срез 7).
///
/// `rect`/`radii` — сама фигура (уже в экранных CSS px), `pad` — запас на
/// радиус ядра блюра: за его пределами вклад гауссианы ровно ноль, потому что
/// ядро в шейдере обрезано так же, как в [`BLUR_SHADER_SRC`]. Квад строится по
/// раздутому прямоугольнику, а `center`/`half_size`/радиусы остаются от
/// исходной фигуры — фрагмент считает размытие именно её.
pub(crate) fn push_shadow_quad(
    out: &mut Vec<ShadowVertex>,
    rect: Rect,
    color: [f32; 4],
    radii: CornerRadii,
    sigma: f32,
    pad: f32,
) {
    let center = [rect.x + rect.width * 0.5, rect.y + rect.height * 0.5];
    let half_size = [rect.width * 0.5, rect.height * 0.5];
    let radii = radii.clamped_to_box(rect.width, rect.height);
    let radii_x = [radii.tl, radii.tr, radii.br, radii.bl];
    let radii_y = [radii.tl_y, radii.tr_y, radii.br_y, radii.bl_y];
    let (x0, y0) = (rect.x - pad, rect.y - pad);
    let (x1, y1) = (rect.x + rect.width + pad, rect.y + rect.height + pad);
    let v = |px: f32, py: f32| ShadowVertex {
        pos: [px, py],
        z: 0.0,
        color,
        center,
        half_size,
        radii_x,
        radii_y,
        sigma,
    };
    out.extend_from_slice(&[
        v(x0, y0), v(x1, y0), v(x1, y1),
        v(x0, y0), v(x1, y1), v(x0, y1),
    ]);
}

/// `Some((tx, ty))`, если матрица — чистый перенос в плоскости экрана.
/// Поворот/масштаб/перспектива уводят фигуру с осей, под которыми аналитическая
/// тень выведена, — такие остаются на прежнем пути с offscreen-уровнем.
pub(crate) fn transform_is_translation(m: &Mat4) -> Option<(f32, f32)> {
    let e = &m.0;
    let unit = |a: f32, b: f32| (a - b).abs() < 1e-6;
    let ok = unit(e[0], 1.0)
        && unit(e[5], 1.0)
        && unit(e[10], 1.0)
        && unit(e[15], 1.0)
        && unit(e[1], 0.0)
        && unit(e[2], 0.0)
        && unit(e[3], 0.0)
        && unit(e[4], 0.0)
        && unit(e[6], 0.0)
        && unit(e[7], 0.0)
        && unit(e[8], 0.0)
        && unit(e[9], 0.0)
        && unit(e[11], 0.0)
        && unit(e[14], 0.0);
    ok.then(|| (e[12], e[13]))
}

/// Applies a `PushTransform` matrix to `RRectVertex::pos` AND `center` fields.
/// `half_size` and `radii` are scale-invariant for Phase 0 (no rotation/scale transforms on layout boxes).
///
/// 2D affine — fast path (z stays 0); 3D/perspective — `Mat4::project_point_z`
/// on pos (writing the projected z into `RRectVertex.z` for GPU depth testing
/// under CSS Transforms L2 `preserve-3d`) and `Mat4::project_point` on center
/// (best-effort: the SDF `half_size`/`radii` stay unprojected, so a rounded
/// rect under perspective keeps uniform corner radii — acceptable Phase-0
/// approximation, same as the no-rotation note above).
pub(crate) fn apply_affine_to_rrect_verts(verts: &mut [RRectVertex], m: &Mat4) {
    if m.is_2d_affine() {
        for v in verts {
            let [px, py] = v.pos;
            v.pos = [
                m.0[0] * px + m.0[4] * py + m.0[12],
                m.0[1] * px + m.0[5] * py + m.0[13],
            ];
            let [cx, cy] = v.center;
            v.center = [
                m.0[0] * cx + m.0[4] * cy + m.0[12],
                m.0[1] * cx + m.0[5] * cy + m.0[13],
            ];
            // z stays unchanged (2D affine: depth=0.5 in shader, painter's order applies)
        }
    } else {
        for v in verts {
            // 3D/perspective: preserve z for cross-type depth testing.
            let (px, py, pz) = m.project_point_z(v.pos[0], v.pos[1], 0.0);
            v.pos = [px, py];
            v.z = pz;
            let (cx, cy) = m.project_point(v.center[0], v.center[1], 0.0);
            v.center = [cx, cy];
        }
    }
}

/// Emits tessellated triangle fan for one border corner arc (quarter-annulus).
/// `center`   = pivot point of the arc (corner center of the rounded rect).
/// `outer_r`  = outer radius (= border-radius value).
/// `inner_r`  = inner radius (= outer_r - border_width, or 0 if border fills the corner).
/// `start_deg`/`end_deg` = sweep in degrees (screen Y-down, clockwise).
/// `color`    = fill color from the adjacent border side.
///
/// Uses 8 segments for smooth Phase 0 quality. Each segment is two triangles
/// forming an annular sector quad.
pub(crate) fn emit_border_arc(
    out: &mut Vec<FillVertex>,
    center: [f32; 2],
    outer_r: f32,
    inner_r: f32,
    start_deg: f32,
    end_deg: f32,
    color: [f32; 4],
) {
    const N: u32 = 8;
    let step = (end_deg - start_deg) / N as f32;
    let [cx, cy] = center;
    for i in 0..N {
        let a0 = (start_deg + i as f32 * step).to_radians();
        let a1 = (start_deg + (i + 1) as f32 * step).to_radians();
        let (s0, c0) = (a0.sin(), a0.cos());
        let (s1, c1) = (a1.sin(), a1.cos());
        // Outer arc vertices.
        let po0 = [cx + outer_r * c0, cy + outer_r * s0];
        let po1 = [cx + outer_r * c1, cy + outer_r * s1];
        // Inner arc vertices (or center if inner_r == 0).
        let pi0 = [cx + inner_r * c0, cy + inner_r * s0];
        let pi1 = [cx + inner_r * c1, cy + inner_r * s1];
        out.extend_from_slice(&[
            FillVertex { pos: po0, z: 0.0, color },
            FillVertex { pos: po1, z: 0.0, color },
            FillVertex { pos: pi1, z: 0.0, color },
            FillVertex { pos: po0, z: 0.0, color },
            FillVertex { pos: pi1, z: 0.0, color },
            FillVertex { pos: pi0, z: 0.0, color },
        ]);
    }
}

/// CSS Images L3 §3.3 — push 6 GradVertex (2 triangles) for `rect`.
/// UV is baked: TL=(0,0), TR=(1,0), BL=(0,1), BR=(1,1).
pub(crate) fn push_grad_quad(out: &mut Vec<GradVertex>, rect: Rect) {
    out.extend_from_slice(&grad_quad(rect));
}

/// [`grad_quad`], прогнанный через накопленный `PushTransform`.
///
/// Маски держат свой квад в плане рендера (а не в общем вершинном буфере),
/// поэтому трансформа обязана примениться на этапе планирования: рендер-стадия
/// матрицы уже не видит, а контент-слой рисуется трансформированным (BUG-277).
pub(crate) fn transformed_grad_quad(rect: Rect, m: Option<&Mat4>) -> [GradVertex; 6] {
    let mut quad = grad_quad(rect);
    if let Some(m) = m {
        apply_affine_to_grad_verts(&mut quad, m);
    }
    quad
}

/// Два треугольника градиентного квада по `rect`, uv = (0,0)…(1,1).
///
/// Отдельно от [`push_grad_quad`], потому что маски держат свой квад
/// в плане рендера (а не в общем вершинном буфере) и должны прогнать
/// его через `PushTransform` ещё на этапе планирования.
fn grad_quad(rect: Rect) -> [GradVertex; 6] {
    let (x0, y0) = (rect.x, rect.y);
    let (x1, y1) = (rect.x + rect.width, rect.y + rect.height);
    [
        GradVertex { pos: [x0, y0], uv: [0.0, 0.0] },
        GradVertex { pos: [x1, y0], uv: [1.0, 0.0] },
        GradVertex { pos: [x1, y1], uv: [1.0, 1.0] },
        GradVertex { pos: [x0, y0], uv: [0.0, 0.0] },
        GradVertex { pos: [x1, y1], uv: [1.0, 1.0] },
        GradVertex { pos: [x0, y1], uv: [0.0, 1.0] },
    ]
}

/// CSS Images L3 §3.3 — resolve `GradientStop` positions to normalized [0,1].
///
/// Thin wrapper over the shared [`crate::gradient_math::resolve_stop_positions`]
/// (single source of truth for all backends, PA-1) converting colours to the
/// `[f32; 4]` straight-RGBA layout the GPU vertex buffers use.
pub(crate) fn resolve_gradient_stops(stops: &[GradientStop], line_len: f32) -> Vec<(f32, [f32; 4])> {
    crate::gradient_math::resolve_stop_positions(stops, line_len)
        .into_iter()
        .map(|(pos, c)| {
            (
                pos,
                [
                    c.r as f32 / 255.0,
                    c.g as f32 / 255.0,
                    c.b as f32 / 255.0,
                    c.a as f32 / 255.0,
                ],
            )
        })
        .collect()
}

/// CSS Images L3 §3.4 — compute linear gradient line endpoints in UV [0,1] space.
///
/// Returns `(start_uv, end_uv, line_len)` such that
/// `t = dot(uv-start, end-start)/|end-start|²` gives t=0 at the start-color
/// edge and t=1 at the end-color edge. `line_len` is the gradient line length
/// in CSS px (mirrors `cpu_raster::linear_uv_endpoints`) — callers must feed
/// it to [`resolve_gradient_stops`] so `Px`/`Calc` stop positions resolve
/// against the same length as the CPU/femtovg backends (BUG-277).
///
/// CSS angle convention: 0° = "to top", 90° = "to right", 180° = "to bottom".
/// Box dimensions `w`×`h` in CSS pixels.
pub(crate) fn linear_gradient_uv_endpoints(w: f32, h: f32, angle_deg: f32) -> ([f32; 2], [f32; 2], f32) {
    if w <= 0.0 || h <= 0.0 {
        return ([0.0, 0.5], [1.0, 0.5], w.max(h).max(1.0));
    }
    let theta = angle_deg.to_radians();
    let dx = theta.sin();
    let dy = -theta.cos(); // negative because CSS y grows down
    let half_len = (w * dx.abs() + h * dy.abs()) / 2.0;
    if half_len < 1e-6 {
        return ([0.5, 0.5], [0.5, 0.5], 1.0);
    }
    let cx = w / 2.0;
    let cy = h / 2.0;
    let sx = (cx - dx * half_len) / w;
    let sy = (cy - dy * half_len) / h;
    let ex = (cx + dx * half_len) / w;
    let ey = (cy + dy * half_len) / h;
    ([sx, sy], [ex, ey], 2.0 * half_len)
}

/// CSS Images L3 §3.5 — compute radial gradient center + semi-axes in UV [0,1] space.
///
/// Returns (center_uv, semi_axes_uv). Шейдер (kind = 1) берёт
/// `t = length((uv − p0) / p1)`, поэтому `t == 1` обязано ложиться ровно на
/// конечную фигуру — эллипс с полуосями `radius_x`/`radius_y` в CSS px,
/// как их посчитал дисплей-лист (`cpu_raster::rasterize_radial_gradient`
/// берёт те же два числа). Перевод в UV — деление на размер бокса.
///
/// BUG-277 срез 6: прежняя версия игнорировала радиусы вовсе и брала
/// `rx = max(cx, 1−cx)` — полуширину бокса, то есть farthest-**side**.
/// На `circle at 50% 50%` в квадрате это давало радиус 150 px вместо
/// farthest-corner 212 px: градиент «сжимался» в √2 раза.
pub(crate) fn radial_gradient_uv_params(
    cx_pct: f32,
    cy_pct: f32,
    radius_x: f32,
    radius_y: f32,
    rect: Rect,
) -> ([f32; 2], [f32; 2]) {
    let rx = (radius_x / rect.width.max(1e-6)).max(1e-6);
    let ry = (radius_y / rect.height.max(1e-6)).max(1e-6);
    ([cx_pct, cy_pct], [rx, ry])
}

/// Box aspect `height / width` — the `param0` a linear gradient feeds the shader.
///
/// The gradient-line endpoints travel in UV, where the box is squashed to a
/// unit square; projecting onto them there tilts the iso-colour bands by the
/// aspect ratio. Passing `h/w` lets the fragment shader restore the pixel
/// metric (BUG-277 срез 9). A degenerate box collapses to 1.0 (square).
pub(crate) fn box_aspect(rect: Rect) -> f32 {
    if rect.width.abs() < 1e-6 || rect.height.abs() < 1e-6 {
        return 1.0;
    }
    rect.height / rect.width
}

/// Build a [`GradParamsCpu`] from resolved stops + pre-computed UV params.
///
/// `param0` is kind-specific: the conic gradient (kind = 2) passes its starting
/// angle in radians (0 = top, clockwise), the linear gradient (kind = 0) passes
/// [`box_aspect`]; the radial gradient does not use it.
///
/// The stop list is taken WHOLE — the shader reads it out of a storage buffer,
/// so there is no capacity to truncate against (BUG-277 срез 11).
pub(crate) fn build_grad_params(
    resolved: &[(f32, [f32; 4])],
    p0: [f32; 2],
    p1: [f32; 2],
    kind: u32,
    repeating: bool,
    param0: f32,
) -> GradParamsCpu {
    let stops: Vec<GradStopCpu> = resolved
        .iter()
        .map(|&(pos, col)| GradStopCpu { color: col, pos, _p0: 0.0, _p1: 0.0, _p2: 0.0 })
        .collect();
    GradParamsCpu {
        header: GradHeaderCpu {
            p0,
            p1,
            n_stops: stops.len() as u32,
            kind,
            repeating: if repeating { 1 } else { 0 },
            param0,
        },
        stops,
    }
}

/// Заливает [`GradParamsCpu`] в свежий storage buffer для градиентного пасса:
/// заголовок со смещения 0, стопы — со смещения `size_of::<GradHeaderCpu>()`
/// (32 байта, выравнивание `array<GradStop>` в WGSL).
///
/// Буфер всегда содержит место хотя бы под один стоп: `array<GradStop>` нулевой
/// длины — невалидная привязка, а `n_stops == 0` шейдер и так отбрасывает.
/// Отдельный буфер на каждый вызов — тот же запрет на общий uniform, что и у
/// параметров фильтра/блэнда: все `write_buffer` ложатся до единственного
/// `submit`, и общий буфер отдал бы всем пассам параметры последнего (срез 7).
pub(crate) fn write_grad_buffer(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    params: &GradParamsCpu,
    label: &str,
) -> wgpu::Buffer {
    let header_size = std::mem::size_of::<GradHeaderCpu>() as u64;
    let stop_size = std::mem::size_of::<GradStopCpu>() as u64;
    let n = params.stops.len().max(1) as u64;
    let buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: header_size + n * stop_size,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    // SAFETY: GradHeaderCpu/GradStopCpu — #[repr(C)] POD без паддинг-инвариантов,
    // раскладка совпадает с WGSL-структурой; байтовое представление валидно.
    queue.write_buffer(&buf, 0, as_bytes(std::slice::from_ref(&params.header)));
    if !params.stops.is_empty() {
        queue.write_buffer(&buf, header_size, as_bytes(&params.stops));
    }
    buf
}

pub(crate) fn push_fill_quad(out: &mut Vec<FillVertex>, rect: Rect, color: [f32; 4]) {
    let x0 = rect.x;
    let y0 = rect.y;
    let x1 = rect.x + rect.width;
    let y1 = rect.y + rect.height;
    out.extend_from_slice(&[
        FillVertex { pos: [x0, y0], z: 0.0, color },
        FillVertex { pos: [x1, y0], z: 0.0, color },
        FillVertex { pos: [x1, y1], z: 0.0, color },
        FillVertex { pos: [x0, y0], z: 0.0, color },
        FillVertex { pos: [x1, y1], z: 0.0, color },
        FillVertex { pos: [x0, y1], z: 0.0, color },
    ]);
}

pub(crate) fn push_image_quad(
    out: &mut Vec<ImageVertex>,
    rect: Rect,
    uv_min: [f32; 2],
    uv_max: [f32; 2],
    alpha: f32,
) {
    let x0 = rect.x;
    let y0 = rect.y;
    let x1 = rect.x + rect.width;
    let y1 = rect.y + rect.height;
    let [u0, v0] = uv_min;
    let [u1, v1] = uv_max;
    out.extend_from_slice(&[
        ImageVertex { pos: [x0, y0], z: 0.0, uv: [u0, v0], alpha },
        ImageVertex { pos: [x1, y0], z: 0.0, uv: [u1, v0], alpha },
        ImageVertex { pos: [x1, y1], z: 0.0, uv: [u1, v1], alpha },
        ImageVertex { pos: [x0, y0], z: 0.0, uv: [u0, v0], alpha },
        ImageVertex { pos: [x1, y1], z: 0.0, uv: [u1, v1], alpha },
        ImageVertex { pos: [x0, y1], z: 0.0, uv: [u0, v1], alpha },
    ]);
}

/// CSS Images L4 §4 — emit one cross-fade quad covering `rect` with UV
/// `[0,0]→[1,1]`. Vertex order matches `push_image_quad` (two triangles,
/// CCW in window space) so the resulting list runs through the
/// `cross_fade_pipeline` without further reordering.
pub(crate) fn push_cross_fade_quad(out: &mut Vec<CrossFadeVertex>, rect: Rect) {
    let x0 = rect.x;
    let y0 = rect.y;
    let x1 = rect.x + rect.width;
    let y1 = rect.y + rect.height;
    out.extend_from_slice(&[
        CrossFadeVertex { pos: [x0, y0], uv: [0.0, 0.0] },
        CrossFadeVertex { pos: [x1, y0], uv: [1.0, 0.0] },
        CrossFadeVertex { pos: [x1, y1], uv: [1.0, 1.0] },
        CrossFadeVertex { pos: [x0, y0], uv: [0.0, 0.0] },
        CrossFadeVertex { pos: [x1, y1], uv: [1.0, 1.0] },
        CrossFadeVertex { pos: [x0, y1], uv: [0.0, 1.0] },
    ]);
}

pub(crate) fn push_composite_quad(out: &mut Vec<CompositeVertex>, alpha: f32) {
    out.extend_from_slice(&[
        CompositeVertex { pos: [-1.0,  1.0], uv: [0.0, 0.0], alpha },
        CompositeVertex { pos: [ 1.0,  1.0], uv: [1.0, 0.0], alpha },
        CompositeVertex { pos: [ 1.0, -1.0], uv: [1.0, 1.0], alpha },
        CompositeVertex { pos: [-1.0,  1.0], uv: [0.0, 0.0], alpha },
        CompositeVertex { pos: [ 1.0, -1.0], uv: [1.0, 1.0], alpha },
        CompositeVertex { pos: [-1.0, -1.0], uv: [0.0, 1.0], alpha },
    ]);
}

/// Pushes 6 vertices for a quad covering only `bounds` (in CSS px) in screen
/// space, sampling from the corresponding UV region of the source texture.
///
/// NDC x = css_x / vw * 2 - 1; NDC y = 1 - css_y / vh * 2 (Y flipped).
/// UV  x = css_x / vw;         UV  y = css_y / vh.
/// `vw = surf_w / dpr`, `vh = surf_h / dpr`.
///
/// `src_region = Some([rx, ry, rw, rh])` (device px) — источник не
/// полноразмерная текстура, а bbox-офскрин региона: UV считается
/// относительно региона (`(css·dpr − r0) / rдлина`), NDC не меняется.
pub(crate) fn push_bounded_quad(
    out: &mut Vec<CompositeVertex>,
    bounds: lumen_core::geom::Rect,
    surf_w: f32,
    surf_h: f32,
    dpr: f32,
    alpha: f32,
    src_region: Option<[u32; 4]>,
) {
    let vw = surf_w / dpr;
    let vh = surf_h / dpr;
    let x0 = bounds.x / vw * 2.0 - 1.0;
    let x1 = (bounds.x + bounds.width) / vw * 2.0 - 1.0;
    let y0 = 1.0 - bounds.y / vh * 2.0;
    let y1 = 1.0 - (bounds.y + bounds.height) / vh * 2.0;
    let (u0, u1, v0, v1) = match src_region {
        Some([rx, ry, rw, rh]) => {
            let (rx, ry, rw, rh) = (rx as f32, ry as f32, (rw as f32).max(1.0), (rh as f32).max(1.0));
            (
                (bounds.x * dpr - rx) / rw,
                ((bounds.x + bounds.width) * dpr - rx) / rw,
                (bounds.y * dpr - ry) / rh,
                ((bounds.y + bounds.height) * dpr - ry) / rh,
            )
        }
        None => (
            bounds.x / vw,
            (bounds.x + bounds.width) / vw,
            bounds.y / vh,
            (bounds.y + bounds.height) / vh,
        ),
    };
    out.extend_from_slice(&[
        CompositeVertex { pos: [x0, y0], uv: [u0, v0], alpha },
        CompositeVertex { pos: [x1, y0], uv: [u1, v0], alpha },
        CompositeVertex { pos: [x1, y1], uv: [u1, v1], alpha },
        CompositeVertex { pos: [x0, y0], uv: [u0, v0], alpha },
        CompositeVertex { pos: [x1, y1], uv: [u1, v1], alpha },
        CompositeVertex { pos: [x0, y1], uv: [u0, v1], alpha },
    ]);
}

/// Квад H-прохода блюра в bbox-офскрин (BUG-405 срез 24): позиция кроет весь
/// офскрин-таргет (NDC −1..1), UV — соответствующая область ПОЛНОРАЗМЕРНОГО
/// источника (`region / surface`).
///
/// Центр фрагмента `i` офскрина шириной `rw` даёт `uv.x = (rx + i + 0.5)/surf_w`
/// — ровно то же значение, что было у пикселя `rx + i` при полноразмерном
/// проходе, поэтому источник читается по тем же текселям.
pub(crate) fn push_region_src_quad(
    out: &mut Vec<CompositeVertex>,
    region: [u32; 4],
    surf_w: f32,
    surf_h: f32,
    alpha: f32,
) {
    let (rx, ry, rw, rh) = (
        region[0] as f32,
        region[1] as f32,
        region[2] as f32,
        region[3] as f32,
    );
    let (u0, u1) = (rx / surf_w, (rx + rw) / surf_w);
    let (v0, v1) = (ry / surf_h, (ry + rh) / surf_h);
    out.extend_from_slice(&[
        CompositeVertex { pos: [-1.0, 1.0], uv: [u0, v0], alpha },
        CompositeVertex { pos: [1.0, 1.0], uv: [u1, v0], alpha },
        CompositeVertex { pos: [1.0, -1.0], uv: [u1, v1], alpha },
        CompositeVertex { pos: [-1.0, 1.0], uv: [u0, v0], alpha },
        CompositeVertex { pos: [1.0, -1.0], uv: [u1, v1], alpha },
        CompositeVertex { pos: [-1.0, -1.0], uv: [u0, v1], alpha },
    ]);
}

/// Квад слитого пасса «вертикальный блюр + фильтры + композит» при
/// bbox-офскрине (BUG-405 срез 24): позиция — прямоугольник региона в
/// полноразмерной цели (device px → NDC), UV — весь офскрин (0..1).
pub(crate) fn push_region_dst_quad(
    out: &mut Vec<CompositeVertex>,
    region: [u32; 4],
    surf_w: f32,
    surf_h: f32,
    alpha: f32,
) {
    let (rx, ry, rw, rh) = (
        region[0] as f32,
        region[1] as f32,
        region[2] as f32,
        region[3] as f32,
    );
    let x0 = rx / surf_w * 2.0 - 1.0;
    let x1 = (rx + rw) / surf_w * 2.0 - 1.0;
    let y0 = 1.0 - ry / surf_h * 2.0;
    let y1 = 1.0 - (ry + rh) / surf_h * 2.0;
    out.extend_from_slice(&[
        CompositeVertex { pos: [x0, y0], uv: [0.0, 0.0], alpha },
        CompositeVertex { pos: [x1, y0], uv: [1.0, 0.0], alpha },
        CompositeVertex { pos: [x1, y1], uv: [1.0, 1.0], alpha },
        CompositeVertex { pos: [x0, y0], uv: [0.0, 0.0], alpha },
        CompositeVertex { pos: [x1, y1], uv: [1.0, 1.0], alpha },
        CompositeVertex { pos: [x0, y1], uv: [0.0, 1.0], alpha },
    ]);
}

/// Конвертирует декодированное изображение в плотный `Rgba8Unorm`-буфер.
/// Gray → серый × 3, alpha = 255. GrayA → серый × 3, alpha из канала.
/// Rgb → opaque (alpha = 255). Rgba — копия.
pub(crate) fn convert_to_rgba(image: &Image) -> Vec<u8> {
    let pixel_count = (image.width as usize) * (image.height as usize);
    let mut out = Vec::with_capacity(pixel_count * 4);
    match image.format {
        PixelFormat::Gray8 => {
            for &g in &image.data {
                out.extend_from_slice(&[g, g, g, 255]);
            }
        }
        PixelFormat::GrayAlpha8 => {
            for pair in image.data.chunks_exact(2) {
                let g = pair[0];
                let a = pair[1];
                out.extend_from_slice(&[g, g, g, a]);
            }
        }
        PixelFormat::Rgb8 => {
            for triple in image.data.chunks_exact(3) {
                out.extend_from_slice(&[triple[0], triple[1], triple[2], 255]);
            }
        }
        PixelFormat::Rgba8 => {
            out.extend_from_slice(&image.data);
        }
    }
    out
}

/// CSS Fonts L4 §7 + OpenType spec — нормализует user-space variation axes
/// в per-fvar-axis normalized coords `[-1.0, 1.0]`, затем применяет avar.
///
/// Возвращает пустой Vec для non-variable fonts (нет таблицы `fvar`) или
/// если `axes` пустой — renderer тогда использует default-instance.
pub(crate) fn normalize_variation_axes(face: &ParsedFace<'_>, axes: &[([u8; 4], f32)]) -> Vec<f32> {
    if axes.is_empty() {
        return Vec::new();
    }
    let fvar = match face.font.fvar() {
        Ok(f) if f.is_variable() => f,
        _ => return Vec::new(),
    };
    let avar = face.font.avar().unwrap_or_default();
    let mut coords = Vec::with_capacity(fvar.axes.len());
    for (axis_idx, axis) in fvar.axes.iter().enumerate() {
        let user_val = axes
            .iter()
            .find(|(tag, _)| tag == &axis.tag)
            .map_or(axis.default, |(_, v)| *v);
        let clamped = axis.clamp(user_val);
        let linear = if (clamped - axis.default).abs() < f32::EPSILON {
            0.0
        } else if clamped < axis.default {
            let range = axis.default - axis.min;
            if range < f32::EPSILON { 0.0 } else { (clamped - axis.default) / range }
        } else {
            let range = axis.max - axis.default;
            if range < f32::EPSILON { 0.0 } else { (clamped - axis.default) / range }
        };
        coords.push(avar.normalize(axis_idx, linear));
    }
    // CSS Fonts L4 §7.12: opsz injected by display_list builder into font_variation_axes
    // when font-optical-sizing: auto (default). normalize_variation_axes handles it here
    // like any other axis — no special case needed.
    coords
}

pub(crate) fn color_to_array(c: &Color) -> [f32; 4] {
    [
        c.r as f32 / 255.0,
        c.g as f32 / 255.0,
        c.b as f32 / 255.0,
        c.a as f32 / 255.0,
    ]
}

/// Scissor rect для wgpu в device pixels — все 4 компоненты u32 (× 16-битных,
/// но wgpu принимает u32). `set_scissor_rect(x, y, w, h)` обрезает все
/// последующие fragments в pass-е координатами окна. Пустой scissor
/// (`width=0` или `height=0`) запрещён wgpu и в нашем коде кодируется как
/// «ничего не рисуем» — caller проверяет `is_empty()` и пропускает draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DeviceScissor {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl DeviceScissor {
    /// Полный фрейм — scissor = вся область surface. wgpu reset = установить
    /// scissor в (0,0,W,H) перед draw.
    pub(crate) fn full(surface_w: u32, surface_h: u32) -> Self {
        Self { x: 0, y: 0, width: surface_w, height: surface_h }
    }

    /// Пустой scissor нельзя задать в wgpu — caller обязан проверить и
    /// пропустить draw. Возвращается из `from_css`, когда clip-rect пуст
    /// (после intersection всё схлопнулось до 0).
    pub(crate) fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }
}

/// Пересечение двух прямоугольников в CSS-px (origin-top-left). Пустое
/// пересечение представляется как `Rect { width: 0.0, height: 0.0 }` —
/// `is_empty_rect` это распознаёт. Используется для combine-логики стека
/// `PushClipRect` (новый scissor = пересечение с текущим), CSS Masking L1 §3.
pub(crate) fn intersect_rects(a: Rect, b: Rect) -> Rect {
    let x0 = a.x.max(b.x);
    let y0 = a.y.max(b.y);
    let x1 = (a.x + a.width).min(b.x + b.width);
    let y1 = (a.y + a.height).min(b.y + b.height);
    if x1 <= x0 || y1 <= y0 {
        Rect::new(x0, y0, 0.0, 0.0)
    } else {
        Rect::new(x0, y0, x1 - x0, y1 - y0)
    }
}

/// Активный blend mode из стека (CSS Compositing & Blending L1 §5): топ стека.
/// Пустой стек = `BlendMode::Normal` (стандарт; источник без blend-group).
#[allow(dead_code)]
pub(crate) fn current_blend_mode(blend_mode_stack: &[BlendMode]) -> BlendMode {
    blend_mode_stack.last().copied().unwrap_or(BlendMode::Normal)
}

/// Маппинг `BlendMode` в u32 для WGSL-uniform `blend_mode` в `BLEND_SHADER_SRC`.
/// Значение 0 (Normal) в теории не должно попасть в blend-pipeline (guard
/// `mode != Normal` в compositing path), но обработано как identity для устойчивости.
pub(crate) fn blend_mode_to_u32(mode: BlendMode) -> u32 {
    match mode {
        BlendMode::Normal      => 0,
        BlendMode::Multiply    => 1,
        BlendMode::Screen      => 2,
        BlendMode::Overlay     => 3,
        BlendMode::Darken      => 4,
        BlendMode::Lighten     => 5,
        BlendMode::ColorDodge  => 6,
        BlendMode::ColorBurn   => 7,
        BlendMode::HardLight   => 8,
        BlendMode::SoftLight   => 9,
        BlendMode::Difference  => 10,
        BlendMode::Exclusion   => 11,
        BlendMode::Hue         => 12,
        BlendMode::Saturation  => 13,
        BlendMode::Color       => 14,
        BlendMode::Luminosity  => 15,
        BlendMode::PlusLighter => 16,
    }
}

/// Применяет alpha-multiplier к RGBA-вершине: `color.a *= alpha`. Используется
/// для fill / text вершин перед записью в vbuf. `apply_alpha(c, 1.0) == c`
/// (no-op для opacity:1 — общий путь).
pub(crate) fn apply_alpha_to_color(color: [f32; 4], alpha: f32) -> [f32; 4] {
    [color[0], color[1], color[2], color[3] * alpha]
}

// Dash/dot геометрия для outline — общая для всех бэкендов (PA-1).
pub(crate) use crate::dash_math::dash_segments;

/// Рисует одну сторону border (top / right / bottom / left) с учётом
/// `BorderStyle`. Логика идентична `emit_outline_side` (Solid → один
/// full-rect, Dashed → pattern `(2w, w)`, Dotted → `(w, w)`), но без
/// «угловых ears» (border-стороны останавливаются у corner-ов и
/// overlap-ятся как fill-rect-ы — это нормально пока border-color
/// одинаков с обеих сторон угла). Phase 0 `BorderStyle::None`
/// фильтруется emit-side через `is_visible()`, но обрабатываем для
/// устойчивости.
pub(crate) fn emit_border_side(
    out: &mut Vec<FillVertex>,
    circle_out: &mut Vec<CircleVertex>,
    side_rect: Rect,
    horizontal: bool,
    width: f32,
    color: [f32; 4],
    style: BorderStyle,
) {
    let total = if horizontal { side_rect.width } else { side_rect.height };
    match style {
        BorderStyle::Dashed => {
            // Сегменты считает общий crate::dash_math (PA-1): dash=max(6,2w),
            // gap=max(4,w), floor-snapping — совпадает с Edge/Skia.
            for (offset, len) in crate::dash_math::dashed_border_offsets(total, width) {
                let seg = if horizontal {
                    Rect::new(side_rect.x + offset, side_rect.y, len, side_rect.height)
                } else {
                    Rect::new(side_rect.x, side_rect.y + offset, side_rect.width, len)
                };
                push_fill_quad(out, seg, color);
            }
        }
        BorderStyle::Dotted => {
            // Сегменты считает общий crate::dash_math (PA-1): симметричный
            // Bresenham-паттерн Edge. For dot_len ≤ 2px: use fill_quad
            // (rectangle) instead of SDF circle — Chrome/Edge renders thin
            // dotted borders as squares, not antialiased circles.
            let use_rect = width.max(1.0) <= 2.0;
            for (offset, len) in crate::dash_math::dotted_border_offsets(total, width) {
                let seg = if horizontal {
                    Rect::new(side_rect.x + offset, side_rect.y, len, side_rect.height)
                } else {
                    Rect::new(side_rect.x, side_rect.y + offset, side_rect.width, len)
                };
                if use_rect {
                    push_fill_quad(out, seg, color);
                } else {
                    push_circle_quad(circle_out, seg, color);
                }
            }
        }
        BorderStyle::Double => {
            // CSS Backgrounds L3 §4.2: two solid lines ~1/3 width each, gap ~1/3.
            // Width < 3px: no room for gap, fall back to solid.
            if width < 3.0 {
                push_fill_quad(out, side_rect, color);
                return;
            }
            let line = (width / 3.0).max(1.0);
            let (r1, r2) = if horizontal {
                (
                    Rect::new(side_rect.x, side_rect.y, side_rect.width, line),
                    Rect::new(side_rect.x, side_rect.y + width - line, side_rect.width, line),
                )
            } else {
                (
                    Rect::new(side_rect.x, side_rect.y, line, side_rect.height),
                    Rect::new(side_rect.x + width - line, side_rect.y, line, side_rect.height),
                )
            };
            push_fill_quad(out, r1, color);
            push_fill_quad(out, r2, color);
        }
        BorderStyle::Solid | BorderStyle::None => {
            push_fill_quad(out, side_rect, color);
        }
    }
}

/// Рисует одну сторону outline (top / right / bottom / left) с учётом
/// `OutlineStyle`. `horizontal=true` для top/bottom (даш-pattern идёт
/// по X), `false` для left/right (по Y). `width` — толщина outline
/// (CSS px), используется как dash/dot длина. Для Solid/Auto/None —
/// один full-rect; для Dashed — pattern `(2w, w)`; для Dotted — `(w, w)`.
pub(crate) fn emit_outline_side(
    out: &mut Vec<FillVertex>,
    circle_out: &mut Vec<CircleVertex>,
    side_rect: Rect,
    horizontal: bool,
    width: f32,
    color: [f32; 4],
    style: OutlineStyle,
) {
    let total = if horizontal { side_rect.width } else { side_rect.height };
    match style {
        OutlineStyle::Dashed => {
            let dash_len = (width * 3.0).max(1.0);
            let gap_len = width.max(1.0);
            for (offset, len) in dash_segments(total, dash_len, gap_len) {
                let seg = if horizontal {
                    Rect::new(side_rect.x + offset, side_rect.y, len, side_rect.height)
                } else {
                    Rect::new(side_rect.x, side_rect.y + offset, side_rect.width, len)
                };
                push_fill_quad(out, seg, color);
            }
        }
        OutlineStyle::Dotted => {
            let dot_len = width.max(1.0);
            for (offset, len) in dash_segments(total, dot_len, dot_len) {
                let seg = if horizontal {
                    Rect::new(side_rect.x + offset, side_rect.y, len, side_rect.height)
                } else {
                    Rect::new(side_rect.x, side_rect.y + offset, side_rect.width, len)
                };
                push_circle_quad(circle_out, seg, color);
            }
        }
        // Solid / Auto / None — full-length rect.
        OutlineStyle::Solid | OutlineStyle::Auto | OutlineStyle::None => {
            push_fill_quad(out, side_rect, color);
        }
    }
}

/// Перед draw-командой убедиться, что в `ops` стоит актуальный `SetScissor`
/// для текущего `clip_stack` (топ стека = пересечение всех Push-ов).
/// Возвращает `false`, если scissor пуст (`width==0` || `height==0`) — caller
/// обязан пропустить draw, wgpu иначе паникует на set_scissor_rect(0,0,0,0).
/// `current_scissor=None` означает, что `SetScissor` ещё не выставлялся
/// в этом render-loop-е — тогда команда добавляется даже если desired==full
/// (нет гарантии, что предыдущий кадр оставил scissor на полный размер).
/// `extra` — клип, не входящий в CSS-стек: кромка кольцевой полосы (BUG-405
/// срез 32). Он пересекается с вершиной стека, но в сам стек не кладётся —
/// иначе он ушёл бы внутрь offscreen-уровней вместе с `PushClipRect`, а туда
/// ему нельзя (уровень обязан рисоваться целиком, обрезается его композит).
pub(crate) fn sync_scissor_to_stack(
    clip_stack: &[Rect],
    extra: Option<Rect>,
    current_scissor: &mut Option<DeviceScissor>,
    ops: &mut Vec<DrawOp>,
    dpr: f32,
    surface_w: u32,
    surface_h: u32,
) -> bool {
    let css = match (clip_stack.last().copied(), extra) {
        (Some(a), Some(b)) => Some(intersect_rects(a, b)),
        (a, b) => a.or(b),
    };
    let desired = match css {
        Some(rect) => css_rect_to_device_scissor(rect, dpr, surface_w, surface_h),
        None => DeviceScissor::full(surface_w, surface_h),
    };
    if Some(desired) != *current_scissor {
        ops.push(DrawOp::SetScissor(desired));
        *current_scissor = Some(desired);
    }
    !desired.is_empty()
}

/// CSS-px rect → device-px scissor с учётом DPR и Y-axis inversion для wgpu.
/// Шейдер у нас работает в CSS px (viewport = surface / dpr); scissor wgpu
/// работает в device px (Y top-left). Округление: внешние границы наружу
/// (`floor` для x/y, `ceil` для right/bottom) — чтобы scissor НЕ обрезал
/// край pixel-perfect содержимого внутри clip-rect-а. Затем clamp в
/// `[0, surface_*]`. Пустой результат — `is_empty()`-флаг.
pub(crate) fn css_rect_to_device_scissor(
    rect: Rect,
    dpr: f32,
    surface_w: u32,
    surface_h: u32,
) -> DeviceScissor {
    let dpr = dpr.max(1e-6);
    let x0 = (rect.x * dpr).floor().max(0.0);
    let y0 = (rect.y * dpr).floor().max(0.0);
    let x1 = ((rect.x + rect.width) * dpr).ceil().max(0.0);
    let y1 = ((rect.y + rect.height) * dpr).ceil().max(0.0);
    let sw = surface_w as f32;
    let sh = surface_h as f32;
    let cx0 = x0.min(sw) as u32;
    let cy0 = y0.min(sh) as u32;
    let cx1 = x1.min(sw) as u32;
    let cy1 = y1.min(sh) as u32;
    DeviceScissor {
        x: cx0,
        y: cy0,
        width: cx1.saturating_sub(cx0),
        height: cy1.saturating_sub(cy0),
    }
}

/// Создаёт отдельный UNIFORM-буфер с параметрами одного filter pass.
/// Каждый filter render pass должен иметь СОБСТВЕННЫЙ буфер, так как
/// wgpu батчит все `queue.write_buffer` перед encoder-командами: записи
/// в один shared буфер переписывают друг друга и все проходы видят
/// только последнее значение.
pub(crate) fn make_filter_param_buf(device: &wgpu::Device, params: &FilterParamsCpu) -> wgpu::Buffer {
    let buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("filter-pass-param"),
        size: std::mem::size_of::<FilterParamsCpu>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: true,
    });
    buf.slice(..).get_mapped_range_mut().copy_from_slice(as_bytes(std::slice::from_ref(params)));
    buf.unmap();
    buf
}

/// Создаёт отдельный UNIFORM-буфер с режимом blend одного composite pass —
/// тот же приём и по той же причине, что [`make_filter_param_buf`] (BUG-277 срез 2).
pub(crate) fn make_blend_mode_param_buf(device: &wgpu::Device, mode_padded: &[u32; 4]) -> wgpu::Buffer {
    let buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("blend-mode-param"),
        size: std::mem::size_of::<[u32; 4]>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: true,
    });
    buf.slice(..).get_mapped_range_mut().copy_from_slice(as_bytes(mode_padded.as_slice()));
    buf.unmap();
    buf
}

/// Создаёт отдельный UNIFORM-буфер с параметрами одного blur pass —
/// тот же приём и по той же причине, что [`make_filter_param_buf`] (BUG-277 срез 7).
/// Разделяемый `blur_uniform` делал ГОРИЗОНТАЛЬНЫЙ проход вертикальным:
/// H и V пишутся до `submit`, побеждает последняя запись — и обе половины
/// сепарабельной свёртки шли по одной оси.
pub(crate) fn make_blur_param_buf(device: &wgpu::Device, params: &BlurParamsCpu) -> wgpu::Buffer {
    let buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("blur-pass-param"),
        size: std::mem::size_of::<BlurParamsCpu>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: true,
    });
    buf.slice(..).get_mapped_range_mut().copy_from_slice(as_bytes(std::slice::from_ref(params)));
    buf.unmap();
    buf
}

// SAFETY: T: Copy + #[repr(C)] плюс отсутствие padding-байт делают этот
// каст безопасным. Используется только для POD-типов из этого файла.
/// BUG-405 срез 4: для каждой команды списка — можно ли обслужить её
/// скруглённый клип шейдером (без offscreen-уровня). `true` стоит ровно на
/// индексах `PushClipRoundedRect`, чьё поддерево не содержит команды, чей
/// композит читает УЖЕ СОБРАННЫЙ слой и потому клипу не подчиняется:
/// фильтр (размывает за контур), backdrop-фильтр (берёт фон родителя),
/// маска и `mix-blend-mode` (читают цель пасса).
///
/// Остальное безопасно: содержимое вложенного уровня (opacity, вложенный
/// клип) рисуется теми же шейдерами и получает клип пофрагментно, а его
/// композит переносит уже обрезанный слой 1:1.
pub(crate) fn shader_rrect_clip_allowed(cmds: &[DisplayCommand]) -> Vec<bool> {
    let mut ok = vec![false; cmds.len()];
    // Индексы открытых `PushClip*`: `Some(i)` — скруглённый (кандидат),
    // `None` — прямоугольный/по форме (парный `PopClip` тот же).
    let mut open: Vec<Option<usize>> = Vec::new();
    for (i, cmd) in cmds.iter().enumerate() {
        match cmd {
            DisplayCommand::PushClipRoundedRect { radii, .. } => {
                let candidate = radii.iter().any(|r| *r > 0.0);
                if candidate {
                    ok[i] = true;
                }
                open.push(candidate.then_some(i));
            }
            DisplayCommand::PushClipRect { .. } | DisplayCommand::PushClipPath { .. } => {
                open.push(None);
            }
            DisplayCommand::PopClip => {
                open.pop();
            }
            DisplayCommand::PushFilter { .. }
            | DisplayCommand::PushBackdropFilter { .. }
            | DisplayCommand::PushBlendMode { .. }
            | DisplayCommand::PushMaskImage { .. }
            | DisplayCommand::PushMaskLinearGradient { .. }
            | DisplayCommand::PushMaskRadialGradient { .. }
            | DisplayCommand::PushMaskConicGradient { .. }
            | DisplayCommand::PushMaskLayer { .. } => {
                for slot in open.iter().flatten() {
                    ok[*slot] = false;
                }
            }
            _ => {}
        }
    }
    ok
}

pub(crate) fn as_bytes<T: Copy>(slice: &[T]) -> &[u8] {
    // SAFETY: the produced slice has exactly `size_of_val(slice)` bytes and borrows
    // `slice`, so it cannot outlive it or alias mutably. Reading `T` as bytes is
    // sound only for a type without padding or uninitialised bytes — the `Copy`
    // bound cannot express that, so it is a precondition on the caller. Every call
    // site here passes a `#[repr(C)]` POD vertex/uniform struct on its way into a
    // wgpu buffer, which is what the requirement amounts to.
    unsafe {
        std::slice::from_raw_parts(slice.as_ptr() as *const u8, std::mem::size_of_val(slice))
    }
}

/// Маленький block_on, чтобы не тащить tokio/pollster ради двух async-вызовов
/// в `Renderer::new`. На request_adapter / request_device обычно сразу `Ready`.
pub(crate) fn block_on<F: std::future::Future>(future: F) -> F::Output {
    use std::pin::pin;
    use std::sync::Arc;
    use std::task::{Context, Poll, Wake, Waker};
    use std::thread;

    struct ThreadWaker(thread::Thread);
    impl Wake for ThreadWaker {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }

    let waker = Waker::from(Arc::new(ThreadWaker(thread::current())));
    let mut cx = Context::from_waker(&waker);
    let mut future = pin!(future);
    loop {
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(v) => return v,
            Poll::Pending => thread::park(),
        }
    }
}

