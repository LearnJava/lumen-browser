//! Text rendering: font resolution, run drawing/measuring, variable-font
//! outline fallback.
//!
//! Вырезано из `femtovg_backend.rs` (SPLIT-PR2 срез 1/4) без изменения
//! поведения — чисто перенос кода между модулями.

use super::*;

// ─── Font family resolution ──────────────────────────────────────────────────

/// Резолвит одну CSS-family в системный face.
///
/// Generic-имя (`serif`/`sans-serif`/`monospace`/`cursive`/`fantasy`/
/// `system-ui`) идёт через таблицу платформенных кандидатов (BUG-128),
/// конкретное — напрямую через `pick_face`. Близнец
/// `Renderer::pick_family_face` в wgpu-бэкенде: оба текстовых пути обязаны
/// выбирать один и тот же face, иначе femtovg и wgpu рисуют страницу разными
/// шрифтами.
fn pick_family_face(
    provider: &Arc<dyn FontProvider>,
    family: &str,
    weight: u16,
    style: lumen_core::ext::FontStyle,
    stretch: u16,
) -> Option<lumen_core::ext::FaceRecord> {
    if lumen_core::ext::is_generic_family(family) {
        provider.pick_generic_face(family, weight, style, stretch)
    } else {
        provider.pick_face(family, weight, style, stretch)
    }
}

/// The style half of a `DisplayCommand::DrawText` — everything except the
/// destination rectangle and the run text.
///
/// Grouped into one struct so the three `text-orientation` paths (horizontal,
/// rotated, mixed) can hand the same bundle down to
/// [`FemtovgBackend::draw_text_run`] instead of repeating a ten-argument call
/// at each call site.
pub(super) struct TextRunStyle<'a> {
    /// Used font size in CSS pixels.
    pub(super) font_size: f32,
    /// Resolved text colour.
    pub(super) color: Color,
    /// `font-family` list, in cascade order (empty = chrome UI face, DS-4).
    pub(super) font_family: &'a [String],
    /// Numeric `font-weight` (100–900).
    pub(super) font_weight: u16,
    /// Computed `font-style`.
    pub(super) font_style: FontStyle,
    /// Computed `font-stretch` as a percentage (100 = normal).
    pub(super) font_stretch: u16,
    /// `font-variation-settings` axes; non-empty routes through the
    /// outline-based variable-font path (BUG-109).
    pub(super) font_variation_axes: &'a [([u8; 4], f32)],
    /// `font-feature-settings` tags passed to the shaper.
    pub(super) font_features: &'a [([u8; 4], u32)],
    /// Tab advance in CSS pixels (CSS Text L3 §10.1).
    pub(super) tab_size: f32,
}

impl FemtovgBackend {
    pub(super) fn load_font_by_path(&mut self, path: &Path, provider: &Arc<dyn FontProvider>) -> Option<femtovg::FontId> {
        if let Some(&id) = self.loaded_fonts.get(path) {
            return Some(id);
        }
        let bytes: Arc<[u8]> = if let Some(mem) = provider.read_face_bytes(path) {
            mem
        } else {
            Arc::from(std::fs::read(path).ok()?)
        };
        let id = self.canvas.add_font_mem(&bytes).ok()?;
        self.loaded_fonts.insert(path.to_owned(), id);
        Some(id)
    }

