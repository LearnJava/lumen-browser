//! P1/SPLIT-RN2: подстатьи `svg-sub`/`text-sub` покадрового лога ([`SvgSubStats`]/
//! [`TextSubStats`]) и glyph-часть free-fn хвоста `renderer.rs` — укладка текстового
//! run-а с мемоизацией ([`TextRunCache`]) и растеризация глифа в атлас
//! (`push_glyph_quad`…`rasterize_and_insert`). Вынесено из `renderer.rs`
//! (`docs/tasks/p1-monolith-split-queue.md` §4, группа RN, батч RN-2).

pub(crate) use crate::shaped_width::shaping_features;
use crate::shaped_width::text_shaping_disabled;
use super::*;

/// BUG-405 срез 12: подстатьи одной команды SVG-супа (`DrawSvgFill`/
/// `DrawSvgStroke`) под `LUMEN_FRAME_LOG=3`.
///
/// Срез 9 назвал статью целиком («`DrawSvgStroke` — 16.9 мкс на команду против
/// 1.9 у `DrawText`»), но не сказал, ЧТО внутри команды столько стоит после
/// мемоизации покрытия. Разбивка отделяет тесселяцию (пересчёт одной и той же
/// геометрии каждый кадр) от работы вокруг кэша: сборка супа в device px,
/// хэш+побитовое сравнение ключа, сам пересчёт покрытия на промахе, укладка
/// готовых квадов обратно в вершины.
///
/// Все счётчики — наносекунды, накопленные ЗА КАДР (сбрасываются в начале фазы
/// `collect`); заполняются только на уровне 3, иначе не берётся даже
/// `Instant::now()` — по той же причине, что у `collect-top`.
pub(crate) struct SvgSubStats {
    /// Вся ИТЕРАЦИЯ цикла `collect` для SVG-команды — от метки счётчика до
    /// конца тела цикла. Охватывает [`SvgSubStats::arm`] и кулинг; разность с
    /// ними называет работу, которая делается над командой вне её arm'а
    /// (срез 16).
    pub(crate) iter: std::sync::atomic::AtomicU64,
    /// Весь arm команды `DrawSvgFill`/`DrawSvgStroke` целиком — охватывающая
    /// статья, разность с суммой остальных называет остаток (срез 16).
    pub(crate) arm: std::sync::atomic::AtomicU64,
    /// `sync_scissor_to_stack` на команду.
    pub(crate) sciss: std::sync::atomic::AtomicU64,
    /// Поиск готовой фигуры в [`SvgShapeCache`]: хэш ключа, побитовое сравнение
    /// и клон `Arc` — цена попадания ПОВЕРХ сборки ключа ([`SvgSubStats::key`]).
    pub(crate) look: std::sync::atomic::AtomicU64,
    /// Тесселяция контуров в треугольный суп.
    pub(crate) tess: std::sync::atomic::AtomicU64,
    /// Укладка супа в `fill_vertices` со сдвигом и накопленной матрицей.
    pub(crate) push: std::sync::atomic::AtomicU64,
    /// Сборка супа в device px перед обращением к кэшу покрытия.
    pub(crate) soup: std::sync::atomic::AtomicU64,
    /// Хэш супа и побитовое сравнение с ключом (цена попадания).
    pub(crate) key: std::sync::atomic::AtomicU64,
    /// `coverage_quads` — CPU-растеризация покрытия (цена промаха).
    pub(crate) calc: std::sync::atomic::AtomicU64,
    /// Укладка готовых квадов обратно в `fill_vertices`.
    pub(crate) emit: std::sync::atomic::AtomicU64,
    /// Сколько команд SVG-супа прошло через тесселяцию за кадр.
    pub(crate) calls: std::sync::atomic::AtomicU64,
    /// Сколько вершин супа они дали суммарно.
    verts: std::sync::atomic::AtomicU64,
    /// Сколько вершин уложено в `fill_vertices` ([`emit_svg_shape`]) — размер
    /// готовой фигуры, единственное, на что нормируется укладка (срез 16).
    pub(crate) emitv: std::sync::atomic::AtomicU64,
    /// Сколько точек контуров пришло на вход — размер ключа мемоизации фигур
    /// против размера супа (`verts`), который ключом служил в срезе 9.
    pts: std::sync::atomic::AtomicU64,
    /// Команд, обслуженных готовой фигурой из [`SvgShapeCache`].
    pub(crate) hit: std::sync::atomic::AtomicU64,
    /// Команд, пересчитавших фигуру.
    pub(crate) miss: std::sync::atomic::AtomicU64,
}

/// Подстатьи SVG-команд текущего кадра ([`SvgSubStats`]).
pub(crate) static SVG_SUB: SvgSubStats = SvgSubStats {
    iter: std::sync::atomic::AtomicU64::new(0),
    arm: std::sync::atomic::AtomicU64::new(0),
    sciss: std::sync::atomic::AtomicU64::new(0),
    look: std::sync::atomic::AtomicU64::new(0),
    tess: std::sync::atomic::AtomicU64::new(0),
    push: std::sync::atomic::AtomicU64::new(0),
    soup: std::sync::atomic::AtomicU64::new(0),
    key: std::sync::atomic::AtomicU64::new(0),
    calc: std::sync::atomic::AtomicU64::new(0),
    emit: std::sync::atomic::AtomicU64::new(0),
    calls: std::sync::atomic::AtomicU64::new(0),
    verts: std::sync::atomic::AtomicU64::new(0),
    emitv: std::sync::atomic::AtomicU64::new(0),
    pts: std::sync::atomic::AtomicU64::new(0),
    hit: std::sync::atomic::AtomicU64::new(0),
    miss: std::sync::atomic::AtomicU64::new(0),
};

impl SvgSubStats {
    /// Обнулить подстатьи перед кадром.
    pub(crate) fn reset(&self) {
        use std::sync::atomic::Ordering::Relaxed;
        for slot in [
            &self.iter,
            &self.arm,
            &self.sciss,
            &self.look,
            &self.tess,
            &self.push,
            &self.soup,
            &self.key,
            &self.calc,
            &self.emit,
            &self.calls,
            &self.verts,
            &self.emitv,
            &self.pts,
            &self.hit,
            &self.miss,
        ] {
            slot.store(0, Relaxed);
        }
    }

    /// Строка `svg-sub` для покадрового лога уровня 3.
    pub(crate) fn line(&self) -> String {
        use std::sync::atomic::Ordering::Relaxed;
        let ms = |slot: &std::sync::atomic::AtomicU64| slot.load(Relaxed) as f64 / 1e6;
        format!(
            "svg-sub: iter {:.2}ms arm {:.2} sciss {:.2} tess {:.2} push {:.2} soup {:.2} key {:.2} \
             look {:.2} calc {:.2} emit {:.2} | \
             команд {} / вершин {} / уложено {} / точек {} | фигуры {}/{}",
            ms(&self.iter),
            ms(&self.arm),
            ms(&self.sciss),
            ms(&self.tess),
            ms(&self.push),
            ms(&self.soup),
            ms(&self.key),
            ms(&self.look),
            ms(&self.calc),
            ms(&self.emit),
            self.calls.load(Relaxed),
            self.verts.load(Relaxed),
            self.emitv.load(Relaxed),
            self.pts.load(Relaxed),
            self.hit.load(Relaxed),
            self.miss.load(Relaxed),
        )
    }
}

/// Таймер подстатьи, снимающий себя на выходе из области видимости — нужен там,
/// где путь может выйти через `continue` (arm команды в фазе `collect`): иначе
/// именно интересные ветки (отказ scissor'а, нет метрик) теряли бы свой вклад.
pub(crate) struct SubTimer<'a> {
    /// Куда прибавить время.
    slot: &'a std::sync::atomic::AtomicU64,
    /// Момент входа.
    t0: std::time::Instant,
}

