use super::*;

/// Adapter that feeds `core::fmt` output straight into a [`Hasher`] without
/// allocating an intermediate `String`.
/// Адаптер `fmt::Write` → `Hasher`: пишет Debug-представление напрямую в хешер,
/// без промежуточной `String` (нулевые аллокации в горячем пути кадра).
pub(crate) struct HashFmt<'a>(pub(crate) &'a mut std::collections::hash_map::DefaultHasher);

impl std::fmt::Write for HashFmt<'_> {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        use std::hash::Hasher;
        self.0.write(s.as_bytes());
        Ok(())
    }
}

/// Writes an `f32` into the hasher by its bit pattern.
///
/// Bit-hashing is *stricter* than the `Debug` text it replaces: `NaN` payloads
/// that print identically hash differently. That direction is safe — it can
/// only produce a spurious "changed" verdict (an extra repaint), never a
/// spurious "identical" one (stale pixels on screen).
#[inline]
pub(crate) fn h_f32(h: &mut std::collections::hash_map::DefaultHasher, v: f32) {
    use std::hash::Hasher;
    h.write_u32(v.to_bits());
}

/// Writes a [`Rect`] into the hasher field-by-field.
#[inline]
pub(crate) fn h_rect(h: &mut std::collections::hash_map::DefaultHasher, r: &Rect) {
    h_f32(h, r.x);
    h_f32(h, r.y);
    h_f32(h, r.width);
    h_f32(h, r.height);
}

/// Writes an RGBA8 [`Color`] into the hasher.
#[inline]
pub(crate) fn h_color(h: &mut std::collections::hash_map::DefaultHasher, c: &Color) {
    use std::hash::Hasher;
    h.write_u8(c.r);
    h.write_u8(c.g);
    h.write_u8(c.b);
    h.write_u8(c.a);
}

/// Writes all eight [`CornerRadii`] components into the hasher.
#[inline]
fn h_radii(h: &mut std::collections::hash_map::DefaultHasher, r: &CornerRadii) {
    for v in [r.tl, r.tl_y, r.tr, r.tr_y, r.br, r.br_y, r.bl, r.bl_y] {
        h_f32(h, v);
    }
}

/// Writes a string into the hasher with a terminator, so that `"ab" + "c"`
/// cannot collide with `"a" + "bc"`.
#[inline]
pub(crate) fn h_str(h: &mut std::collections::hash_map::DefaultHasher, s: &str) {
    use std::hash::Hasher;
    h.write(s.as_bytes());
    h.write_u8(0xff);
}

/// Folds one [`DisplayCommand`] into `h` **structurally** — raw field bytes,
/// no `core::fmt` machinery.
///
/// Why: the frame-skip hash used to fold every command through `{cmd:?}`.
/// `Debug` for `f32` runs the Grisu/Dragon shortest-repr algorithm per float,
/// and a typical frame carries thousands of them — measured at 1.2–2.5 ms per
/// frame on `1000000-final.html` (see EXPERIMENT.md §9 "открытые хвосты").
///
/// **Safety of the fast path.** The hot variants below destructure *every*
/// field explicitly — no `..` rest-pattern — so adding a field to one of them
/// is a compile error, not a silent stale-pixel bug. Every other variant falls
/// through to the original `Debug` fold, which is exhaustive by construction:
/// a newly added variant is hashed correctly (just slower) from day one. The
/// variant tag itself is always folded via `mem::discriminant`, so two variants
/// with structurally identical payloads can never collide.
pub(crate) fn hash_command_into(
    cmd: &DisplayCommand,
    h: &mut std::collections::hash_map::DefaultHasher,
) {
    use std::fmt::Write as _;
    use std::hash::{Hash as _, Hasher as _};

    std::mem::discriminant(cmd).hash(h);

    match cmd {
        DisplayCommand::FillRect { rect, color } => {
            h_rect(h, rect);
            h_color(h, color);
        }
        DisplayCommand::FillRoundedRect { rect, color, radii } => {
            h_rect(h, rect);
            h_color(h, color);
            h_radii(h, radii);
        }
        DisplayCommand::DrawBorder { rect, widths, colors, styles, radii } => {
            h_rect(h, rect);
            for w in widths {
                h_f32(h, *w);
            }
            for c in colors {
                h_color(h, c);
            }
            for s in styles {
                std::mem::discriminant(s).hash(h);
            }
            h_radii(h, radii);
        }
        DisplayCommand::DrawOutline { rect, width, style, color, offset } => {
            h_rect(h, rect);
            h_f32(h, *width);
            std::mem::discriminant(style).hash(h);
            h_color(h, color);
            h_f32(h, *offset);
        }
        DisplayCommand::PushClipRect { rect } => h_rect(h, rect),
        DisplayCommand::PushOpacity { alpha, bounds } => {
            h_f32(h, *alpha);
            if let Some(r) = bounds {
                h_rect(h, r);
            }
        }
        DisplayCommand::PushTransform { matrix } => {
            for v in matrix.0 {
                h_f32(h, v);
            }
        }
        DisplayCommand::DrawText {
            rect,
            text,
            font_size,
            color,
            font_family,
            font_weight,
            font_style,
            font_stretch,
            font_variation_axes,
            font_features,
            font_palette,
            tab_size,
            highlight_name,
            text_orientation,
        } => {
            h_rect(h, rect);
            h_str(h, text);
            h_f32(h, *font_size);
            h_color(h, color);
            h.write_usize(font_family.len());
            for f in font_family {
                h_str(h, f);
            }
            h.write_u16(font_weight.0);
            std::mem::discriminant(font_style).hash(h);
            // Влияет на выбор face-а — стало быть, и на пиксели: без него
            // кадр, где сменился только font-stretch, переиспользует
            // закэшированный тайл со старым (нормальной ширины) face-ом.
            h.write_u16(font_stretch.0);
            h.write_usize(font_variation_axes.len());
            for (tag, v) in font_variation_axes {
                h.write(tag);
                h_f32(h, *v);
            }
            h.write_usize(font_features.len());
            for (tag, v) in font_features {
                h.write(tag);
                h.write_u32(*v);
            }
            // Structurally complex and almost always `None` — `Debug` here costs
            // four bytes and no float formatting.
            {
                let mut hf = HashFmt(h);
                let _ = write!(hf, "{font_palette:?}");
            }
            h_f32(h, *tab_size);
            match highlight_name {
                Some(s) => {
                    h.write_u8(1);
                    h_str(h, s);
                }
                None => h.write_u8(0),
            }
            match text_orientation {
                Some(t) => {
                    h.write_u8(1);
                    std::mem::discriminant(t).hash(h);
                }
                None => h.write_u8(0),
            }
        }
        // Cold variants: gradients, SVG, masks, filters, snapshots, scrollbars,
        // and every unit variant. Folded through `Debug` exactly as before.
        other => {
            let mut hf = HashFmt(h);
            // Errors are impossible: HashFmt::write_str never fails.
            let _ = write!(hf, "{other:?}");
        }
    }
}