    /// Resolves CSS `font-family` list + weight/style to a femtovg font chain.
    ///
    /// Order: CSS-declared families (first match wins per CSS Fonts L4 §3.1;
    /// reserved bundled names "Golos Text"/"Golos Text Medium"/"JetBrains Mono"
    /// resolve to the DS-4 chrome faces without touching the provider) →
    /// bundled Inter → curated system fallbacks (emoji/CJK/RTL/Indic/Thai).
    /// Generic keywords (serif/sans-serif/monospace/cursive/fantasy/system-ui)
    /// resolve through the platform candidate table (BUG-128,
    /// [`pick_family_face`]); when no candidate is installed they fall through
    /// to Inter as before.
    /// An empty `families` list — every chrome `DrawText` call site, since
    /// chrome never resolves CSS font-family — defaults to bundled Golos Text.
    /// Returns at least `[inter_id]` when no provider is set.
    ///
    /// The two booleans report whether the FIRST provider-resolved face is a
    /// true bold (`weight >= 600`) / true italic — callers use them to decide
    /// whether synthetic bold/italic fallbacks are needed (RP-6). When the
    /// chain is bundled-Inter-only both flags are `false`.
    fn resolve_font_chain(
        &mut self,
        families: &[String],
        weight: u16,
        style: FontStyle,
        stretch: u16,
    ) -> (Vec<femtovg::FontId>, bool, bool) {
        let mut ids: Vec<femtovg::FontId> = Vec::new();
        let mut true_bold = false;
        let mut true_italic = false;

        // DS-4: chrome never queries the CSS FontProvider — every chrome
        // `DrawText` passes an empty `font_family` (page content always has a
        // non-empty one, from the UA/author stylesheet's font-family cascade),
        // so an empty list defaults to the bundled chrome UI face. Reserved
        // bundled family names ("Golos Text"/"Golos Text Medium"/"JetBrains
        // Mono") resolve directly and skip the provider lookup below —
        // independent of whether a `FontProvider` is installed at all.
        if families.is_empty() {
            if let Some(chrome) = self.chrome_font_id {
                ids.push(chrome);
            }
        } else {
            let provider = self.font_provider.clone();
            let core_style = match style {
                FontStyle::Normal => lumen_core::ext::FontStyle::Normal,
                FontStyle::Italic => lumen_core::ext::FontStyle::Italic,
                FontStyle::Oblique => lumen_core::ext::FontStyle::Oblique,
            };
            for fam in families {
                match fam.as_str() {
                    "Golos Text" => {
                        if let Some(id) = self.chrome_font_id
                            && !ids.contains(&id)
                        {
                            ids.push(id);
                        }
                        continue;
                    }
                    "Golos Text Medium" => {
                        if let Some(id) = self.chrome_font_medium_id
                            && !ids.contains(&id)
                        {
                            ids.push(id);
                        }
                        continue;
                    }
                    "JetBrains Mono" => {
                        if let Some(id) = self.mono_font_id
                            && !ids.contains(&id)
                        {
                            ids.push(id);
                        }
                        continue;
                    }
                    _ => {}
                }
                let Some(provider) = provider.as_ref() else { continue };
                if let Some(rec) = pick_family_face(provider, fam, weight, core_style, stretch)
                    && let Some(id) = self.load_font_by_path(&rec.path.clone(), provider)
                    && !ids.contains(&id)
                {
                    if ids.is_empty() {
                        true_bold = rec.weight >= 600;
                        true_italic = !matches!(rec.style, lumen_core::ext::FontStyle::Normal);
                    }
                    ids.push(id);
                    // BUG-434: @font-face subsets of the same (family, weight,
                    // style, stretch) partition the codepoint space via
                    // non-overlapping `unicode-range` (CSS Fonts L4 §5.1)
                    // instead of competing — `pick_family_face` only ever
                    // returns one of them. Load every sibling right after the
                    // primary so femtovg's own per-glyph fallback across the
                    // chain can reach the subset that actually has the glyph,
                    // ahead of the next fallback family.
                    for sibling in provider.lookup_faces(fam) {
                        if sibling.path == rec.path
                            || sibling.weight != rec.weight
                            || sibling.style != rec.style
                            || sibling.stretch != rec.stretch
                        {
                            continue;
                        }
                        if let Some(sib_id) = self.load_font_by_path(&sibling.path.clone(), provider)
                            && !ids.contains(&sib_id)
                        {
                            ids.push(sib_id);
                        }
                    }
                }
            }
        }

        // Bundled Inter as the primary Latin fallback (DS-4: stays in the chain
        // even for chrome text, per the design brief — Golos/mono are primary,
        // Inter still backstops any glyph missing from those bundled faces).
        if let Some(inter) = self.font_id
            && !ids.contains(&inter)
        {
            ids.push(inter);
        }

        // Curated system fallbacks (emoji/CJK/RTL/Indic/Thai) appended last.
        for &fb in &self.fallback_chain.clone() {
            if !ids.contains(&fb) {
                ids.push(fb);
            }
        }

        (ids, true_bold, true_italic)
    }