impl Drop for SubTimer<'_> {
    fn drop(&mut self) {
        sub_add(self.slot, self.t0);
    }
}

/// [`SubTimer`] на подстатью, если покадровый лог просит разбивку.
pub(crate) fn sub_timer(on: bool, slot: &std::sync::atomic::AtomicU64) -> Option<SubTimer<'_>> {
    on.then(|| SubTimer { slot, t0: std::time::Instant::now() })
}

/// Прибавить к подстатье покадрового лога ([`SVG_SUB`], [`TEXT_SUB`]) время,
/// прошедшее с `t0`.
pub(crate) fn sub_add(slot: &std::sync::atomic::AtomicU64, t0: std::time::Instant) {
    slot.fetch_add(t0.elapsed().as_nanos() as u64, std::sync::atomic::Ordering::Relaxed);
}

/// Учесть одну оттесселированную фигуру SVG: размер её супа и размер исходных
/// контуров. Команды считаются отдельно ([`SVG_SUB`]`.calls`): тесселяция —
/// удел промаха, а нормировать статью надо на ВСЕ команды, иначе плечо с
/// кэшем поделит своё время на одни промахи и покажет цену вчетверо больше.
pub(crate) fn count_svg_soup(verts: usize, contours: &[Vec<[f32; 2]>]) {
    use std::sync::atomic::Ordering::Relaxed;
    SVG_SUB.verts.fetch_add(verts as u64, Relaxed);
    SVG_SUB.pts.fetch_add(contours.iter().map(Vec::len).sum::<usize>() as u64, Relaxed);
}

/// BUG-405 срез 13: подстатьи одной команды `DrawText` под `LUMEN_FRAME_LOG=3`.
///
/// Срез 12 оставил `DrawText` крупнейшей статьёй фазы `collect` (16.8 мс на
/// 9977 команд прокрутки `lenta.ru`), но разбивки ВНУТРИ команды не было —
/// и без неё правка была бы угадыванием (пункт 16 остатка бага). Разбивка
/// отделяет работу над командой (scissor, аффинная матрица) от работы над
/// каждым символом: выбор face-а под кодпойнт, нормализация осей вариаций,
/// поиск глифа в атласе и укладка квада.
///
/// Все счётчики — наносекунды ЗА КАДР (сбрасываются в начале фазы `collect`),
/// заполняются только на уровне 3: `Instant::now()` на символ сравним с самой
/// работой, поэтому замеры уровня 2 обязаны остаться чистыми.
pub(crate) struct TextSubStats {
    /// Весь arm команды `DrawText` целиком — охватывающая статья, разность с
    /// суммой остальных называет остаток (то, что не попало ни в одну).
    pub(crate) arm: std::sync::atomic::AtomicU64,
    /// Вся укладка глифов ([`push_text_glyphs`]) целиком.
    run: std::sync::atomic::AtomicU64,
    /// Цикл по символам целиком (внутри [`TextSubStats::run`]).
    lp: std::sync::atomic::AtomicU64,
    /// Пролог команды: bin размера, метрики primary face-а, базовая линия и
    /// три per-run кэша.
    pre: std::sync::atomic::AtomicU64,
    /// Выбор face-а под кодпойнт (per-run кэш + [`pick_face_for_codepoint`]).
    pick: std::sync::atomic::AtomicU64,
    /// Нормализация осей вариаций под face (per-run кэш).
    coord: std::sync::atomic::AtomicU64,
    /// Шейпинг под-run-ов через `active_text_shaper` (TEXTQ-4) — цена промаха
    /// [`TextRunCache`], на тёплом кадре не платится.
    shape: std::sync::atomic::AtomicU64,
    /// Всё, что делается с символом после выбора face-а: проверка COLR, поиск
    /// глифа и укладка квада. Охватывает [`TextSubStats::look`] и
    /// [`TextSubStats::quad`] — разность называет проверку COLR и вызовы.
    glyf: std::sync::atomic::AtomicU64,
    /// Поиск глифа в кэше атласа — без цены промаха (она отдельной статьёй,
    /// [`GLYPH_RASTER_NANOS`], срез 3).
    look: std::sync::atomic::AtomicU64,
    /// Укладка квада глифа в вершины и сдвиг пера.
    quad: std::sync::atomic::AtomicU64,
    /// `sync_scissor_to_stack` на команду.
    pub(crate) sciss: std::sync::atomic::AtomicU64,
    /// Наложение накопленной матрицы на вершины команды.
    pub(crate) xform: std::sync::atomic::AtomicU64,
    /// Команд `DrawText`, дошедших до укладки глифов.
    pub(crate) cmds: std::sync::atomic::AtomicU64,
    /// Символов, пройденных циклом (включая `\t` и невидимые).
    chars: std::sync::atomic::AtomicU64,
    /// Квадов глифов, уложенных в вершины (цветной глиф даёт квад на слой).
    glyphs: std::sync::atomic::AtomicU64,
}

/// Подстатьи команд `DrawText` текущего кадра ([`TextSubStats`]).
pub(crate) static TEXT_SUB: TextSubStats = TextSubStats {
    arm: std::sync::atomic::AtomicU64::new(0),
    run: std::sync::atomic::AtomicU64::new(0),
    lp: std::sync::atomic::AtomicU64::new(0),
    pre: std::sync::atomic::AtomicU64::new(0),
    pick: std::sync::atomic::AtomicU64::new(0),
    coord: std::sync::atomic::AtomicU64::new(0),
    shape: std::sync::atomic::AtomicU64::new(0),
    glyf: std::sync::atomic::AtomicU64::new(0),
    look: std::sync::atomic::AtomicU64::new(0),
    quad: std::sync::atomic::AtomicU64::new(0),
    sciss: std::sync::atomic::AtomicU64::new(0),
    xform: std::sync::atomic::AtomicU64::new(0),
    cmds: std::sync::atomic::AtomicU64::new(0),
    chars: std::sync::atomic::AtomicU64::new(0),
    glyphs: std::sync::atomic::AtomicU64::new(0),
};

impl TextSubStats {
    /// Обнулить подстатьи перед кадром.
    pub(crate) fn reset(&self) {
        use std::sync::atomic::Ordering::Relaxed;
        for slot in [
            &self.arm,
            &self.run,
            &self.lp,
            &self.pre,
            &self.pick,
            &self.coord,
            &self.shape,
            &self.glyf,
            &self.look,
            &self.quad,
            &self.sciss,
            &self.xform,
            &self.cmds,
            &self.chars,
            &self.glyphs,
        ] {
            slot.store(0, Relaxed);
        }
    }

    /// Строка `text-sub` для покадрового лога уровня 3.
    pub(crate) fn line(&self) -> String {
        use std::sync::atomic::Ordering::Relaxed;
        let ms = |slot: &std::sync::atomic::AtomicU64| slot.load(Relaxed) as f64 / 1e6;
        format!(
            "text-sub: arm {:.2}ms run {:.2} loop {:.2} pre {:.2} pick {:.2} coord {:.2} shape {:.2} \
             glyf {:.2} look {:.2} quad {:.2} sciss {:.2} xform {:.2} | \
             команд {} / символов {} / квадов {}",
            ms(&self.arm),
            ms(&self.run),
            ms(&self.lp),
            ms(&self.pre),
            ms(&self.pick),
            ms(&self.coord),
            ms(&self.shape),
            ms(&self.glyf),
            ms(&self.look),
            ms(&self.quad),
            ms(&self.sciss),
            ms(&self.xform),
            self.cmds.load(Relaxed),
            self.chars.load(Relaxed),
            self.glyphs.load(Relaxed),
        )
    }
}

