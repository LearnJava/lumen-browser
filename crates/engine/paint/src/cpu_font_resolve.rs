//! Deterministic CPU-path `@font-face` byte resolution (FONTLOAD-18/19).
//!
//! Mirrors the wgpu renderer's real-face resolution (FONTLOAD-9/16/17,
//! `renderer/glyph_raster.rs`) for [`crate::cpu_raster`]: [`resolve_face_candidates`]
//! resolves the same declared `font-family` stack to an ordered list of sfnt
//! candidates (primary pick + its `unicode-range` siblings, BUG-434), which
//! `cpu_raster::itemize_by_cascade` then routes per sub-run the same way the
//! wgpu path routes per codepoint.

use lumen_core::{FaceRecord, FontProvider, NORMAL_STRETCH_PERCENT};

/// Bundled Inter Regular — the default (and, without `LUMEN_CPU_SYSTEM_FONTS`
/// or a matching registered `@font-face`, only) face the deterministic CPU
/// path rasterizes. Mirrors `INTER_FONT` in `lumen-driver`. See
/// [`resolve_face_candidates`] for the two escape hatches (registered
/// `@font-face` bytes, and the `LUMEN_CPU_SYSTEM_FONTS` conformance probe flag).
const BUNDLED_FONT: &[u8] = include_bytes!("../../../../assets/fonts/Inter-Regular.ttf");

/// `LUMEN_CPU_SYSTEM_FONTS` opt-in, read once (same `OnceLock` pattern
/// `text_shaper.rs` used for its now-removed `LUMEN_OWN_TEXT_SHAPING`
/// rollback flag, LIB-3). Diagnostic-only: exists so
/// the LIB-3 conformance re-measurement can render `docs/conformance/probes/
/// text-shaping.html`'s Arabic/Devanagari/Hebrew/RTL checks against a real OS
/// face instead of bundled-Inter `.notdef` tofu. Unset in every default build,
/// CI run and graphic-test invocation, so it never touches a committed golden.
fn cpu_system_fonts_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("LUMEN_CPU_SYSTEM_FONTS").is_some())
}