/// Хеширует одну команду структурно, без аллокаций.
///
/// [`hash_display_list_dual`] сворачивает кадр через этот дайджест: свёртка
/// команды, попадающая в оба кадровых хэша, считается один раз. Границы команд
/// при этом становятся явными (в непрерывном потоке они были неявными) — это
/// строже, а не слабее: «размазать» поля соседних команд друг в друга нельзя.
pub(crate) fn hash_one_command(cmd: &DisplayCommand) -> u64 {
    use std::hash::Hasher;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    hash_command_into(cmd, &mut hasher);
    hasher.finish()
}

/// Computes a content hash over a frame's display list plus the viewport state
/// that affects backdrop-filter output (scroll offset and surface size).
///
/// Used by the renderer's `backdrop-filter` cache (CSS Filter Effects L1 §2):
/// if two consecutive frames hash identically, every backdrop element's
/// filtered result is guaranteed identical, so the blur passes can be skipped
/// and the cached texture reused.
///
/// The hash is **total** — it folds every field of every command (see
/// [`hash_command_into`]: explicit fields for the hot variants, `Debug` for the
/// cold ones) — so adding new `DisplayCommand` variants or fields can never
/// silently produce a false cache hit (which would paint stale pixels).
///
/// The hasher (`DefaultHasher`) is process-deterministic and never influences
/// pixel output (only the skip decision), so cross-OS bit-identity is not a
/// concern here.
#[must_use]
pub fn hash_display_list(
    content: &[DisplayCommand],
    overlay: &[DisplayCommand],
    scroll_x: f32,
    scroll_y: f32,
    surface_w: u32,
    surface_h: u32,
) -> u64 {
    use std::hash::Hasher;

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    hasher.write_u32(surface_w);
    hasher.write_u32(surface_h);
    hasher.write_u32(scroll_x.to_bits());
    hasher.write_u32(scroll_y.to_bits());
    // Lane lengths: keeps the "content then overlay" fold unambiguous when a
    // command migrates between lanes.
    hasher.write_usize(content.len());
    hasher.write_usize(overlay.len());
    for cmd in content.iter().chain(overlay.iter()) {
        hash_command_into(cmd, &mut hasher);
    }
    hasher.finish()
}

/// Content-only frame hash (ADR-016 M0.5).
///
/// Unlike [`hash_display_list`], this folds **only** the page-content commands
/// and the surface size into the hash — the scroll offset and the fixed page
/// offset are deliberately excluded. Two frames that differ only in how far the
/// page is scrolled therefore hash identically, which is exactly what lets the
/// compositor tell "same content, new offset" (a blit — M3's fast path) apart
/// from "content changed" (a full re-raster).
///
/// Overlay commands (scrollbar thumb, docked panels, find-bar) are intentionally
/// **not** passed here: they are viewport-locked and cheap to repaint every
/// frame, and the scrollbar thumb in particular is rebuilt from `scroll_y` each
/// frame, so folding it in would make every scroll frame look like a content
/// change and defeat the content/offset split.
///
/// Allocation-free: each command's `Debug` output is streamed straight into the
/// hasher via [`HashFmt`], preserving `hash_display_list`'s totality guarantee
/// (a new `DisplayCommand` variant or field can never silently collide).
#[must_use]
pub fn hash_content(content: &[DisplayCommand], surface_w: u32, surface_h: u32) -> u64 {
    use std::fmt::Write as _;
    use std::hash::Hasher;

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    hasher.write_u32(surface_w);
    hasher.write_u32(surface_h);
    hasher.write_usize(content.len());
    {
        let mut hf = HashFmt(&mut hasher);
        for cmd in content {
            // Errors are impossible: HashFmt::write_str never fails.
            let _ = write!(hf, "{cmd:?}");
        }
    }
    hasher.finish()
}

/// Как [`hash_display_list`], но с выколотыми диапазонами `skip` (static-часть
/// кадра для scroll-инвариантного ключа полосы скролл-композитора).
///
/// Эквивалентен `hash_display_list` от материализованного списка без
/// skip-команд: та же свёртка, длиной content-полосы служит число
/// оставшихся команд. `skip` обязан быть отсортирован и не пересекаться
/// (гарантируется [`build_display_list_ordered_with_anim_split`]).
pub fn hash_display_list_skipping(
    content: &[DisplayCommand],
    skip: &[std::ops::Range<usize>],
    overlay: &[DisplayCommand],
    scroll_x: f32,
    scroll_y: f32,
    surface_w: u32,
    surface_h: u32,
) -> u64 {
    use std::hash::Hasher;

    let skipped: usize = skip.iter().map(std::ops::Range::len).sum();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    hasher.write_u32(surface_w);
    hasher.write_u32(surface_h);
    hasher.write_u32(scroll_x.to_bits());
    hasher.write_u32(scroll_y.to_bits());
    hasher.write_usize(content.len().saturating_sub(skipped));
    hasher.write_usize(overlay.len());
    let mut skip_iter = skip.iter().peekable();
    for (i, cmd) in content.iter().enumerate() {
        while skip_iter.peek().is_some_and(|r| r.end <= i) {
            skip_iter.next();
        }
        if skip_iter.peek().is_some_and(|r| r.contains(&i)) {
            continue;
        }
        hash_command_into(cmd, &mut hasher);
    }
    for cmd in overlay {
        hash_command_into(cmd, &mut hasher);
    }
    hasher.finish()
}