/// Кладёт два треугольника одного глифа: позиция от пера `pen_dev` и baseline-а
/// `baseline_dev`, UV — из atlas-записи. Вынесено, потому что монохромный путь и
/// каждый слой COLR-глифа кладут один и тот же quad, различаясь только
/// цветом и glyph-id.
///
/// Позиции приходят в device px и должны быть целыми (TEXTQ-1): левый край
/// bitmap-а `left` и верх `top` у растеризатора целые, `bitmap_scale == 1.0`
/// для всякого глифа, кроме сверхкрупного, поэтому углы квада лежат на целых
/// device px и выборка из атласа идёт 1:1. В вершины уходят CSS px —
/// деление на `dpr`.
fn push_glyph_quad(
    out: &mut Vec<TextVertex>,
    g: &CachedGlyph,
    pen_dev: f32,
    baseline_dev: f32,
    bitmap_scale: f32,
    dpr: f32,
    color: [f32; 4],
) {
    let x0d = pen_dev + g.left * bitmap_scale;
    let y0d = baseline_dev - g.top * bitmap_scale;
    let x1d = x0d + g.entry.width as f32 * bitmap_scale;
    let y1d = y0d + g.entry.height as f32 * bitmap_scale;
    let (x0, y0, x1, y1) = (x0d / dpr, y0d / dpr, x1d / dpr, y1d / dpr);
    let u0 = g.entry.atlas_x as f32 / ATLAS_DIM as f32;
    let v0 = g.entry.atlas_y as f32 / ATLAS_DIM as f32;
    let u1 = (g.entry.atlas_x + g.entry.width) as f32 / ATLAS_DIM as f32;
    let v1 = (g.entry.atlas_y + g.entry.height) as f32 / ATLAS_DIM as f32;
    out.extend_from_slice(&[
        TextVertex { pos: [x0, y0], z: 0.0, uv: [u0, v0], color },
        TextVertex { pos: [x1, y0], z: 0.0, uv: [u1, v0], color },
        TextVertex { pos: [x1, y1], z: 0.0, uv: [u1, v1], color },
        TextVertex { pos: [x0, y0], z: 0.0, uv: [u0, v0], color },
        TextVertex { pos: [x1, y1], z: 0.0, uv: [u1, v1], color },
        TextVertex { pos: [x0, y1], z: 0.0, uv: [u0, v1], color },
    ]);
}

/// Шаг укладки текстового run-а — то, что цикл по символам делает с пером.
///
/// Разбивка `text-sub` (срез 13) показала, что из 16.8 мс команд `DrawText` на
/// прокрутке `lenta.ru` собственно укладка квадов — 2.9 мс, а остальное уходит
/// на то, ЧТО класть: выбор face-а под кодпойнт, нормализацию осей вариаций и
/// поиск глифа в атласе — три хэш-таблицы на каждый символ. Ответ на все три
/// вопроса зависит только от входа команды, а он на прокрутке повторяется:
/// один и тот же заголовок перекладывается каждый кадр заново.
#[derive(Clone, Copy)]
enum TextRunStep {
    /// Положить квад глифа и сдвинуть перо на `advance`.
    Glyph {
        /// Готовая запись атласа с метриками.
        g: CachedGlyph,
        /// Сдвиг пера, уже домноженный на `font_size / units_per_em` face-а,
        /// с которого взят глиф.
        advance: f32,
        /// Целая device-позиция пера глифа от `floor(rect.x × dpr)`, уже
        /// привязанная к сетке (TEXTQ-1); фаза пера зашита в `g`.
        pen_dev: i32,
        /// Подъём глифа над baseline на целые device px — `y_offset` GPOS
        /// (TEXTQ-4); `0.0` у подавляющего большинства глифов.
        y_off_dev: f32,
        /// Растяжение битмапа до нужного размера: `1.0`, кроме глифа крупнее
        /// `MAX_GLYPH_RASTER_PX`. У каждого глифа СВОЁ, а не общее на run —
        /// `size-adjust` (CSS Fonts L4 §14.4, FONTLOAD-17) масштабирует
        /// эффективный font-size лишь у face-а, которому принадлежит глиф, а
        /// `pick_face_for_codepoint` внутри одного run-а может выбирать разные
        /// face-ы для разных символов (fallback на несовпадающий unicode-range).
        bitmap_scale: f32,
    },
    /// Сдвинуть перо, ничего не кладя (табуляция или неотрисовавшийся глиф).
    Advance(f32),
}

/// Ось `font-variation-settings`: тег и значение.
type VariationAxis = ([u8; 4], f32);
/// Фича `font-feature-settings`: тег и значение.
type FontFeature = ([u8; 4], u32);

/// Оси вариаций face-а после слияния с дескриптором `@font-face` (в шейпер) и
/// их нормализованные координаты (в растеризатор).
type FaceAxesCoords = (Vec<([u8; 4], f32)>, Vec<f32>);

/// План укладки run-а: последовательность [`TextRunStep`] от пера в `rect.x`.
/// Позиции глифов в нём — относительно `floor(rect.x × dpr)`, а выбор фазы
/// зависит от дробной части `rect.x × dpr`, поэтому она и `dpr` входят в ключ
/// плана.
type TextRunPlan = std::sync::Arc<Vec<TextRunStep>>;

/// Запись [`TextRunCache`]: численная часть ключа, строка и её план.
type TextRunEntry = (Vec<u32>, Box<str>, TextRunPlan);

/// Сколько шагов планов держит [`TextRunCache`], прежде чем сбросить себя
/// целиком. Та же политика и тот же порядок, что у
/// [`SVG_SHAPE_CACHE_MAX_VERTS`]: страница живёт на единицах тысяч, сброс нужен
/// патологии — потоку НОВЫХ строк каждый кадр (счётчик, тикающий посимвольно).
const TEXT_RUN_CACHE_MAX_STEPS: usize = 1 << 18;

/// Мемоизация укладки целого текстового run-а (BUG-405 срез 13).
///
/// Ключ — ВХОД команды: сама строка, кегль, ширина табуляции, primary face и
/// оси вариаций. Всё, от чего зависит план, в ключе есть, поэтому попадание
/// возвращает ровно те шаги, которые вернул бы пересчёт; сравнение ключей
/// побитовое, коллизия хэша даёт промах, а не чужие глифы.
///
/// План хранит шаги, а не готовые вершины, именно чтобы попадание повторило
/// ТЕ ЖЕ операции над `f32` в ТОМ ЖЕ порядке, что и пересчёт: перо стартует с
/// `rect.x` и накапливает те же слагаемые, `push_glyph_quad` получает те же
/// аргументы. Сложение `f32` не ассоциативно — вынеси мы позицию из плана
/// (уложив run в начале координат и сдвинув потом), вершины разошлись бы на
/// ULP, и гейт перестал бы быть побайтовым.
///
/// **Цветной глиф (COLR) не кэшируется**: его квады зависят ещё и от палитры и
/// от цвета текста, а сам он редок (эмодзи). Такой run кладётся мимо кэша.
///
/// **Попадание не трогает атлас**, поэтому не обновляет `last_accessed` его
/// записей: под эвикцией по памяти (`atlas_on_memory_pressure`) давно
/// повторяющийся run может потерять свои глифы. Та же экспозиция, что у
/// `Renderer::cached_glyphs`, который тоже переживает эвикцию.
#[derive(Default)]
pub(crate) struct TextRunCache {
    /// Хэш ключа → ключи с таким хэшом и посчитанные по ним планы.
    buckets: std::collections::HashMap<u64, Vec<TextRunEntry>>,
    /// Переиспользуемый буфер под численную часть ключа — иначе каждая
    /// команда аллоцировала бы.
    scratch: Vec<u32>,
    /// Сколько слов ключей и шагов планов суммарно хранится.
    pub(crate) stored: usize,
    /// Сколько команд вернули готовый план.
    pub(crate) hits: u64,
    /// Сколько команд уложили run заново.
    pub(crate) misses: u64,
}