/// Cascade candidate list for [`crate::cpu_raster`]'s per-codepoint
/// `@font-face` fallback (FONTLOAD-19, CSS Fonts L4 §5.1 `unicode-range`).
///
/// `provider`, when given, is the page's `lumen_font::FontRegistry` — the same
/// object the wgpu path receives via `Renderer::set_font_provider`, built by
/// `page_pipeline.rs::load_font_faces` from the page's parsed `@font-face`
/// rules. For each name in `font_family`, in order, this tries
/// `provider.pick_face` (CSS Fonts L4 §5.2 weight/style matching) and accepts
/// the result **only if `provider.read_face_bytes` returns bytes for it** —
/// that is true exactly for `@font-face`-registered in-memory faces, and
/// false for a `FaceRecord` that resolved to a plain OS system font (`pick_face`
/// merges system + custom candidates, so an ordinary `font-family: Arial`
/// fallback with no matching `@font-face` rule would otherwise match a system
/// face here too). This keeps every existing deterministic golden byte-for-byte
/// unchanged: system-font resolution on the CPU path stays exactly where it
/// was, gated behind [`cpu_system_fonts_enabled`], not silently expanded by
/// this function.
///
/// Index 0 of the returned `Vec` is that primary pick. Further entries are
/// its **BUG-434 siblings**: other `@font-face` records under the SAME
/// declared family with the SAME `(weight, style, stretch)` — the real-world
/// pattern behind `unicode-range` is subsetting one logical family into
/// several files that partition the codepoint space instead of competing for
/// it (`provider.pick_face` only ever returns one of them, by construction).
/// This mirrors `Renderer::resolve_face_id_uncached` (`renderer.rs`), the
/// wgpu path's own BUG-434 fix, exactly — down to the sibling filter.
///
/// Only the `@font-face`-registered branch has siblings: system-font and
/// bundled fallback return a single-element `Vec`, so a page with no
/// unicode-range subsetting gets byte-identical behaviour to the
/// pre-FONTLOAD-19 single-face resolve (no new cascade cost for the common
/// case — see `cpu_raster::itemize_by_cascade`'s own `len() <= 1` fast path).
///
/// Falls back to a single bundled-Inter candidate (no [`FaceRecord`]) when
/// `provider` is `None`, no name matches a registered face, or the registered
/// face's bytes can't be read — this must never fail the draw, so the
/// returned `Vec` is never empty.
pub(crate) fn resolve_face_candidates(
    provider: Option<&dyn FontProvider>,
    font_family: &[String],
    weight: u16,
    style: lumen_layout::FontStyle,
) -> Vec<(Vec<u8>, Option<FaceRecord>)> {
    let core_style = match style {
        lumen_layout::FontStyle::Normal => lumen_core::FontStyle::Normal,
        lumen_layout::FontStyle::Italic => lumen_core::FontStyle::Italic,
        lumen_layout::FontStyle::Oblique => lumen_core::FontStyle::Oblique,
    };
    if let Some(provider) = provider {
        for family in font_family {
            if let Some(record) = provider.pick_face(family, weight, core_style, NORMAL_STRETCH_PERCENT)
                && let Some(bytes) = provider.read_face_bytes(&record.path)
            {
                let mut candidates = vec![(bytes.to_vec(), Some(record.clone()))];
                for sibling in provider.lookup_faces(family) {
                    if sibling.path == record.path
                        || sibling.weight != record.weight
                        || sibling.style != record.style
                        || sibling.stretch != record.stretch
                    {
                        continue;
                    }
                    if let Some(sib_bytes) = provider.read_face_bytes(&sibling.path) {
                        candidates.push((sib_bytes.to_vec(), Some(sibling)));
                    }
                }
                return candidates;
            }
        }
    }
    if cpu_system_fonts_enabled() {
        for family in font_family {
            if let Some(record) =
                lumen_font::shared_system_index().pick_face(family, weight, core_style, NORMAL_STRETCH_PERCENT)
                && let Ok(bytes) = std::fs::read(&record.path)
            {
                return vec![(bytes, None)];
            }
        }
    }
    vec![(BUNDLED_FONT.to_vec(), None)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_font::FontRegistry;
    use lumen_layout::FontStyle;

    /// Test-only shim for the pre-FONTLOAD-19 single-face API — most of these
    /// tests only care about the primary pick, not the whole cascade.
    fn resolve_face(
        provider: Option<&dyn FontProvider>,
        font_family: &[String],
        weight: u16,
        style: FontStyle,
    ) -> (Vec<u8>, Option<FaceRecord>) {
        resolve_face_candidates(provider, font_family, weight, style)
            .into_iter()
            .next()
            .expect("resolve_face_candidates never returns empty")
    }

    #[test]
    fn no_provider_falls_back_to_bundled() {
        let (bytes, record) = resolve_face(None, &["Whatever".to_string()], 400, FontStyle::Normal);
        assert_eq!(bytes, BUNDLED_FONT.to_vec());
        assert!(record.is_none());
    }

    #[test]
    fn empty_family_list_falls_back_to_bundled() {
        let registry = FontRegistry::new();
        let (bytes, record) =
            resolve_face(Some(&registry as &dyn FontProvider), &[], 400, FontStyle::Normal);
        assert_eq!(bytes, BUNDLED_FONT.to_vec());
        assert!(record.is_none());
    }

    #[test]
    fn registered_custom_face_wins_and_carries_overrides() {
        let registry = FontRegistry::new();
        registry.register_from_bytes(
            "MyWebFont",
            400,
            lumen_core::FontStyle::Normal,
            &[],
            vec![1, 2, 3, 4],
            Some(0.9),
            Some(0.3),
            Some(1.5),
            None,
        );
        let (bytes, record) = resolve_face(
            Some(&registry as &dyn FontProvider),
            &["MyWebFont".to_string()],
            400,
            FontStyle::Normal,
        );
        assert_eq!(bytes, vec![1, 2, 3, 4]);
        let record = record.expect("custom face must be reported");
        assert_eq!(record.ascent_override, Some(0.9));
        assert_eq!(record.descent_override, Some(0.3));
        assert_eq!(record.size_adjust, Some(1.5));
    }

    #[test]
    fn unregistered_family_name_falls_back_to_bundled_not_system() {
        // A `font-family` name with no matching `@font-face` rule must NOT
        // silently pick up a real OS system font on the CPU path — that stays
        // gated behind `LUMEN_CPU_SYSTEM_FONTS` (unset in this test process),
        // or every existing deterministic golden using a common family name
        // like "Arial" would stop being cross-OS bit-identical.
        let registry = FontRegistry::new();
        let (bytes, record) = resolve_face(
            Some(&registry as &dyn FontProvider),
            &["Arial".to_string()],
            400,
            FontStyle::Normal,
        );
        assert_eq!(bytes, BUNDLED_FONT.to_vec());
        assert!(record.is_none());
    }

    #[test]
    fn siblings_with_disjoint_unicode_ranges_all_become_candidates() {
        // Mirrors WPT `css/css-fonts/font-face-unicode-range.html` and
        // `pick_face_for_codepoint_tests` (wgpu side): two `@font-face` rules
        // share one font file but declare disjoint `unicode-range` — the real
        // shape of BUG-434 subsetting. `pick_face` only ever returns one of
        // them (same weight/style/stretch); `resolve_face_candidates` must
        // surface the other as a sibling so the CPU cascade can reach it.
        let registry = FontRegistry::new();
        registry.register_from_bytes(
            "MyFont",
            400,
            lumen_core::FontStyle::Normal,
            &[lumen_font::UnicodeRange { start: 0x41, end: 0x5A }], // A-Z
            vec![1],
            None,
            None,
            None,
            None,
        );
        registry.register_from_bytes(
            "MyFont",
            400,
            lumen_core::FontStyle::Normal,
            &[lumen_font::UnicodeRange { start: 0x61, end: 0x7A }], // a-z
            vec![2],
            None,
            None,
            None,
            None,
        );
        let candidates = resolve_face_candidates(
            Some(&registry as &dyn FontProvider),
            &["MyFont".to_string()],
            400,
            FontStyle::Normal,
        );
        assert_eq!(candidates.len(), 2, "both unicode-range subsets must be candidates");
        let bytes: std::collections::HashSet<Vec<u8>> =
            candidates.iter().map(|(b, _)| b.clone()).collect();
        assert!(bytes.contains(&vec![1_u8]));
        assert!(bytes.contains(&vec![2_u8]));
    }

    #[test]
    fn sibling_with_different_weight_is_not_a_candidate() {
        // A bold face under the same family is a DIFFERENT face selection
        // (CSS Fonts L4 §5.2 weight matching), not a unicode-range subset of
        // the regular one — must not leak into the cascade.
        let registry = FontRegistry::new();
        registry.register_from_bytes(
            "MyFont",
            400,
            lumen_core::FontStyle::Normal,
            &[],
            vec![1],
            None,
            None,
            None,
            None,
        );
        registry.register_from_bytes(
            "MyFont",
            700,
            lumen_core::FontStyle::Normal,
            &[],
            vec![2],
            None,
            None,
            None,
            None,
        );
        let candidates = resolve_face_candidates(
            Some(&registry as &dyn FontProvider),
            &["MyFont".to_string()],
            400,
            FontStyle::Normal,
        );
        assert_eq!(candidates.len(), 1, "different-weight face must not be a cascade candidate");
        assert_eq!(candidates[0].0, vec![1]);
    }

    #[test]
    fn second_family_in_list_used_when_first_unregistered() {
        let registry = FontRegistry::new();
        registry.register_from_bytes(
            "Fallback",
            400,
            lumen_core::FontStyle::Normal,
            &[],
            vec![9, 9],
            None,
            None,
            None,
            None,
        );
        let (bytes, record) = resolve_face(
            Some(&registry as &dyn FontProvider),
            &["NotRegistered".to_string(), "Fallback".to_string()],
            400,
            FontStyle::Normal,
        );
        assert_eq!(bytes, vec![9, 9]);
        assert!(record.is_some());
    }
}