/// Оба кадровых хэша за ОДИН обход списка (BUG-405 срез 35, пункт 70).
///
/// Возвращает `(хэш кадра, ключ полосы)` — те же две свёртки, что кадр раньше
/// считал двумя раздельными обходами ([`hash_display_list`] по
/// `content` + `overlay` со скроллом и размерами поверхности,
/// [`hash_display_list_skipping`] по статичной части `content` при нулевом
/// скролле и размерах полосы). Значения ДРУГИЕ, чем у пары: оба хэша
/// сравниваются только с хэшем предыдущего кадра того же процесса, поэтому
/// важны их свойства (см. гейты `dual_*` в тестах), а не конкретные числа.
///
/// **Почему один проход дешевле.** Скролл и размеры входят в оба хэша
/// отдельными полями, а не через обход, а сами команды у хэшей общие — при
/// пустом `skip` полностью, иначе с точностью до выколотых диапазонов. Список
/// разбирается один раз, и общая часть сворачивается ОДНИМ потоком SipHash:
/// команда даёт 64-битный дайджест, который уходит в оба хешера. Тройник
/// ([`TeeHasher`]) остаётся только для кадров с непустым `skip`, где у плеч
/// разные множества команд, — там экономится разбор, но не байты.
///
/// `skip` обязан быть отсортирован и не пересекаться — то же требование, что
/// у [`hash_display_list_skipping`]; overlay в ключ не входит вовсе
/// (viewport-locked, см. [`hash_content`]).
#[must_use]
pub fn hash_display_list_dual(
    content: &[DisplayCommand],
    overlay: &[DisplayCommand],
    skip: &[std::ops::Range<usize>],
    scroll: (f32, f32),
    surface: (u32, u32),
    band: (u32, u32),
) -> (u64, u64) {
    hash_display_list_dual_memo(content, overlay, skip, scroll, surface, band, None).0
}

/// Свёртки content-части кадра для обоих кадровых хэшей (BUG-405 срез 39).
///
/// `.0` — поток дайджестов ВСЕХ команд списка (вход хэша кадра), `.1` — тот же
/// поток без выколотых `skip`-диапазонов (вход ключа полосы). Обход и дайджест
/// команды — ровно те же, что считал [`hash_display_list_dual`] до среза 39;
/// новое здесь только то, что результат обхода стал ЗНАЧЕНИЕМ, которое кадр
/// может запомнить и переиспользовать, пока список не менялся.
///
/// `skip` обязан быть отсортирован и не пересекаться.
#[must_use]
pub fn fold_content_dual(content: &[DisplayCommand], skip: &[std::ops::Range<usize>]) -> (u64, u64) {
    use std::hash::Hasher;

    let mut frame = std::collections::hash_map::DefaultHasher::new();
    let mut key = std::collections::hash_map::DefaultHasher::new();
    if skip.is_empty() {
        // Горячий случай (страница без анимируемых сегментов): у плеч одно и
        // то же множество команд, поэтому дайджест команды считается один раз
        // и пишется в оба хешера — байты команды сворачиваются однократно.
        for cmd in content {
            let d = hash_one_command(cmd);
            frame.write_u64(d);
            key.write_u64(d);
        }
    } else {
        let mut skip_iter = skip.iter().peekable();
        for (i, cmd) in content.iter().enumerate() {
            while skip_iter.peek().is_some_and(|r| r.end <= i) {
                skip_iter.next();
            }
            let d = hash_one_command(cmd);
            frame.write_u64(d);
            if !skip_iter.peek().is_some_and(|r| r.contains(&i)) {
                key.write_u64(d);
            }
        }
    }
    (frame.finish(), key.finish())
}

/// Дайджест overlay-списка, один [`hash_one_command`] на элемент (BUG-405
/// срез 47). Раньше он считался НЕЗАВИСИМО в двух местах одного и того же
/// кадра: здесь (внутри [`hash_display_list_dual_memo`], статья `frame-hash`)
/// и в `Renderer::overlay_cache_step` (статья `послекэша` — срез 44 измерил
/// её ~0.12 мс на кадр попадания, срез 43 назвал сам факт безусловного
/// пересчёта). Оба потребителя сравнивают дайджест с одним и тем же
/// определением (`hash_one_command`), так что вызывающий (`render_with_anim`)
/// теперь считает его ОДИН раз и передаёт результат в оба места —
/// [`hash_display_list_dual_memo_with_overlay_digests`] и
/// `overlay_cache_step`.
#[must_use]
pub fn fold_overlay(overlay: &[DisplayCommand]) -> Vec<u64> {
    overlay.iter().map(hash_one_command).collect()
}

/// Как [`fold_overlay`], но переиспользует готовые дайджесты хвоста
/// `overlay[start..]` вместо пересчёта `hash_one_command` по нему (BUG-405
/// срез 57, п.85). `reuse` — `(start, digests)`, где `digests[i]` обязан
/// быть дайджестом именно `overlay[start + i]`; это гарантирует вызывающий
/// (shell), а не эта функция — здесь только проверка длины
/// (`start + digests.len() == overlay.len()`), достаточная, чтобы не
/// вылезти за границы, но НЕ доказывающая, что дайджесты относятся к тем
/// же командам. Несовпадение длины (сокращённый overlay, промах кэша,
/// смена состава билдеров) — тихий откат на полный [`fold_overlay`], не
/// панику: единственный источник этого аргумента (`ChromeOverlayFrameCache`
/// через [`RenderBackend::set_overlay_digest_reuse`](crate::backend::RenderBackend::set_overlay_digest_reuse))
/// уже перепроверяет применимость на своей стороне перед вызовом.
#[must_use]
pub fn fold_overlay_with_reuse(overlay: &[DisplayCommand], reuse: Option<&(usize, Vec<u64>)>) -> Vec<u64> {
    if let Some((start, tail_digests)) = reuse
        && *start <= overlay.len()
        && overlay.len() - start == tail_digests.len()
    {
        let mut out: Vec<u64> = overlay[..*start].iter().map(hash_one_command).collect();
        out.extend_from_slice(tail_digests);
        return out;
    }
    fold_overlay(overlay)
}