impl TextRunCache {
    /// Готовый план по ключу, если он есть, и хэш ключа для последующего
    /// [`TextRunCache::put`] — считать его второй раз незачем.
    fn get(&mut self, params: &[u32], text: &str) -> (Option<TextRunPlan>, u64) {
        let h = text_run_hash(params, text);
        let found = self
            .buckets
            .get(&h)
            .and_then(|bucket| {
                bucket
                    .iter()
                    .find(|(p, t, _)| p.as_slice() == params && &**t == text)
            })
            .map(|(_, _, plan)| std::sync::Arc::clone(plan));
        if found.is_some() {
            self.hits += 1;
        } else {
            self.misses += 1;
        }
        (found, h)
    }

    /// Запомнить план, уложенный по этому ключу (`h` — хэш, отданный `get`).
    fn put(&mut self, h: u64, params: &[u32], text: &str, plan: Vec<TextRunStep>) {
        let cost = params.len() + text.len() + plan.len();
        if self.stored + cost > TEXT_RUN_CACHE_MAX_STEPS {
            self.buckets.clear();
            self.stored = 0;
        }
        self.stored += cost;
        self.buckets.entry(h).or_default().push((
            params.to_vec(),
            Box::from(text),
            std::sync::Arc::new(plan),
        ));
    }

    /// Выбрасывает все планы. Обязателен при сбросе/эвикции атласа: планы
    /// держат готовые `CachedGlyph` со старыми координатами записей, и после
    /// сброса они указывали бы на пиксели уже других глифов (BUG-435).
    /// Счётчики попаданий/промахов процесса не трогаются — это статистика.
    pub(crate) fn clear(&mut self) {
        self.buckets.clear();
        self.stored = 0;
    }
}

/// Численная часть ключа run-а для [`TextRunCache`] — всё, от чего зависит
/// план укладки, кроме самой строки. Пишется в переиспользуемый буфер `out`.
///
/// Строка в буфер НЕ упаковывается. Первая редакция паковала её байты в слова
/// `u32`, и упаковка съедала заметную долю выигрыша от кэша — тот же класс,
/// что и ключ среза 12: строка хранится и сравнивается как есть, а в хэш идёт
/// побайтово.
fn build_text_run_key(
    out: &mut Vec<u32>,
    font_size: f32,
    tab_size: f32,
    (letter_spacing, word_spacing): (f32, f32),
    primary_face_id: usize,
    (axes, features): (&[VariationAxis], &[FontFeature]),
    (dpr, frac0): (f32, f32),
) {
    out.clear();
    out.push(dpr.to_bits());
    out.push(frac0.to_bits());
    out.push(font_size.to_bits());
    out.push(tab_size.to_bits());
    out.push(letter_spacing.to_bits());
    out.push(word_spacing.to_bits());
    out.push(primary_face_id as u32);
    out.push(axes.len() as u32);
    for (tag, value) in axes {
        out.push(u32::from_be_bytes(*tag));
        out.push(value.to_bits());
    }
    // TEXTQ-4: `font-feature-settings` меняет шейпинг (лигатуры, кернинг).
    out.push(features.len() as u32);
    for (tag, value) in features {
        out.push(u32::from_be_bytes(*tag));
        out.push(*value);
    }
}

/// FNV-1a по численной части ключа и байтам строки — та же дешёвая свёртка,
/// что у [`bits_hash`]; коллизия отсеивается сравнением самого ключа.
fn text_run_hash(params: &[u32], text: &str) -> u64 {
    let mut h = bits_hash(params);
    for b in text.as_bytes() {
        h = (h ^ u64::from(*b)).wrapping_mul(0x100_0000_01b3);
    }
    h
}

/// `true`, если мемоизация укладки текста отключена
/// (`LUMEN_NO_TEXT_RUN_CACHE=1`) — рычаг отката BUG-405 срез 13 к укладке
/// run-а на каждую команду.
fn text_run_cache_disabled() -> bool {
    use std::sync::OnceLock;
    static OFF: OnceLock<bool> = OnceLock::new();
    *OFF.get_or_init(|| std::env::var("LUMEN_NO_TEXT_RUN_CACHE").is_ok_and(|v| v == "1"))
}

