//! Deterministic CPU-path `@font-face` byte resolution (FONTLOAD-18).
//!
//! Mirrors the wgpu renderer's real-face resolution (FONTLOAD-9/16/17,
//! `renderer/glyph_raster.rs`) for [`crate::cpu_raster`], scoped to a single
//! face per `DrawText` run (the CPU path shapes a whole run with one `bytes`
//! buffer — no per-codepoint `unicode-range` cascade, see
//! `bugs/BUG-467-OPEN.md` FONTLOAD-18 for what's deferred).

use lumen_core::{FaceRecord, FontProvider, NORMAL_STRETCH_PERCENT};

/// Bundled Inter Regular — the default (and, without `LUMEN_CPU_SYSTEM_FONTS`
/// or a matching registered `@font-face`, only) face the deterministic CPU
/// path rasterizes. Mirrors `INTER_FONT` in `lumen-driver`. See
/// [`resolve_face`] for the two escape hatches (registered `@font-face`
/// bytes, and the `LUMEN_CPU_SYSTEM_FONTS` conformance probe flag).
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

/// Resolve the sfnt bytes a `DrawText` run should rasterize with, plus the
/// declared `@font-face` metadata (`unicode-range`/override descriptors) when
/// the match came from a real, page-registered face.
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
/// Falls back to bundled Inter (no [`FaceRecord`]) when `provider` is `None`,
/// no name matches a registered face, or the registered face's bytes can't be
/// read — this must never fail the draw.
pub(crate) fn resolve_face(
    provider: Option<&dyn FontProvider>,
    font_family: &[String],
    weight: u16,
    style: lumen_layout::FontStyle,
) -> (Vec<u8>, Option<FaceRecord>) {
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
                return (bytes.to_vec(), Some(record));
            }
        }
    }
    if cpu_system_fonts_enabled() {
        for family in font_family {
            if let Some(record) =
                lumen_font::shared_system_index().pick_face(family, weight, core_style, NORMAL_STRETCH_PERCENT)
                && let Ok(bytes) = std::fs::read(&record.path)
            {
                return (bytes, None);
            }
        }
    }
    (BUNDLED_FONT.to_vec(), None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_font::FontRegistry;
    use lumen_layout::FontStyle;

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
