//! P1/SPLIT-RN2: подстатьи `svg-sub`/`text-sub` покадрового лога ([`SvgSubStats`]/
//! [`TextSubStats`]) и glyph-часть free-fn хвоста `renderer.rs` — укладка текстового
//! run-а с мемоизацией ([`TextRunCache`]) и растеризация глифа в атлас
//! (`push_glyph_quad`…`rasterize_and_insert`). Вынесено из `renderer.rs`
//! (`docs/tasks/p1-monolith-split-queue.md` §4, группа RN, батч RN-2).

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
            "text-sub: arm {:.2}ms run {:.2} loop {:.2} pre {:.2} pick {:.2} coord {:.2} \
             glyf {:.2} look {:.2} quad {:.2} sciss {:.2} xform {:.2} | \
             команд {} / символов {} / квадов {}",
            ms(&self.arm),
            ms(&self.run),
            ms(&self.lp),
            ms(&self.pre),
            ms(&self.pick),
            ms(&self.coord),
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

/// Кладёт два треугольника одного глифа: позиция от пера `cursor_x` и
/// baseline-а, UV — из atlas-записи. Вынесено, потому что монохромный путь и
/// каждый слой COLR-глифа кладут один и тот же quad, различаясь только
/// цветом и glyph-id.
fn push_glyph_quad(
    out: &mut Vec<TextVertex>,
    g: &CachedGlyph,
    cursor_x: f32,
    baseline_y: f32,
    display_scale: f32,
    color: [f32; 4],
) {
    let bm_left = g.left * display_scale;
    let bm_top = g.top * display_scale;
    let bm_w = g.entry.width as f32 * display_scale;
    let bm_h = g.entry.height as f32 * display_scale;
    let x0 = cursor_x + bm_left;
    let y0 = baseline_y - bm_top;
    let x1 = x0 + bm_w;
    let y1 = y0 + bm_h;
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
    },
    /// Сдвинуть перо, ничего не кладя (табуляция или неотрисовавшийся глиф).
    Advance(f32),
}