/// Returns the final pen `x` (== `rect.x` + shaped advance) — used by
/// [`push_text_glyphs_mixed`] to measure a segment's real width without a
/// separate shaping pass.
#[allow(clippy::too_many_arguments)]
#[allow(clippy::expect_used)]  // унаследовано, docs/lint-policy.md §10
pub(crate) fn push_text_glyphs(
    out: &mut Vec<TextVertex>,
    rect: Rect,
    text: &str,
    font_size: f32,
    color: [f32; 4],
    primary_face_id: usize,
    lazy: &mut LazyParsedFaces<'_>,
    atlas: &mut GlyphAtlas,
    cached: &mut HashMap<AtlasKey, Option<CachedGlyph>>,
    runs: &mut TextRunCache,
    runs_enabled: bool,
    font_variation_axes: &[([u8; 4], f32)],
    font_features: &[([u8; 4], u32)],
    tab_size: f32,
    (letter_spacing, word_spacing): (f32, f32),
    font_palette: Option<&FontPaletteSelection>,
    dpr: f32,
) -> f32 {
    // BUG-405 срез 13: разбивка команды текста (`text-sub`, уровень 3).
    let log = crate::frame_log_level() >= 3;
    let _t_run = sub_timer(log, &TEXT_SUB.run);
    let t_pre = log.then(std::time::Instant::now);

    // Baseline: ascent / (ascent − descent) primary face-а. Для Inter ≈ 0.80.
    // Используем primary для всех глифов в run-е — иначе при смешивании
    // face-ов символы прыгали бы по вертикали.
    let primary_face = &lazy.faces[primary_face_id];
    let primary = primary_face
        .metrics
        .as_ref()
        .expect("primary face metrics must exist (checked by caller)");
    // CSS Fonts L4 §14 (FONTLOAD-17, BUG-467): ascent-override/
    // descent-override заменяют реальные hhea-метрики face-а везде, где их
    // читают — это правило распространяется и на позицию baseline внутри
    // `rect`, не только на line-box высоту, которую уже считает
    // override-aware layout-сторона (`PrimaryFontMetrics` в
    // `crates/engine/paint/src/lib.rs`). Подстановка на уровне font units
    // (а не готового px) сохраняет прежнюю формулу байт-в-байт, когда
    // override отсутствует — единственный путь, где overrides реально влияли
    // на диффы (`ascent-descent-override.html`, FONTLOAD-16) до этого среза
    // не читал их вовсе.
    let ascent_units = primary_face
        .ascent_override
        .map_or(primary.ascent as f32, |pct| pct * primary.units_per_em as f32);
    let descent_units = primary_face
        .descent_override
        .map_or(primary.descent as f32, |pct| -(pct * primary.units_per_em as f32));
    let ascent_ratio = ascent_units / (ascent_units - descent_units);
    // `size-adjust` (CSS Fonts L4 §14.4) премультиплицирует font-size ДО
    // вычисления override/реальных метрик — та же формула, что у glyph-сайза
    // ниже, применённая к primary face-у для позиции baseline.
    let primary_font_size = font_size * primary_face.size_adjust.unwrap_or(1.0);
    let baseline_y = rect.y + primary_font_size * ascent_ratio;
    // TEXTQ-1: baseline — на целый device px, перо по X — целая часть
    // `floor(rect.x × dpr)` плюс дробная `frac0`, из которой каждому глифу
    // выбирается фаза.
    let dpr = if dpr.is_finite() && dpr > 0.0 { dpr } else { 1.0 };
    let baseline_dev = (baseline_y * dpr).round();
    let base_dev = rect.x * dpr;
    let base_floor = base_dev.floor();
    let frac0 = base_dev - base_floor;

    // Per-char cache на длительность одного DrawText: одни и те же символы
    // в строке («the the the») не нужно пробовать через все face-ы каждый раз.
    let mut char_face_cache: HashMap<char, (usize, u16)> = HashMap::new();
    // Объединённые оси и normalized variation coords per face_id — лениво вычисляется при первом
    // обращении к данному face. Нормализация требует fvar+avar из шрифта
    // (единственный потребитель `ParsedFace` на пути без промахов атласа;
    // при пустых axes face не парсится вовсе).
    let mut norm_coords_cache: HashMap<usize, FaceAxesCoords> = HashMap::new();
    // CSS Fonts L4 §11.3 — развёрнутая палитра per face_id. Считается лениво:
    // у монохромного face-а (подавляющее большинство) `FaceMetrics.color` =
    // None и сюда не заходим ни разу.
    let mut palette_cache: HashMap<usize, Option<Vec<[f32; 4]>>> = HashMap::new();
    // BUG-405 срез 13: готовый план укладки этого run-а. Ключ строится по
    // входу команды — он на порядок меньше плана (18 байт строки против 18
    // шагов с записями атласа), поэтому спросить кэш дёшево.
    let cache_on = runs_enabled && !text_run_cache_disabled();
    let mut key = std::mem::take(&mut runs.scratch);
    let mut key_hash = 0_u64;
    let plan = if cache_on {
        build_text_run_key(
            &mut key,
            font_size,
            tab_size,
            (letter_spacing, word_spacing),
            primary_face_id,
            (font_variation_axes, font_features),
            (dpr, frac0),
        );
        let (found, h) = runs.get(&key, text);
        key_hash = h;
        found
    } else {
        None
    };
    if let Some(t0) = t_pre {
        sub_add(&TEXT_SUB.pre, t0);
        use std::sync::atomic::Ordering::Relaxed;
        TEXT_SUB.cmds.fetch_add(1, Relaxed);
        TEXT_SUB.chars.fetch_add(text.chars().count() as u64, Relaxed);
    }

    if let Some(plan) = plan {
        let _t_loop = sub_timer(log, &TEXT_SUB.lp);
        let mut cursor_x = rect.x;
        for step in plan.iter() {
            match step {
                TextRunStep::Glyph { g, advance, pen_dev, y_off_dev, bitmap_scale } => {
                    let t_quad = log.then(std::time::Instant::now);
                    let pen = base_floor + *pen_dev as f32;
                    push_glyph_quad(out, g, pen, baseline_dev - y_off_dev, *bitmap_scale, dpr, color);
                    cursor_x += advance;
                    if let Some(t0) = t_quad {
                        sub_add(&TEXT_SUB.quad, t0);
                        TEXT_SUB.glyphs.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    }
                }
                TextRunStep::Advance(advance) => cursor_x += advance,
            }
        }
        runs.scratch = key;
        return cursor_x;
    }

    // Промах: укладываем run и попутно записываем план. `None` означает, что
    // запоминать нечего — кэш выключен или в run-е попался цветной глиф.
    let mut plan: Option<Vec<TextRunStep>> = cache_on.then(Vec::new);
    let _t_loop = sub_timer(log, &TEXT_SUB.lp);
    let mut cursor_x = rect.x;
    // TEXTQ-4: фичи шейпинга — `font-feature-settings` плюс отказ от
    // необязательных лигатур при ненулевом `letter-spacing`.
    let shape_features = shaping_features(font_features, letter_spacing);
    // CSS Text L3 §10.1 — tab character advances by tab_size pixels. Шейпинг
    // идёт по отрезкам между табуляциями, чтобы табуляция не входила в
    // лигатуру или кернинговую пару.
    let segments: Vec<&str> = if tab_size > 0.0 { text.split('\t').collect() } else { vec![text] };
    for (seg_idx, segment) in segments.into_iter().enumerate() {
        if seg_idx > 0 {
            cursor_x += tab_size;
            if let Some(plan) = plan.as_mut() {
                plan.push(TextRunStep::Advance(tab_size));
            }
        }
        // Под-run-ы по резолвленному face-у: каскад выбирается на кодпойнт, а
        // лигатура не может пересечь границу двух face-ов.
        let mut sub_start = 0usize;
        let mut sub_face = usize::MAX;
        let mut boundaries: Vec<(usize, usize, usize)> = Vec::new();
        let t_pick = log.then(std::time::Instant::now);
        for (byte_idx, ch) in segment.char_indices() {
            let (face_id, _) = *char_face_cache
                .entry(ch)
                .or_insert_with(|| pick_face_for_codepoint(ch as u32, primary_face_id, lazy.faces));
            if sub_face == usize::MAX {
                sub_face = face_id;
            } else if face_id != sub_face {
                boundaries.push((sub_face, sub_start, byte_idx));
                sub_start = byte_idx;
                sub_face = face_id;
            }
        }
        if sub_face != usize::MAX {
            boundaries.push((sub_face, sub_start, segment.len()));
        }
        if let Some(t0) = t_pick {
            sub_add(&TEXT_SUB.pick, t0);
        }
        for (face_id, from, to) in boundaries {
            let sub_text = &segment[from..to];
            let face = &lazy.faces[face_id];
            let metrics = face
                .metrics
                .as_ref()
                .expect("pick_face_for_codepoint вернул face_id с валидными metrics");
            // CSS Fonts L4 §14.4 (FONTLOAD-17, BUG-467): size-adjust премультиплицирует
            // font-size для КОНКРЕТНОГО face-а — разные под-run-ы одного run-а могут
            // резолвиться в разные face-ы через `pick_face_for_codepoint`
            // (fallback по unicode-range), поэтому масштаб растеризации/дисплея
            // считается на под-run, а не на run (в отличие от baseline, который
            // намеренно общий на весь run — см. выше).
            let adjusted_font_size = font_size * face.size_adjust.unwrap_or(1.0);
            let advance_scale = adjusted_font_size / metrics.units_per_em as f32;
            // TEXTQ-1: глиф растрируется в точном размере в device px; перо
            // привязано к сетке — целая часть + фаза из 4 четвертей пикселя.
            let (glyph_size_q, bitmap_scale) = glyph_raster_size(adjusted_font_size, dpr);
            let t_coord = log.then(std::time::Instant::now);
            let (axes, coords): (&[([u8; 4], f32)], &[f32]) = {
                let (axes, coords) = match norm_coords_cache.entry(face_id) {
                    std::collections::hash_map::Entry::Occupied(e) => e.into_mut(),
                    std::collections::hash_map::Entry::Vacant(v) => {
                        let axes =
                            merge_variation_settings(&face.variation_settings, font_variation_axes);
                        let computed = if axes.is_empty() {
                            Vec::new()
                        } else if let Some(parsed_face) = lazy.get(face_id) {
                            normalize_variation_axes(parsed_face, &axes)
                        } else {
                            Vec::new()
                        };
                        v.insert((axes, computed))
                    }
                };
                (axes, coords)
            };
            if let Some(t0) = t_coord {
                sub_add(&TEXT_SUB.coord, t0);
            }
            let t_shape = log.then(std::time::Instant::now);
            let mut shaped = if text_shaping_disabled() {
                Vec::new()
            } else {
                lumen_font::active_text_shaper().shape(
                    &face.bytes,
                    sub_text,
                    lumen_core::ext::ShapeDirection::LeftToRight,
                    None,
                    &shape_features,
                    axes,
                )
            };
            if shaped.is_empty() {
                // Шейпер выключен рычагом или не принял face, который
                // распарсил наш парсер — укладываем по cmap/hmtx, как до
                // TEXTQ-4.
                shaped = cmap_shape(sub_text, metrics);
            }
            if let Some(t0) = t_shape {
                sub_add(&TEXT_SUB.shape, t0);
            }
            for (i, sg) in shaped.iter().enumerate() {
                let _t_glyf = sub_timer(log, &TEXT_SUB.glyf);
                let glyph_id = sg.glyph_id;
                // CSS Text L3 §8.2 — `letter-spacing` добавляется к КЛАСТЕРУ,
                // а не к глифу: лигатура из двух символов получает его один
                // раз, составной символ из базы и меток — тоже. Последний
                // глиф кластера — тот, за которым следует другой кластер.
                // `word-spacing` следует за разделителем слов (§8.1).
                let cluster_end = shaped.get(i + 1).is_none_or(|n| n.cluster != sg.cluster);
                let mut spacing = 0.0;
                if cluster_end {
                    spacing = letter_spacing;
                    if word_spacing != 0.0
                        && sub_text.get(sg.cluster as usize..).is_some_and(|t| t.starts_with(' '))
                    {
                        spacing += word_spacing;
                    }
                }
                let advance = sg.x_advance as f32 * advance_scale + spacing;
                // GPOS-смещение пера: по X — до привязки к сетке, по Y — на
                // целые device px (растр глифа выровнен по baseline).
                let x_off = sg.x_offset as f32 * advance_scale;
                let y_off_dev = (sg.y_offset as f32 * advance_scale * dpr).round();
                let pen_local = frac0 + (cursor_x - rect.x + x_off) * dpr;
                let (pen_int, x_phase) = snap_pen(pen_local, glyph_size_q);
                let pen_dev = base_floor + pen_int as f32;
                let glyph_baseline = baseline_dev - y_off_dev;
                // CSS Fonts L4 §11.3 — COLR v0 цветной глиф: вместо одного quad-а
                // текстовым цветом кладём по quad-у на каждый слой, снизу вверх, со
                // своим цветом из выбранной палитры. Слой — обычный монохромный
                // глиф, поэтому идёт через тот же атлас. Advance берём у базового
                // глифа (сам он не растеризуется — слои его полностью замещают).
                if let Some(layers) = metrics
                    .color
                    .as_ref()
                    .and_then(|c| c.colr.layers_for(glyph_id))
                {
                    let palette = palette_cache
                        .entry(face_id)
                        .or_insert_with(|| {
                            metrics.color.as_ref().and_then(|c| resolve_palette(c, font_palette))
                        })
                        .as_deref();
                    for layer in layers {
                        let Some(g) = ensure_glyph(
                            cached, atlas, lazy, face_id, layer.glyph_id, glyph_size_q, x_phase,
                            coords,
                        ) else {
                            continue;
                        };
                        let t_quad = log.then(std::time::Instant::now);
                        push_glyph_quad(
                            out,
                            &g,
                            pen_dev,
                            glyph_baseline,
                            bitmap_scale,
                            dpr,
                            layer_color(palette, layer.palette_index, color),
                        );
                        if let Some(t0) = t_quad {
                            sub_add(&TEXT_SUB.quad, t0);
                            TEXT_SUB.glyphs.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        }
                    }
                    cursor_x += advance;
                    // Цветной глиф зависит от палитры и цвета текста — run с ним
                    // мимо кэша целиком (см. [`TextRunCache`]).
                    plan = None;
                    continue;
                }

                let cached_glyph = ensure_glyph(
                    cached,
                    atlas,
                    lazy,
                    face_id,
                    glyph_id,
                    glyph_size_q,
                    x_phase,
                    coords,
                );

                if let Some(g) = cached_glyph {
                    let t_quad = log.then(std::time::Instant::now);
                    push_glyph_quad(out, &g, pen_dev, glyph_baseline, bitmap_scale, dpr, color);
                    cursor_x += advance;
                    if let Some(plan) = plan.as_mut() {
                        plan.push(TextRunStep::Glyph {
                            g,
                            advance,
                            pen_dev: pen_int,
                            y_off_dev,
                            bitmap_scale,
                        });
                    }
                    if let Some(t0) = t_quad {
                        sub_add(&TEXT_SUB.quad, t0);
                        TEXT_SUB.glyphs.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    }
                } else {
                    // Глиф не отрисовался (пробел, empty или нет места в
                    // атласе) — двигаем перо на advance из шейпера.
                    cursor_x += advance;
                    if let Some(plan) = plan.as_mut() {
                        plan.push(TextRunStep::Advance(advance));
                    }
                }
            }
        }
    }
    if let Some(plan) = plan {
        runs.put(key_hash, &key, text, plan);
    }
    runs.scratch = key;
    cursor_x
}