/// [`hash_display_list_dual`] с ГОТОВОЙ свёрткой content-части (BUG-405 срез 39).
///
/// `folds` — результат [`fold_content_dual`] для этого же `content`/`skip`,
/// снятый на прошлом кадре; `None` — посчитать заново. Возвращает пару хэшей и
/// свёртку, которой они посчитаны (её и запоминает кадр).
///
/// Зачем: на кадре ПОПАДАНИЯ полосы content не менялся вовсе — страница
/// свёрстана, едет только скролл, — а оба хэша обходили его целиком. Перепись
/// среза 39: 0.76 мс на кадр при 843 + 132 командах, 37 % честного кадра
/// попадания. Скролл, размеры поверхности и полосы, длины и overlay в свёртку
/// не входят и дописываются здесь каждый кадр, поэтому переиспользование
/// свёртки НЕ делает кадр слепым ни к одному из них.
///
/// Ответственность за «список не менялся» лежит на вызывающем
/// ([`RenderBackend::set_content_epoch`](crate::backend::RenderBackend::set_content_epoch)).
///
/// Пересчитывает overlay-дайджест внутри себя ([`fold_overlay`]) — тесты и
/// прочие вызывающие, которым нечего переиспользовать, используют эту форму
/// как раньше. Горячий путь `render_with_anim` вызывает
/// [`hash_display_list_dual_memo_with_overlay_digests`] напрямую с уже
/// посчитанным дайджестом (срез 47).
#[must_use]
pub fn hash_display_list_dual_memo(
    content: &[DisplayCommand],
    overlay: &[DisplayCommand],
    skip: &[std::ops::Range<usize>],
    scroll: (f32, f32),
    surface: (u32, u32),
    band: (u32, u32),
    folds: Option<(u64, u64)>,
) -> ((u64, u64), (u64, u64)) {
    hash_display_list_dual_memo_with_overlay_digests(
        content,
        &fold_overlay(overlay),
        skip,
        scroll,
        surface,
        band,
        folds,
    )
}

/// [`hash_display_list_dual_memo`] с ГОТОВЫМ overlay-дайджестом
/// ([`fold_overlay`]) вместо самого overlay-списка (BUG-405 срез 47) — та же
/// формула хэша, `overlay_digests.len()` заменяет `overlay.len()` (равны по
/// построению: один дайджест на команду).
#[must_use]
pub fn hash_display_list_dual_memo_with_overlay_digests(
    content: &[DisplayCommand],
    overlay_digests: &[u64],
    skip: &[std::ops::Range<usize>],
    scroll: (f32, f32),
    surface: (u32, u32),
    band: (u32, u32),
    folds: Option<(u64, u64)>,
) -> ((u64, u64), (u64, u64)) {
    use std::hash::Hasher;

    let (scroll_x, scroll_y) = scroll;
    let (surface_w, surface_h) = surface;
    let (key_w, key_h) = band;
    let folds = folds.unwrap_or_else(|| fold_content_dual(content, skip));

    let mut frame = std::collections::hash_map::DefaultHasher::new();
    frame.write_u32(surface_w);
    frame.write_u32(surface_h);
    frame.write_u32(scroll_x.to_bits());
    frame.write_u32(scroll_y.to_bits());
    frame.write_usize(content.len());
    frame.write_usize(overlay_digests.len());
    frame.write_u64(folds.0);

    let skipped: usize = skip.iter().map(std::ops::Range::len).sum();
    let mut key = std::collections::hash_map::DefaultHasher::new();
    key.write_u32(key_w);
    key.write_u32(key_h);
    key.write_usize(content.len().saturating_sub(skipped));
    key.write_u64(folds.1);

    for &d in overlay_digests {
        frame.write_u64(d);
    }
    ((frame.finish(), key.finish()), folds)
}

/// How a frame differs from the previously presented one (ADR-016 M0.5).
///
/// Produced by [`FrameFingerprint::delta_from`]. The variants map directly onto
/// the render strategies the staged multithreaded pipeline will pick between:
/// `Identical` → skip, `OffsetOnly` → blit (M3), `ContentChanged` → re-raster.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameDelta {
    /// Page content, scroll and page offset are all unchanged — the previously
    /// presented framebuffer is still correct and the frame can be skipped.
    Identical,
    /// Page content is unchanged but the scroll and/or fixed page offset moved —
    /// the M3 blit fast path can shift the retained content instead of
    /// re-rasterizing it.
    OffsetOnly,
    /// Page content changed (or the surface was resized) — a full re-raster is
    /// required.
    ContentChanged,
}

/// Split fingerprint of a presented frame (ADR-016 M0.5).
///
/// Separates the content hash (page commands + surface size, scroll excluded)
/// from the raw scroll and page offsets. Keeping the offsets out of the hash —
/// as plain copyable values rather than folded into it — is what lets
/// [`FrameFingerprint::delta_from`] return [`FrameDelta::OffsetOnly`] for a
/// scroll-only frame; those same offsets are also the input the M3 blit needs to
/// know how far to shift the retained content.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameFingerprint {
    /// Hash of the page-content commands and surface size — scroll excluded
    /// (see [`hash_content`]).
    pub content_hash: u64,
    /// Scroll offset `(x, y)`, in the same units the render backend receives.
    pub scroll: (f32, f32),
    /// Fixed page offset `(x, y)` — the left-docked sidebar width and tab-bar
    /// height applied render-side since M0.4.
    pub offset: (f32, f32),
}

impl FrameFingerprint {
    /// Build a fingerprint for the current frame from its page content, surface
    /// size and the two offsets.
    #[must_use]
    pub fn new(
        content: &[DisplayCommand],
        surface_w: u32,
        surface_h: u32,
        scroll: (f32, f32),
        offset: (f32, f32),
    ) -> Self {
        Self {
            content_hash: hash_content(content, surface_w, surface_h),
            scroll,
            offset,
        }
    }

    /// Classify how this frame differs from the previously presented `prev`.
    ///
    /// A differing `content_hash` always wins (`ContentChanged`) — a resize or
    /// any command edit forces a re-raster. Only when the content hash matches do
    /// the offsets decide between `OffsetOnly` (something moved) and `Identical`
    /// (nothing moved).
    #[must_use]
    pub fn delta_from(&self, prev: &FrameFingerprint) -> FrameDelta {
        if self.content_hash != prev.content_hash {
            FrameDelta::ContentChanged
        } else if self.scroll != prev.scroll || self.offset != prev.offset {
            FrameDelta::OffsetOnly
        } else {
            FrameDelta::Identical
        }
    }
}