/// План укладки run-а: последовательность [`TextRunStep`] от пера в `rect.x`.
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
    primary_face_id: usize,
    axes: &[([u8; 4], f32)],
) {
    out.clear();
    out.push(font_size.to_bits());
    out.push(tab_size.to_bits());
    out.push(primary_face_id as u32);
    out.push(axes.len() as u32);
    for (tag, value) in axes {
        out.push(u32::from_be_bytes(*tag));
        out.push(value.to_bits());
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
    tab_size: f32,
    font_palette: Option<&FontPaletteSelection>,
) -> f32 {
    // BUG-405 срез 13: разбивка команды текста (`text-sub`, уровень 3).
    let log = crate::frame_log_level() >= 3;
    let _t_run = sub_timer(log, &TEXT_SUB.run);
    let t_pre = log.then(std::time::Instant::now);
    // Multi-size atlas: подбираем bin под font_size, растеризируем глифы
    // на этом bin. Display масштаб = font_size / size_bin — если font_size
    // совпал с bin-ом (12/16/24/32/...) — масштаба нет, текст резкий.
    let size_bin = size_bin_for(font_size);
    let display_scale = font_size / size_bin as f32;

    // Baseline: ascent / (ascent − descent) primary face-а. Для Inter ≈ 0.80.
    // Используем primary для всех глифов в run-е — иначе при смешивании
    // face-ов символы прыгали бы по вертикали.
    let primary = lazy.faces[primary_face_id]
        .metrics
        .as_ref()
        .expect("primary face metrics must exist (checked by caller)");
    let ascent_ratio = primary.ascent as f32
        / (primary.ascent as f32 - primary.descent as f32);
    let baseline_y = rect.y + font_size * ascent_ratio;

    // Per-char cache на длительность одного DrawText: одни и те же символы
    // в строке («the the the») не нужно пробовать через все face-ы каждый раз.
    let mut char_face_cache: HashMap<char, (usize, u16)> = HashMap::new();
    // Normalized variation coords per face_id — лениво вычисляется при первом
    // обращении к данному face. Нормализация требует fvar+avar из шрифта
    // (единственный потребитель `ParsedFace` на пути без промахов атласа;
    // при пустых axes face не парсится вовсе).
    let mut norm_coords_cache: HashMap<usize, Vec<f32>> = HashMap::new();
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
            primary_face_id,
            font_variation_axes,
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
                TextRunStep::Glyph { g, advance } => {
                    let t_quad = log.then(std::time::Instant::now);
                    push_glyph_quad(out, g, cursor_x, baseline_y, display_scale, color);
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
    for ch in text.chars() {
        // CSS Text L3 §10.1 — tab character advances by tab_size pixels.
        if ch == '\t' && tab_size > 0.0 {
            cursor_x += tab_size;
            if let Some(plan) = plan.as_mut() {
                plan.push(TextRunStep::Advance(tab_size));
            }
            continue;
        }
        let t_pick = log.then(std::time::Instant::now);
        let (face_id, glyph_id) = *char_face_cache
            .entry(ch)
            .or_insert_with(|| pick_face_for_codepoint(ch as u32, primary_face_id, lazy.faces));
        let metrics = lazy.faces[face_id]
            .metrics
            .as_ref()
            .expect("pick_face_for_codepoint вернул face_id с валидными metrics");
        let advance_scale = font_size / metrics.units_per_em as f32;
        if let Some(t0) = t_pick {
            sub_add(&TEXT_SUB.pick, t0);
        }
        let t_coord = log.then(std::time::Instant::now);
        let coords: &[f32] = match norm_coords_cache.entry(face_id) {
            std::collections::hash_map::Entry::Occupied(e) => e.into_mut(),
            std::collections::hash_map::Entry::Vacant(v) => {
                let computed = if font_variation_axes.is_empty() {
                    Vec::new()
                } else if let Some(face) = lazy.get(face_id) {
                    normalize_variation_axes(face, font_variation_axes)
                } else {
                    Vec::new()
                };
                v.insert(computed)
            }
        };
        if let Some(t0) = t_coord {
            sub_add(&TEXT_SUB.coord, t0);
        }
        let _t_glyf = sub_timer(log, &TEXT_SUB.glyf);
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
                let Some(g) =
                    ensure_glyph(cached, atlas, lazy, face_id, layer.glyph_id, size_bin, coords)
                else {
                    continue;
                };
                let t_quad = log.then(std::time::Instant::now);
                push_glyph_quad(
                    out,
                    &g,
                    cursor_x,
                    baseline_y,
                    display_scale,
                    layer_color(palette, layer.palette_index, color),
                );
                if let Some(t0) = t_quad {
                    sub_add(&TEXT_SUB.quad, t0);
                    TEXT_SUB.glyphs.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
            }
            if let Some(&adv) = metrics.advances.get(glyph_id as usize) {
                cursor_x += adv as f32 * advance_scale;
            }
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
            size_bin,
            coords,
        );

        if let Some(g) = cached_glyph {
            let t_quad = log.then(std::time::Instant::now);
            let advance = g.advance_native as f32 * advance_scale;
            push_glyph_quad(out, &g, cursor_x, baseline_y, display_scale, color);
            cursor_x += advance;
            if let Some(plan) = plan.as_mut() {
                plan.push(TextRunStep::Glyph { g, advance });
            }
            if let Some(t0) = t_quad {
                sub_add(&TEXT_SUB.quad, t0);
                TEXT_SUB.glyphs.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        } else {
            // Глиф не отрисовался (composite-fallback, empty или нет места
            // в атласе). Двигаем cursor на advance из выбранного face-а.
            if let Some(&adv) = metrics.advances.get(glyph_id as usize) {
                let advance = adv as f32 * advance_scale;
                cursor_x += advance;
                if let Some(plan) = plan.as_mut() {
                    plan.push(TextRunStep::Advance(advance));
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

/// Ph3 writing-mode vertical, Срез 2 — rotates a glyph run's vertices 90° CW
/// around the local origin and translates the result onto `dest`. Mirrors the
/// CPU rasterizer's `rasterize_text_rotated` transform
/// (`tiny_skia::Transform::from_row(0, 1, -1, 0, dest.x, dest.y)`): a point
/// laid out horizontally at `(x, y)` maps to `(-y + dest.x, x + dest.y)`.
/// Callers must have generated `verts` with `push_text_glyphs` at the local
/// origin `(0, 0)` — not at `dest`.
pub(crate) fn rotate_text_vertices_cw(verts: &mut [TextVertex], dest: Rect) {
    for v in verts {
        let (x, y) = (v.pos[0], v.pos[1]);
        v.pos = [-y + dest.x, x + dest.y];
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
    tab_size: f32,
    font_palette: Option<&FontPaletteSelection>,
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
                cached, runs, runs_enabled, font_variation_axes, tab_size, font_palette,
            );
            y_cursor += end_x - dest.x;
        } else {
            let v_start = out.len();
            let local_rect = Rect::new(y_cursor, 0.0, dest.width, dest.height);
            let end_x = push_text_glyphs(
                out, local_rect, &seg_text, font_size, color, primary_face_id, lazy, atlas,
                cached, runs, runs_enabled, font_variation_axes, tab_size, font_palette,
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
fn pick_face_for_codepoint(
    cp: u32,
    primary_face_id: usize,
    faces: &[LoadedFace],
) -> (usize, u16) {
    if let Some(m) = faces.get(primary_face_id).and_then(|f| f.metrics.as_ref())
        && let Some(gid) = m.cmap.glyph_index(cp).filter(|&g| g != 0)
    {
        return (primary_face_id, gid);
    }
    for (idx, face) in faces.iter().enumerate() {
        if idx == primary_face_id {
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

#[allow(clippy::too_many_arguments)]
pub(crate) fn ensure_glyph(
    cached: &mut HashMap<AtlasKey, Option<CachedGlyph>>,
    atlas: &mut GlyphAtlas,
    lazy: &mut LazyParsedFaces<'_>,
    face_id: usize,
    glyph_id: u16,
    size_bin: u16,
    coords: &[f32],
) -> Option<CachedGlyph> {
    // BUG-405 срез 13: поиск в кэше атласа — отдельной подстатьёй `text-sub`.
    // Цена промаха (растеризация) считается ниже своим счётчиком (срез 3),
    // поэтому таймер снимается ровно на границе попадания/промаха.
    let t_look = (crate::frame_log_level() >= 3).then(std::time::Instant::now);
    let key = atlas_key(face_id, glyph_id, size_bin, AtlasKey::hash_coords(coords));
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
        &face.hmtx,
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
    hmtx: &Hmtx,
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
    let raster = Rasterizer::new(f32::from(key.size_bin), units_per_em);
    let Some(bitmap): Option<Bitmap> = raster.rasterize(&glyph) else {
        return GlyphRaster::Empty;
    };
    let entry = match atlas.try_insert(key, &bitmap) {
        InsertOutcome::Inserted(entry) => entry,
        InsertOutcome::Rejected => return GlyphRaster::Empty,
        InsertOutcome::OutOfSpace => return GlyphRaster::OutOfSpace,
    };
    // HVAR delta applied: for variable fonts, advance width varies per axis instance.
    // Font::advance_width_varied falls back to hmtx base when HVAR is absent.
    let advance_native = font.advance_width_varied(key.glyph_id, hmtx, coords);
    GlyphRaster::Ready(CachedGlyph {
        entry,
        left: bitmap.left,
        top: bitmap.top,
        advance_native,
    })
}