/// Укладка под-run-а без шейпера — глиф на кодпойнт из `cmap`, advance из
/// `hmtx`. Запасной путь на случай, когда rustybuzz не принял face (так
/// укладывался весь GPU-путь до TEXTQ-4).
fn cmap_shape(text: &str, metrics: &FaceMetrics) -> Vec<lumen_core::ext::ShapedGlyph> {
    text.char_indices()
        .map(|(byte_idx, ch)| {
            let glyph_id = metrics.cmap.glyph_index(ch as u32).unwrap_or(0);
            lumen_core::ext::ShapedGlyph {
                glyph_id,
                cluster: byte_idx as u32,
                x_advance: metrics.advances.get(glyph_id as usize).map_or(0, |&a| i32::from(a)),
                x_offset: 0,
                y_offset: 0,
            }
        })
        .collect()
}

/// Ph3 writing-mode vertical, Срез 2 — rotates a glyph run's vertices 90° CW
/// around the local origin and translates the result onto `dest`. Mirrors the
/// CPU rasterizer's `rasterize_text_rotated` transform
/// (`tiny_skia::Transform::from_row(0, 1, -1, 0, dest.x + dest.width, dest.y)`): a point
/// laid out horizontally at `(x, y)` maps to `(-y + dest.x + dest.width, x + dest.y)`.
/// The glyph top (`y = 0`) lands on the column's right edge and the body grows
/// leftwards into the column — anchoring at `dest.x` instead put the whole run
/// one line-height to the left of its box (BUG-553, срез 62).
/// Callers must have generated `verts` with `push_text_glyphs` at the local
/// origin `(0, 0)` — not at `dest`.
pub(crate) fn rotate_text_vertices_cw(verts: &mut [TextVertex], dest: Rect) {
    for v in verts {
        let (x, y) = (v.pos[0], v.pos[1]);
        v.pos = [-y + dest.x + dest.width, x + dest.y];
    }
}