    /// Draws one horizontal text run into `rect` — the whole body of a
    /// `DisplayCommand::DrawText` minus the `text-orientation` dispatch.
    ///
    /// BUG-109: femtovg's text API cannot apply `font-variation-settings`
    /// axes. When axes are present and resolve to a variable face, the run is
    /// rendered via lumen-font outlines (vector fill) so wght/wdth/slnt take
    /// effect; otherwise femtovg's fast text path runs, with synthetic
    /// bold/italic filled in when the picked face is not truly bold/italic.
    ///
    /// Everything here works through the *current* canvas transform, so the
    /// vertical paths ([`Self::draw_text_mixed`] and the `Sideways` branch of
    /// the `DrawText` arm) get rotation for free by installing
    /// [`rotate_cw_transform`] and passing a local-space `rect`.
    pub(super) fn draw_text_run(&mut self, rect: &Rect, text: &str, s: &TextRunStyle<'_>) {
        if !s.font_variation_axes.is_empty()
            && self.draw_varied_text(
                rect, text, s.font_size, s.color, s.font_family,
                s.font_weight, s.font_style, s.font_stretch,
                s.font_variation_axes, s.font_features, s.tab_size,
            )
        {
            return;
        }
        let (chain, true_bold, true_italic) =
            self.resolve_font_chain(s.font_family, s.font_weight, s.font_style, s.font_stretch);
        let synth_bold = s.font_weight >= 600 && !true_bold;
        let synth_italic = !matches!(s.font_style, FontStyle::Normal) && !true_italic;
        self.draw_text_styled(
            rect.x, rect.y, text, s.font_size, s.color, &chain, synth_bold, synth_italic,
        );
    }

    /// Shaped horizontal advance of `text` under `s`, in CSS pixels.
    ///
    /// Used by [`Self::draw_text_mixed`] to step the column cursor: ink extent
    /// alone would misplace a whitespace-only segment (no glyph, but a real
    /// advance). Measured through femtovg's own shaper — the same one that
    /// will draw the segment — so cursor and glyphs cannot drift apart. A run
    /// that ends up on the variable-font outline path (BUG-109) is still
    /// measured with the static face femtovg picks; per-segment drift there is
    /// bounded by the axis delta and is not worth a second shaping engine.
    fn measure_run_advance(&mut self, text: &str, s: &TextRunStyle<'_>) -> f32 {
        if text.is_empty() {
            return 0.0;
        }
        let (chain, _, _) =
            self.resolve_font_chain(s.font_family, s.font_weight, s.font_style, s.font_stretch);
        let mut paint = femtovg::Paint::color(lumen_to_fvg(s.color));
        if !chain.is_empty() {
            paint.set_font(&chain);
        }
        paint.set_font_size(s.font_size);
        self.canvas
            .measure_text(0.0, 0.0, text, &paint)
            .map(|m| m.width())
            .unwrap_or(0.0)
    }

    /// Ph3 writing-mode vertical — per-glyph split for `text-orientation:
    /// mixed` (CSS Writing Modes L4 §4), femtovg path.
    ///
    /// Each CJK ideograph paints upright at an increasing offset down `dest`'s
    /// column (no rotation — exactly the horizontal path, just moved down);
    /// each run of consecutive non-CJK characters is drawn as one block at the
    /// local origin under [`rotate_cw_transform`], so Latin kerning and
    /// ligatures inside a word stay intact. Mirrors the CPU rasterizer's
    /// `rasterize_text_mixed` and the wgpu renderer's `push_text_glyphs_mixed`
    /// — including the local-x offset trick: the rotated segment is laid out
    /// at `x = y_cursor` in the pre-rotation frame, which the rotation turns
    /// into a downward shift of `y_cursor` in `dest`'s column.
    pub(super) fn draw_text_mixed(&mut self, dest: &Rect, text: &str, s: &TextRunStyle<'_>) {
        let mut y_cursor = 0.0_f32;
        for seg in crate::display_list::split_mixed_runs(text) {
            let (seg_text, upright) = match seg {
                crate::display_list::MixedSegment::Cjk(ch) => (ch.to_string(), true),
                crate::display_list::MixedSegment::Other(txt) => (txt, false),
            };
            // Measured before any rotation is installed: `Canvas::measure_text`
            // quantizes the font size by the current transform's average scale,
            // and keeping the measurement in the unrotated frame matches what
            // the upright branch will actually draw.
            let advance = self.measure_run_advance(&seg_text, s);
            if upright {
                let seg_rect = Rect::new(dest.x, dest.y + y_cursor, dest.width, dest.height);
                self.draw_text_run(&seg_rect, &seg_text, s);
            } else {
                self.canvas.save();
                self.canvas.set_transform(&rotate_cw_transform(*dest));
                let local = Rect::new(y_cursor, 0.0, dest.height, dest.width);
                self.draw_text_run(&local, &seg_text, s);
                self.canvas.restore();
            }
            y_cursor += advance;
        }
    }