// ─── Static/animated split: план оверлея + painter's-order guard ────────────

/// Консервативный bbox draw-команды в её локальных координатах (до
/// transform-стека). `None` — команда ничего не рисует (push/pop/PageBreak).
/// `SegBounds::Unbounded` — экстент вычислить нельзя (рисует «где-то»).
fn draw_cmd_local_bbox(cmd: &DisplayCommand) -> Option<SegBounds> {
    fn pts_bbox(iter: impl Iterator<Item = [f32; 2]>) -> SegBounds {
        let mut min_x = f32::INFINITY;
        let mut min_y = f32::INFINITY;
        let mut max_x = f32::NEG_INFINITY;
        let mut max_y = f32::NEG_INFINITY;
        let mut any = false;
        for [x, y] in iter {
            any = true;
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
        if any {
            SegBounds::Rect(Rect::new(min_x, min_y, max_x - min_x, max_y - min_y))
        } else {
            SegBounds::Empty
        }
    }
    Some(match cmd {
        DisplayCommand::FillRect { rect, .. }
        | DisplayCommand::FillRoundedRect { rect, .. }
        | DisplayCommand::DrawBorder { rect, .. }
        | DisplayCommand::DrawImage { rect, .. }
        | DisplayCommand::LazyImageSlot { rect, .. }
        | DisplayCommand::DrawBackgroundImage { rect, .. }
        | DisplayCommand::DrawLinearGradient { rect, .. }
        | DisplayCommand::DrawRadialGradient { rect, .. }
        | DisplayCommand::DrawConicGradient { rect, .. }
        | DisplayCommand::DrawLayerSnapshot { rect, .. } => SegBounds::Rect(*rect),
        // Глифы могут выступать за line-box (курсив, свисания) — запас в
        // половину кегля со всех сторон, строго в большую сторону.
        DisplayCommand::DrawText { rect, font_size, .. } => {
            SegBounds::Rect(inflate_rect(*rect, font_size * 0.5))
        }
        DisplayCommand::DrawOutline { rect, width, offset, .. } => {
            SegBounds::Rect(inflate_rect(*rect, width + offset.max(0.0)))
        }
        DisplayCommand::DrawCrossFade { dest, .. } => SegBounds::Rect(*dest),
        DisplayCommand::BoxModelOverlay { margin, .. } => SegBounds::Rect(*margin),
        DisplayCommand::DrawScrollbar { track_rect, thumb_rect, .. } => {
            SegBounds::Rect(union_rects(*track_rect, *thumb_rect))
        }
        DisplayCommand::DrawSvgPath { vertices, .. } => pts_bbox(vertices.iter().copied()),
        DisplayCommand::DrawSvgFill { contours, .. } => {
            pts_bbox(contours.iter().flatten().copied())
        }
        DisplayCommand::DrawSvgStroke { contours, params, .. } => {
            // Miter-стык может выступать до half_width·miterlimit от осевой.
            let d = params.half_width * params.miterlimit.max(1.0) + 1.0;
            match pts_bbox(contours.iter().flatten().copied()) {
                SegBounds::Rect(r) => SegBounds::Rect(inflate_rect(r, d)),
                other => other,
            }
        }
        DisplayCommand::PageBreak => SegBounds::Empty,
        _ => return None,
    })
}

/// Экстент множества draw-команд: пустой, прямоугольник или «неизвестно».
enum SegBounds {
    /// Ничего не нарисовано.
    Empty,
    /// Всё нарисованное лежит внутри прямоугольника (документные CSS px).
    Rect(Rect),
    /// Экстент вычислить нельзя.
    Unbounded,
}

impl SegBounds {
    fn union(&mut self, other: SegBounds) {
        match (&*self, other) {
            (_, SegBounds::Empty) => {}
            (SegBounds::Unbounded, _) => {}
            (_, SegBounds::Unbounded) => *self = SegBounds::Unbounded,
            (SegBounds::Empty, r @ SegBounds::Rect(_)) => *self = r,
            (SegBounds::Rect(a), SegBounds::Rect(b)) => {
                *self = SegBounds::Rect(union_rects(*a, b));
            }
        }
    }
}

fn inflate_rect(r: Rect, d: f32) -> Rect {
    Rect::new(r.x - d, r.y - d, r.width + 2.0 * d, r.height + 2.0 * d)
}

fn rects_overlap(a: &Rect, b: &Rect) -> bool {
    a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height
}

/// Аффинный bbox прямоугольника после матрицы (4 угла → min/max).
fn affine_rect_bbox(m: &Mat4, r: Rect) -> Rect {
    let a = m.0[0];
    let b = m.0[1];
    let c = m.0[4];
    let d = m.0[5];
    let e = m.0[12];
    let f = m.0[13];
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for (px, py) in [
        (r.x, r.y),
        (r.x + r.width, r.y),
        (r.x, r.y + r.height),
        (r.x + r.width, r.y + r.height),
    ] {
        let tx = a * px + c * py + e;
        let ty = b * px + d * py + f;
        min_x = min_x.min(tx);
        min_y = min_y.min(ty);
        max_x = max_x.max(tx);
        max_y = max_y.max(ty);
    }
    Rect::new(min_x, min_y, max_x - min_x, max_y - min_y)
}

/// Суммарная инфляция bbox от blur-функций фильтра — ровно охват ядра
/// нашего блюр-шейдера: `min(ceil(3σ), 32) + 2` текселя (та же формула,
/// что у bbox-scissor фильтр-пассов, EXPERIMENT.md п.16). Займётся
/// downscale-цепочка для σ > 4 — формулу менять синхронно с шейдером.
fn filter_bbox_inflate(filters: &[FilterFn]) -> f32 {
    filters
        .iter()
        .map(|f| match f {
            FilterFn::Blur(r) => (3.0 * r).ceil().min(32.0) + 2.0,
            _ => 0.0,
        })
        .sum()
}

/// Static/animated split (EXPERIMENT.md §2): строит план отрисовки
/// анимируемых сегментов поверх статичной полосы и проверяет, что перенос
/// сегментов в конец painter's order не меняет картинку.
///
/// Возвращает `Some(plan)` — команды сегментов, обёрнутые в реплей их
/// внешнего контекста (transform/clip/scroll-layer), в исходном порядке.
/// `None` — split в этом кадре небезопасен, рисовать монолитом:
/// - контекст сегмента содержит нереплеябельные группы (opacity/filter/
///   blend/mask поверх сегмента);
/// - сегмент несбалансирован по push/pop (не должно случаться по построению);
/// - статичная команда, рисуемая ПОЗЖЕ сегмента, пересекает его bbox —
///   сегмент, нарисованный поверх полосы, перекрыл бы её;
/// - в списке есть `BeginStickyLayer` (нелинейная зависимость от скролла).
pub fn anim_split_compose_plan(
    content: &[DisplayCommand],
    ranges: &[std::ops::Range<usize>],
) -> Option<(DisplayList, Vec<std::ops::Range<usize>>)> {
    // Диагностика причин отказа (LUMEN_FRAME_LOG=2) — один eprintln на кадр.
    macro_rules! bail {
        ($($why:tt)*) => {{
            if crate::frame_log_level() >= 2 {
                eprintln!("[frame:wgpu] anim-split bail: {}", format_args!($($why)*));
            }
            return None;
        }};
    }
    // Валидация диапазонов: отсортированы, не пересекаются, в пределах списка.
    let mut prev_end = 0usize;
    for r in ranges {
        if r.start < prev_end || r.end <= r.start || r.end > content.len() {
            bail!("malformed range {}..{}", r.start, r.end);
        }
        prev_end = r.end;
    }

    /// Push-команда пригодна для реплея вокруг сегмента: чистая
    /// геометрия/клип, без offscreen-групповой семантики.
    fn ctx_replayable(cmd: &DisplayCommand) -> bool {
        matches!(
            cmd,
            DisplayCommand::PushTransform { .. }
                | DisplayCommand::PushClipRect { .. }
                | DisplayCommand::PushClipRoundedRect { .. }
                | DisplayCommand::PushClipPath { .. }
                | DisplayCommand::PushScrollLayer { .. }
        )
    }

    fn pop_for_ctx(cmd: &DisplayCommand) -> DisplayCommand {
        match cmd {
            DisplayCommand::PushTransform { .. } => DisplayCommand::PopTransform,
            DisplayCommand::PushScrollLayer { .. } => DisplayCommand::PopScrollLayer,
            _ => DisplayCommand::PopClip,
        }
    }

    let mut ctx_stack: Vec<usize> = Vec::new(); // индексы активных Push-команд
    let mut mat_stack: Vec<Option<Mat4>> = Vec::new(); // накопленный 2D-аффинный transform (None = не-2D)
    let mut infl_stack: Vec<f32> = Vec::new(); // накопленная blur-инфляция активных фильтров
    let mut seg_bounds: Vec<SegBounds> = Vec::with_capacity(ranges.len());
    let mut seg_ctx: Vec<Vec<usize>> = Vec::with_capacity(ranges.len());
    let mut cur_range: Option<(usize, usize)> = None; // (индекс диапазона, глубина ctx на входе)
    let mut next_range = 0usize;
    let mut cur_bounds = SegBounds::Empty;
    // Первая статичная команда, конфликтующая с bbox сегмента → tail-split.
    let mut violation: Option<usize> = None;

    for (i, cmd) in content.iter().enumerate() {
        if cur_range.is_none() && next_range < ranges.len() && i == ranges[next_range].start {
            if let Some(&ci) = ctx_stack.iter().find(|&&ci| !ctx_replayable(&content[ci])) {
                bail!("ctx not replayable at {}: {}", i, content[ci].variant_name());
            }
            seg_ctx.push(ctx_stack.clone());
            cur_range = Some((next_range, ctx_stack.len()));
            cur_bounds = SegBounds::Empty;
        }

        let cur_mat = mat_stack.last().copied().flatten();
        let cur_infl = infl_stack.last().copied().unwrap_or(0.0);
        let identity_below = mat_stack.is_empty();

        match cmd {
            DisplayCommand::BeginStickyLayer { .. } => bail!("sticky layer at {}", i),
            DisplayCommand::PushTransform { matrix } => {
                let m = if matrix.is_2d_affine() {
                    if identity_below {
                        Some(*matrix)
                    } else {
                        cur_mat.map(|prev| prev.multiply(matrix))
                    }
                } else {
                    None
                };
                ctx_stack.push(i);
                mat_stack.push(m);
                infl_stack.push(cur_infl);
            }
            DisplayCommand::PushScrollLayer { scroll_x, scroll_y, .. } => {
                let t = Mat4::translation_2d(-*scroll_x, -*scroll_y);
                let m = if identity_below { Some(t) } else { cur_mat.map(|prev| prev.multiply(&t)) };
                ctx_stack.push(i);
                mat_stack.push(m);
                infl_stack.push(cur_infl);
            }
            DisplayCommand::PushFilter { filters, .. } => {
                ctx_stack.push(i);
                mat_stack.push(if identity_below { Some(Mat4::IDENTITY) } else { cur_mat });
                infl_stack.push(cur_infl + filter_bbox_inflate(filters));
            }
            DisplayCommand::PushBackdropFilter { filters, bounds } => {
                // Composite backdrop-фильтра пишет в `bounds` — учитываем его
                // как «рисующую» область (плюс blur-инфляция).
                let region = inflate_rect(*bounds, filter_bbox_inflate(filters));
                let eff = if identity_below {
                    SegBounds::Rect(region)
                } else {
                    match cur_mat {
                        Some(m) => SegBounds::Rect(affine_rect_bbox(&m, region)),
                        None => SegBounds::Unbounded,
                    }
                };
                if cur_range.is_some() {
                    cur_bounds.union(eff);
                } else if seg_hit(&seg_bounds, &eff) {
                    violation = Some(i);
                }
                ctx_stack.push(i);
                mat_stack.push(if identity_below { Some(Mat4::IDENTITY) } else { cur_mat });
                infl_stack.push(cur_infl + filter_bbox_inflate(filters));
            }
            DisplayCommand::PushClipRect { .. }
            | DisplayCommand::PushClipRoundedRect { .. }
            | DisplayCommand::PushClipPath { .. }
            | DisplayCommand::PushOpacity { .. }
            | DisplayCommand::PushBlendMode { .. }
            | DisplayCommand::PushMaskImage { .. }
            | DisplayCommand::PushMaskLinearGradient { .. }
            | DisplayCommand::PushMaskRadialGradient { .. }
            | DisplayCommand::PushMaskConicGradient { .. }
            | DisplayCommand::PushMaskLayer { .. } => {
                ctx_stack.push(i);
                mat_stack.push(if identity_below { Some(Mat4::IDENTITY) } else { cur_mat });
                infl_stack.push(cur_infl);
            }
            DisplayCommand::PopTransform
            | DisplayCommand::PopClip
            | DisplayCommand::PopOpacity
            | DisplayCommand::PopBlendMode
            | DisplayCommand::PopMask
            | DisplayCommand::PopMaskLayer
            | DisplayCommand::PopFilter
            | DisplayCommand::PopBackdropFilter
            | DisplayCommand::PopScrollLayer
            | DisplayCommand::EndStickyLayer => {
                if ctx_stack.pop().is_none() {
                    bail!("unbalanced pop at {}", i); // malformed список
                }
                mat_stack.pop();
                infl_stack.pop();
                if let Some((_, depth)) = cur_range
                    && ctx_stack.len() < depth
                {
                    bail!("segment pop below entry depth at {}", i);
                }
            }
            _ => {
                if let Some(local) = draw_cmd_local_bbox(cmd) {
                    let eff = match local {
                        SegBounds::Empty => SegBounds::Empty,
                        SegBounds::Unbounded => SegBounds::Unbounded,
                        SegBounds::Rect(r) => {
                            let r = inflate_rect(r, cur_infl);
                            if identity_below {
                                SegBounds::Rect(r)
                            } else {
                                match cur_mat {
                                    Some(m) => SegBounds::Rect(affine_rect_bbox(&m, r)),
                                    None => SegBounds::Unbounded,
                                }
                            }
                        }
                    };
                    if cur_range.is_some() {
                        cur_bounds.union(eff);
                    } else if seg_hit(&seg_bounds, &eff) {
                        violation = Some(i);
                    }
                }
            }
        }

        if violation.is_some() {
            // Конфликт вне сегмента (cur_range == None): стеки заморожены на
            // моменте конфликта — по ним считается точка tail-cut ниже.
            break;
        }

        if let Some((ri, depth)) = cur_range
            && i + 1 == ranges[ri].end
        {
            if ctx_stack.len() != depth {
                bail!("segment unbalanced at {}", i);
            }
            seg_bounds.push(std::mem::replace(&mut cur_bounds, SegBounds::Empty));
            cur_range = None;
            next_range += 1;
        }
    }

    // Tail-split: точка отреза = начало внешней нереплеябельной группы
    // конфликтующей команды (иначе — сама команда). Всё от cut до конца
    // уходит в оверлей; сегменты, завершившиеся до cut, остаются сегментами.
    let (kept, tail): (usize, Option<(usize, Vec<usize>)>) = if let Some(vi) = violation {
        let split_pos = ctx_stack.iter().position(|&ci| !ctx_replayable(&content[ci]));
        let (cut, tail_ctx): (usize, Vec<usize>) = match split_pos {
            Some(p) => (ctx_stack[p], ctx_stack[..p].to_vec()),
            None => (vi, ctx_stack.clone()),
        };
        if cut * 2 < content.len() {
            bail!("tail cut {} too early (dl {})", cut, content.len());
        }
        // Симуляция баланса хвоста: он обязан закрыть реплеенный контекст
        // и выйти в ноль (весь список сбалансирован эмиттером).
        let mut depth = tail_ctx.len() as i64;
        for cmd in &content[cut..] {
            depth += layer_push_pop_delta(cmd);
            if depth < 0 {
                bail!("tail below entry depth");
            }
        }
        if depth != 0 {
            bail!("tail unbalanced at end: {depth}");
        }
        if crate::frame_log_level() >= 2 {
            eprintln!(
                "[frame:wgpu] anim-split tail cut at {} of {} (violation at {})",
                cut,
                content.len(),
                vi,
            );
        }
        (seg_bounds.len(), Some((cut, tail_ctx)))
    } else {
        (ranges.len(), None)
    };

    // План: каждый сегмент — реплей внешнего контекста + команды сегмента +
    // закрывающие Pop-ы в LIFO-порядке; затем хвост (закрывает реплеенный
    // контекст собственными Pop-ами — они в нём уже есть).
    let mut plan: DisplayList = Vec::new();
    let mut effective: Vec<std::ops::Range<usize>> = Vec::with_capacity(kept + 1);
    for (k, r) in ranges.iter().take(kept).enumerate() {
        for &ci in &seg_ctx[k] {
            plan.push(content[ci].clone());
        }
        plan.extend_from_slice(&content[r.clone()]);
        for &ci in seg_ctx[k].iter().rev() {
            plan.push(pop_for_ctx(&content[ci]));
        }
        effective.push(r.clone());
    }
    if let Some((cut, tail_ctx)) = tail {
        for &ci in &tail_ctx {
            plan.push(content[ci].clone());
        }
        plan.extend_from_slice(&content[cut..]);
        effective.push(cut..content.len());
    }
    Some((plan, effective))
}

/// Δ push/pop-глубины layer-команды: +1 для Push*/Begin*, −1 для Pop*/End*.
fn layer_push_pop_delta(cmd: &DisplayCommand) -> i64 {
    match cmd {
        DisplayCommand::PushTransform { .. }
        | DisplayCommand::PushClipRect { .. }
        | DisplayCommand::PushClipRoundedRect { .. }
        | DisplayCommand::PushClipPath { .. }
        | DisplayCommand::PushOpacity { .. }
        | DisplayCommand::PushBlendMode { .. }
        | DisplayCommand::PushFilter { .. }
        | DisplayCommand::PushBackdropFilter { .. }
        | DisplayCommand::PushMaskImage { .. }
        | DisplayCommand::PushMaskLinearGradient { .. }
        | DisplayCommand::PushMaskRadialGradient { .. }
        | DisplayCommand::PushMaskConicGradient { .. }
        | DisplayCommand::PushMaskLayer { .. }
        | DisplayCommand::PushScrollLayer { .. }
        | DisplayCommand::BeginStickyLayer { .. } => 1,
        DisplayCommand::PopTransform
        | DisplayCommand::PopClip
        | DisplayCommand::PopOpacity
        | DisplayCommand::PopBlendMode
        | DisplayCommand::PopFilter
        | DisplayCommand::PopBackdropFilter
        | DisplayCommand::PopMask
        | DisplayCommand::PopMaskLayer
        | DisplayCommand::PopScrollLayer
        | DisplayCommand::EndStickyLayer => -1,
        _ => 0,
    }
}

/// Пересекает ли `eff` какой-либо из завершённых сегментов.
fn seg_hit(seg_bounds: &[SegBounds], eff: &SegBounds) -> bool {
    if matches!(eff, SegBounds::Empty) {
        return false;
    }
    seg_bounds.iter().any(|s| match (s, eff) {
        (SegBounds::Empty, _) | (_, SegBounds::Empty) => false,
        (SegBounds::Unbounded, _) | (_, SegBounds::Unbounded) => true,
        (SegBounds::Rect(a), SegBounds::Rect(b)) => rects_overlap(a, b),
    })
}

/// Результат сравнения двух display-list-ов.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DiffResult {
    /// Если true, то оба display list-а идентичны — можно пропустить GPU upload.
    pub identical: bool,
    ///累積bounding rectangle всех команд, которые изменились или добавились.
    /// Используется для dirty-rect tracking в renderer-е.
    /// `Rect { x: f32::NAN, y: f32::NAN, width: 0.0, height: 0.0 }` если нет изменений.
    pub changed_rects: Rect,
}