/// Ph3 writing-mode vertical, Срез 3 — per-glyph split for `text-orientation:
/// mixed`, wgpu path: each CJK ideograph paints upright at an increasing
/// offset along `dest`'s column (no rotation — same as `push_text_glyphs`
/// generating straight into `dest`); each run of non-CJK characters shapes as
/// one block at the local origin, then [`rotate_text_vertices_cw`] maps it
/// onto `dest` starting at the same column offset. Mirrors the CPU
/// rasterizer's `rasterize_text_mixed`. `push_text_glyphs`'s returned pen
/// position gives each segment's real shaped width, so a whitespace-only
/// segment (no visible glyph, but still an advance) still moves the cursor.
#[allow(clippy::too_many_arguments)]
pub(crate) fn push_text_glyphs_mixed(
    out: &mut Vec<TextVertex>,
    dest: Rect,
    text: &str,
    font_size: f32,
    color: [f32; 4],
    primary_face_id: usize,
    lazy: &mut LazyParsedFaces<'_>,
    atlas: &mut GlyphAtlas,
    cached: &mut HashMap<AtlasKey, Option<CachedGlyph>>,
    runs: &mut TextRunCache,
    runs_enabled: bool,
    font_variation_axes: &[([u8; 4], f32)],
    font_features: &[([u8; 4], u32)],
    tab_size: f32,
    font_palette: Option<&FontPaletteSelection>,
    dpr: f32,
) {
    let mut y_cursor = 0.0_f32;
    for seg in crate::display_list::split_mixed_runs(text) {
        let (seg_text, upright) = match seg {
            crate::display_list::MixedSegment::Cjk(ch) => {
                let mut s = String::new();
                s.push(ch);
                (s, true)
            }
            crate::display_list::MixedSegment::Other(s) => (s, false),
        };
        if upright {
            let seg_rect = Rect::new(dest.x, dest.y + y_cursor, dest.width, dest.height);
            let end_x = push_text_glyphs(
                out, seg_rect, &seg_text, font_size, color, primary_face_id, lazy, atlas,
                cached, runs, runs_enabled, font_variation_axes, font_features, tab_size, (0.0, 0.0),
                font_palette, dpr,
            );
            y_cursor += end_x - dest.x;
        } else {
            let v_start = out.len();
            let local_rect = Rect::new(y_cursor, 0.0, dest.width, dest.height);
            let end_x = push_text_glyphs(
                out, local_rect, &seg_text, font_size, color, primary_face_id, lazy, atlas,
                cached, runs, runs_enabled, font_variation_axes, font_features, tab_size, (0.0, 0.0),
                font_palette, dpr,
            );
            rotate_text_vertices_cw(&mut out[v_start..], dest);
            y_cursor = end_x;
        }
    }
}

/// CSS Fonts L4 §5.3 — for each character cascade. Сначала пробуем primary
/// face; если `cmap.glyph_index` возвращает None или Some(0) (= .notdef) —
/// обходим остальные loaded faces. Если ни у кого нет — возвращаем
/// `(primary, 0)` (отрисовать .notdef из primary).
///
/// Работает на owned `FaceMetrics.cmap` — без парсинга шрифтов.
///
/// FONTLOAD-9 (BUG-467): каждый кандидат сначала фильтруется по заявленному
/// `unicode-range` (`LoadedFace::unicode_ranges`,
/// [`lumen_core::codepoint_in_face_ranges`]) — пустой список (нет дескриптора)
/// проходит фильтр всегда, поэтому системные face-ы и обычные web-шрифты не
/// затронуты. Раньше выбор шёл ИСКЛЮЧИТЕЛЬНО по покрытию cmap (решение
/// BUG-434, `subsystems/font.md`); это совпадает с заявленным диапазоном,
/// только когда файл-сабсет физически не содержит глифов вне него. WPT
/// `css/css-fonts/font-face-unicode-range.html` даёт двум `@font-face` с
/// непересекающимися диапазонами ОДИН И ТОТ ЖЕ файл шрифта именно чтобы
/// поймать реализацию, которая смотрит только на cmap — без этого фильтра
/// `face-family: NOIMPACT` (диапазон `U+0061-0064`, строчные) ошибочно
/// применялся бы и к заглавным `ABCDEFG`, потому что тот же файл содержит их
/// глифы под именем `IMPACT`.
fn pick_face_for_codepoint(
    cp: u32,
    primary_face_id: usize,
    faces: &[LoadedFace],
) -> (usize, u16) {
    if let Some(primary) = faces.get(primary_face_id)
        && lumen_core::codepoint_in_face_ranges(cp, &primary.unicode_ranges)
        && let Some(m) = primary.metrics.as_ref()
        && let Some(gid) = m.cmap.glyph_index(cp).filter(|&g| g != 0)
    {
        return (primary_face_id, gid);
    }
    for (idx, face) in faces.iter().enumerate() {
        if idx == primary_face_id {
            continue;
        }
        if !lumen_core::codepoint_in_face_ranges(cp, &face.unicode_ranges) {
            continue;
        }
        if let Some(m) = face.metrics.as_ref()
            && let Some(gid) = m.cmap.glyph_index(cp).filter(|&g| g != 0)
        {
            return (idx, gid);
        }
    }
    (primary_face_id, 0)
}