    /// Рисует текст с опциональными синтетическими bold/italic (RP-6).
    ///
    /// Fake bold — повторный `fill_text` со сдвигом вправо; fake italic —
    /// shear-трансформ ~12° вокруг baseline. Оба эффекта чисто визуальные:
    /// advance-метрики layout-а не меняются.
    #[allow(clippy::too_many_arguments)]
    fn draw_text_styled(
        &mut self,
        x: f32,
        y: f32,
        text: &str,
        font_size: f32,
        color: Color,
        chain: &[femtovg::FontId],
        synth_bold: bool,
        synth_italic: bool,
    ) {
        let mut paint = femtovg::Paint::color(lumen_to_fvg(color));
        if !chain.is_empty() {
            paint.set_font(chain);
        }
        paint.set_font_size(font_size);
        let baseline_y = y + font_size * 0.8;

        if synth_italic {
            self.canvas.save();
            let s = 0.2126; // tan 12°
            let transform = femtovg::Transform2D([1.0, 0.0, -s, 1.0, s * baseline_y, 0.0]);
            self.canvas.set_transform(&transform);
        }

        let _ = self.canvas.fill_text(x, baseline_y, text, &paint);

        if synth_bold {
            let bold_offset = (font_size / 24.0).clamp(0.5, 2.0);
            let _ = self.canvas.fill_text(x + bold_offset, baseline_y, text, &paint);
        }

        if synth_italic {
            self.canvas.restore();
        }
    }

    /// BUG-109: renders a text run with `font-variation-settings` axes applied,
    /// bypassing femtovg's variation-blind text engine.
    ///
    /// Resolves the first CSS-declared family that maps to a **variable** face,
    /// builds filled-glyph paths at the requested axis coordinates via
    /// [`crate::varied_text::build_varied_text_paths`], and fills them with the
    /// text colour through the current canvas transform/clip. Returns `true`
    /// when the run was rendered here; `false` when no variable face was found
    /// (no provider, only static/generic families) so the caller falls back to
    /// femtovg's native text path.
    #[allow(clippy::too_many_arguments)]
    fn draw_varied_text(
        &mut self,
        rect: &Rect,
        text: &str,
        font_size: f32,
        color: Color,
        families: &[String],
        weight: u16,
        style: FontStyle,
        stretch: u16,
        axes: &[([u8; 4], f32)],
        features: &[([u8; 4], u32)],
        tab_size: f32,
    ) -> bool {
        let Some(provider) = self.font_provider.clone() else {
            return false;
        };
        let core_style = match style {
            FontStyle::Normal => lumen_core::ext::FontStyle::Normal,
            FontStyle::Italic => lumen_core::ext::FontStyle::Italic,
            FontStyle::Oblique => lumen_core::ext::FontStyle::Oblique,
        };
        for fam in families {
            let Some(rec) = pick_family_face(&provider, fam, weight, core_style, stretch) else {
                continue;
            };
            let Some(bytes) = provider
                .read_face_bytes(&rec.path)
                .or_else(|| std::fs::read(&rec.path).ok().map(Arc::from))
            else {
                continue;
            };
            // `build_varied_text_paths` returns None for static faces — defer to
            // the next family (and ultimately femtovg) in that case.
            if let Some(cmds) = crate::varied_text::build_varied_text_paths(
                &bytes, axes, features, text, font_size, rect.x, rect.y, tab_size,
            ) {
                self.fill_glyph_path(&cmds, color);
                return true;
            }
        }
        false
    }

    /// Fills a set of [`crate::varied_text::PathCmd`]s (screen pixels, Y-down)
    /// with a solid colour, honouring the canvas's current transform and clip.
    fn fill_glyph_path(&mut self, cmds: &[crate::varied_text::PathCmd], color: Color) {
        use crate::varied_text::PathCmd;
        if cmds.is_empty() {
            return;
        }
        let mut path = femtovg::Path::new();
        for cmd in cmds {
            match *cmd {
                PathCmd::MoveTo(x, y) => path.move_to(x, y),
                PathCmd::LineTo(x, y) => path.line_to(x, y),
                PathCmd::QuadTo(cx, cy, x, y) => path.quad_to(cx, cy, x, y),
                PathCmd::Close => path.close(),
            }
        }
        let paint = femtovg::Paint::color(lumen_to_fvg(color));
        self.canvas.fill_path(&path, &paint);
    }
}