impl DiffResult {
    /// Создаёт DiffResult для идентичных display list-ов.
    #[inline]
    pub fn identical() -> Self {
        Self {
            identical: true,
            changed_rects: Rect {
                x: f32::NAN,
                y: f32::NAN,
                width: 0.0,
                height: 0.0,
            },
        }
    }

    /// Создаёт DiffResult для изменённых display list-ов с заданным bounding rect.
    #[inline]
    pub fn changed(changed_rects: Rect) -> Self {
        Self {
            identical: false,
            changed_rects,
        }
    }
}

/// Сравнивает два display list-а по Debug hash каждой команды.
/// Возвращает DiffResult с флагом `identical` и bounding rectangle всех изменений.
///
/// Алгоритм:
/// 1. Если длины списков различаются → список изменился
/// 2. Для каждой пары команд вычисляем Debug hash и сравниваем
/// 3. Если все хеши совпадают → `identical = true`
/// 4. Если есть отличия → собираем bounding rect всех `rect`-полей из изменённых команд
pub fn diff_display_lists(prev: &[DisplayCommand], next: &[DisplayCommand]) -> DiffResult {
    // Быстрая проверка: если длины различаются, список точно изменился.
    if prev.len() != next.len() {
        return DiffResult::changed(union_all_rects(next));
    }

    // Вычисляем hashes обеих последовательностей и сравниваем поэлементно.
    let mut all_identical = true;
    let mut changed_rects = Rect {
        x: f32::INFINITY,
        y: f32::INFINITY,
        width: 0.0,
        height: 0.0,
    };

    for (prev_cmd, next_cmd) in prev.iter().zip(next.iter()) {
        // Debug-представление через HashFmt — без String-аллокаций на команду.
        let prev_hash = hash_one_command(prev_cmd);
        let next_hash = hash_one_command(next_cmd);

        if prev_hash != next_hash {
            all_identical = false;
            // Собираем rect из обеих команд (старая + новая).
            if let Some(rect) = get_command_rect(prev_cmd) {
                changed_rects = union_rects(changed_rects, rect);
            }
            if let Some(rect) = get_command_rect(next_cmd) {
                changed_rects = union_rects(changed_rects, rect);
            }
        }
    }

    if all_identical {
        DiffResult::identical()
    } else {
        DiffResult::changed(changed_rects)
    }
}