/// Merges a face's `font-variation-settings` `@font-face` descriptor
/// defaults (CSS Fonts L4 §6.2, FONTLOAD-20) with the element's
/// `font-variation-settings` CSS property axes, per axis: the property wins
/// where it names an axis, the descriptor's default applies where the
/// property doesn't mention that axis at all. Property-only (no descriptor
/// defaults) returns the property list unchanged — the common case, since
/// most `@font-face` rules carry no `font-variation-settings` descriptor.
fn merge_variation_settings(
    descriptor_defaults: &[([u8; 4], f32)],
    property_axes: &[([u8; 4], f32)],
) -> Vec<([u8; 4], f32)> {
    if descriptor_defaults.is_empty() {
        return property_axes.to_vec();
    }
    let mut merged = descriptor_defaults.to_vec();
    for &(tag, value) in property_axes {
        if let Some(slot) = merged.iter_mut().find(|(t, _)| *t == tag) {
            slot.1 = value;
        } else {
            merged.push((tag, value));
        }
    }
    merged
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn ensure_glyph(
    cached: &mut HashMap<AtlasKey, Option<CachedGlyph>>,
    atlas: &mut GlyphAtlas,
    lazy: &mut LazyParsedFaces<'_>,
    face_id: usize,
    glyph_id: u16,
    size_q: u16,
    x_phase: u8,
    coords: &[f32],
) -> Option<CachedGlyph> {
    // BUG-405 срез 13: поиск в кэше атласа — отдельной подстатьёй `text-sub`.
    // Цена промаха (растеризация) считается ниже своим счётчиком (срез 3),
    // поэтому таймер снимается ровно на границе попадания/промаха.
    let t_look = (crate::frame_log_level() >= 3).then(std::time::Instant::now);
    let key = atlas_key(face_id, glyph_id, size_q, x_phase, AtlasKey::hash_coords(coords));
    let hit = cached.get(&key).copied();
    if let Some(t0) = t_look {
        sub_add(&TEXT_SUB.look, t0);
    }
    if let Some(entry) = hit {
        return entry;
    }

    // Промах atlas-кэша — единственный путь, где нужен распарсенный шрифт
    // (outline + HVAR). Ленивый парс: на тёплом кадре сюда не заходим.
    let t0 = std::time::Instant::now();
    let face = lazy.get(face_id)?;
    let result = rasterize_and_insert(
        atlas,
        &face.font,
        face.head.units_per_em,
        key,
        coords,
    );
    // BUG-405 срез 3: цена промаха атласа — отдельной статьёй от обхода
    // display list (обе живут в фазе `collect`).
    GLYPHS_RASTERIZED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    GLYPH_RASTER_NANOS.fetch_add(
        t0.elapsed().as_nanos() as u64,
        std::sync::atomic::Ordering::Relaxed,
    );
    match result {
        GlyphRaster::Ready(g) => {
            cached.insert(key, Some(g));
            Some(g)
        }
        GlyphRaster::Empty => {
            // Глифу нечего рисовать (пробел, нет outline-а, растеризатор вернул
            // пусто) — свойство самого глифа, помним навсегда.
            cached.insert(key, None);
            None
        }
        // BUG-435: отказ по МЕСТУ мемоизировать нельзя. Раньше он попадал в
        // `cached` как `None` и держался там вечно — буква исчезала до конца
        // жизни процесса, в том числе в хроме, хотя атлас на старте следующего
        // кадра уже сброшен и место есть.
        GlyphRaster::OutOfSpace => None,
    }
}

/// Итог растеризации глифа в атлас (BUG-435): «нет места» отличается от
/// «класть нечего», потому что первое лечится сбросом атласа, а второе нет.
enum GlyphRaster {
    /// Глиф в атласе.
    Ready(CachedGlyph),
    /// Рисовать нечего — outline пуст/не Simple, растеризация не дала битмап.
    Empty,
    /// Атлас исчерпан.
    OutOfSpace,
}

fn rasterize_and_insert(
    atlas: &mut GlyphAtlas,
    font: &Font,
    units_per_em: u16,
    key: AtlasKey,
    coords: &[f32],
) -> GlyphRaster {
    // `glyph_resolved_with_coords` разворачивает composite в Simple
    // рекурсивно и применяет gvar deltas в указанной точке пространства
    // осей. Пустой coords (default-instance) → short-circuit на путь
    // `glyph_resolved` (для non-VF шрифтов или CSS без
    // `font-variation-settings`).
    let Some(glyph) = font.glyph_resolved_with_coords(key.glyph_id, coords).ok().flatten() else {
        return GlyphRaster::Empty;
    };
    if !matches!(glyph.outline, Outline::Simple(_)) {
        return GlyphRaster::Empty;
    }
    let raster = Rasterizer::new(f32::from(key.size_q) / 4.0, units_per_em);
    let dx = f32::from(key.x_phase) / f32::from(X_PHASES);
    let Some(bitmap): Option<Bitmap> = raster.rasterize_offset(&glyph, dx) else {
        return GlyphRaster::Empty;
    };
    let entry = match atlas.try_insert(key, &bitmap) {
        InsertOutcome::Inserted(entry) => entry,
        InsertOutcome::Rejected => return GlyphRaster::Empty,
        InsertOutcome::OutOfSpace => return GlyphRaster::OutOfSpace,
    };
    GlyphRaster::Ready(CachedGlyph {
        entry,
        left: bitmap.left,
        top: bitmap.top,
    })
}

#[cfg(test)]
mod pick_face_for_codepoint_tests {
    use super::*;

    static INTER: &[u8] = include_bytes!("../../../../../assets/fonts/Inter-Regular.ttf");

    fn face_with_range(ranges: Vec<(u32, u32)>) -> LoadedFace {
        LoadedFace {
            bytes: Arc::from(INTER),
            metrics: build_face_metrics(INTER),
            unicode_ranges: ranges,
            ascent_override: None,
            descent_override: None,
            size_adjust: None,
            variation_settings: Vec::new(),
        }
    }

    /// Mirrors WPT `css/css-fonts/font-face-unicode-range.html`: two
    /// `@font-face` rules share ONE font file (`LigatureSymbolsWithSpaces.woff`
    /// there, `Inter-Regular.ttf` here) but declare disjoint `unicode-range`.
    /// The descriptor must win over the fact that the shared file's `cmap`
    /// actually covers both ranges — before FONTLOAD-9, `pick_face_for_codepoint`
    /// looked only at `cmap` (BUG-434) and would have used either face for
    /// either codepoint.
    #[test]
    fn declared_range_wins_over_accidental_cmap_coverage() {
        let uppercase_only = face_with_range(vec![(0x41, 0x5A)]); // A-Z
        let lowercase_only = face_with_range(vec![(0x61, 0x7A)]); // a-z
        let faces = [uppercase_only, lowercase_only];

        // primary = face 0 (A-Z only): 'a' is outside its declared range, so
        // it must fall through to face 1 even though Inter's cmap does have
        // a lowercase glyph in face 0's own table too.
        let (face_id, glyph_id) = pick_face_for_codepoint('a' as u32, 0, &faces);
        assert_eq!(face_id, 1, "'a' is outside face 0's declared range — must fall through");
        assert_ne!(glyph_id, 0, "face 1 must resolve a real glyph for 'a'");

        // Symmetric: primary = face 1 (a-z only), asking for 'A' must fall
        // through to face 0.
        let (face_id, glyph_id) = pick_face_for_codepoint('A' as u32, 1, &faces);
        assert_eq!(face_id, 0, "'A' is outside face 1's declared range — must fall through");
        assert_ne!(glyph_id, 0, "face 0 must resolve a real glyph for 'A'");
    }

    /// No `unicode-range` descriptor (empty `Vec`, the common case for both
    /// system fonts and most `@font-face` rules) must not restrict anything —
    /// this is the pre-FONTLOAD-9 behaviour, unchanged.
    #[test]
    fn unrestricted_face_matches_any_codepoint() {
        let unrestricted = face_with_range(Vec::new());
        let faces = [unrestricted];
        let (face_id, glyph_id) = pick_face_for_codepoint('A' as u32, 0, &faces);
        assert_eq!(face_id, 0);
        assert_ne!(glyph_id, 0);
    }

    /// A codepoint outside every loaded face's declared range still falls
    /// back to `(primary, 0)` — draws `.notdef` from the primary face,
    /// exactly like the pre-existing missing-glyph fallback.
    #[test]
    fn codepoint_outside_every_range_falls_back_to_primary_notdef() {
        let restricted = face_with_range(vec![(0x41, 0x5A)]); // A-Z only
        let faces = [restricted];
        let (face_id, glyph_id) = pick_face_for_codepoint('!' as u32, 0, &faces);
        assert_eq!(face_id, 0);
        assert_eq!(glyph_id, 0);
    }
}

#[cfg(test)]
mod merge_variation_settings_tests {
    use super::*;

    #[test]
    fn no_descriptor_defaults_returns_property_axes_unchanged() {
        let merged = merge_variation_settings(&[], &[(*b"wght", 700.0)]);
        assert_eq!(merged, vec![(*b"wght", 700.0)]);
    }

    #[test]
    fn no_property_axes_uses_descriptor_defaults() {
        let merged = merge_variation_settings(&[(*b"wght", 375.0)], &[]);
        assert_eq!(merged, vec![(*b"wght", 375.0)]);
    }

    #[test]
    fn property_overrides_matching_descriptor_axis() {
        let merged = merge_variation_settings(&[(*b"wght", 375.0)], &[(*b"wght", 700.0)]);
        assert_eq!(merged, vec![(*b"wght", 700.0)]);
    }

    #[test]
    fn descriptor_axis_not_named_by_property_still_applies() {
        let merged = merge_variation_settings(
            &[(*b"wght", 375.0), (*b"slnt", -8.0)],
            &[(*b"wght", 700.0)],
        );
        assert_eq!(merged, vec![(*b"wght", 700.0), (*b"slnt", -8.0)]);
    }

    #[test]
    fn property_axis_not_named_by_descriptor_is_appended() {
        let merged = merge_variation_settings(&[(*b"wght", 375.0)], &[(*b"slnt", -8.0)]);
        assert_eq!(merged, vec![(*b"wght", 375.0), (*b"slnt", -8.0)]);
    }

    #[test]
    fn both_empty_is_empty() {
        assert!(merge_variation_settings(&[], &[]).is_empty());
    }
}