/// Извлекает rect из DisplayCommand, если применимо.
pub(crate) fn get_command_rect(cmd: &DisplayCommand) -> Option<Rect> {
    match cmd {
        DisplayCommand::FillRect { rect, .. } => Some(*rect),
        DisplayCommand::FillRoundedRect { rect, .. } => Some(*rect),
        DisplayCommand::DrawBorder { rect, .. } => Some(*rect),
        DisplayCommand::DrawOutline { rect, .. } => Some(*rect),
        DisplayCommand::DrawText { rect, .. } => Some(*rect),
        DisplayCommand::DrawImage { rect, .. } => Some(*rect),
        DisplayCommand::LazyImageSlot { rect, .. } => Some(*rect),
        DisplayCommand::DrawBackgroundImage { rect, .. } => Some(*rect),
        DisplayCommand::DrawLinearGradient { rect, .. } => Some(*rect),
        DisplayCommand::DrawRadialGradient { rect, .. } => Some(*rect),
        DisplayCommand::DrawConicGradient { rect, .. } => Some(*rect),
        _ => None,
    }
}

/// Объединяет two rectangles в их bounding rect.
fn union_rects(a: Rect, b: Rect) -> Rect {
    if a.width == 0.0 && a.height == 0.0 {
        return b;
    }
    if b.width == 0.0 && b.height == 0.0 {
        return a;
    }

    let x1 = a.x.min(b.x);
    let y1 = a.y.min(b.y);
    let x2 = (a.x + a.width).max(b.x + b.width);
    let y2 = (a.y + a.height).max(b.y + b.height);

    Rect {
        x: x1,
        y: y1,
        width: (x2 - x1).max(0.0),
        height: (y2 - y1).max(0.0),
    }
}

/// Собирает bounding rect всех команд в display list.
fn union_all_rects(cmds: &[DisplayCommand]) -> Rect {
    let mut result = Rect {
        x: f32::INFINITY,
        y: f32::INFINITY,
        width: 0.0,
        height: 0.0,
    };

    for cmd in cmds {
        if let Some(rect) = get_command_rect(cmd) {
            result = union_rects(result, rect);
        }
    }

    // Если нет ни одного rect-команды, вернуть нулевой rect.
    if result.x == f32::INFINITY {
        result = Rect {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
        };
    }

    result
}
