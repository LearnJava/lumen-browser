//! Курируемый список fallback-имён для CSS Fonts L4 §5.3 codepoint
//! cascade. Используется Renderer-ом, чтобы покрыть эмодзи / CJK /
//! Arabic / Indic / Thai символы на страницах **без явного
//! `font-family`-указания** — иначе fallback работает только по
//! уже-загруженным face-ам (через CSS), и страница в стиле «`body
//! { font-family: Inter; }`» будет рисовать эмодзи как `.notdef`.
//!
//! Список включает имена, типичные для трёх основных платформ
//! (Linux, Windows, macOS). `Renderer::preload_curated_fallbacks`
//! пытается загрузить **все**; имена, не найденные в текущей ОС,
//! тихо пропускаются. Идемпотентен — повторный вызов на уже
//! загруженной семье — no-op благодаря cache в Renderer.

/// Курируемый fallback chain, упорядоченный по приоритету. Используется
/// `Renderer::preload_curated_fallbacks` (см. `lumen-paint::renderer`).
///
/// Категории (в порядке размещения):
/// 1. **Emoji** — color emoji-шрифты. На странице без явной
///    `font-family` для эмодзи нужны до `.notdef` через cascade.
/// 2. **CJK** — Sans-варианты для упрощённого китайского / японского /
///    корейского / традиционного китайского. Покрывает текст
///    «你好世界 / 日本語 / 한글 / 繁體中文» без явной family.
/// 3. **Arabic / Hebrew** — RTL-скрипты.
/// 4. **Indic** — Devanagari / Tamil / Bengali и т.д.
/// 5. **Thai** — отдельный шрифт для тайского.
///
/// Каждая категория содержит pluri-platform варианты, чтобы один из
/// них нашёлся на любой ОС:
/// - **Linux**: `Noto …` (Google Noto-семья, обычно в `fonts-noto-*`).
/// - **macOS**: `Apple Color Emoji`, `Hiragino …`, `PingFang …`.
/// - **Windows**: `Segoe UI Emoji`, `Microsoft YaHei`, `Yu Gothic`,
///   `Malgun Gothic`.
pub const CURATED_FALLBACK_FAMILIES: &[&str] = &[
    // ── Emoji ─────────────────────────────────────────────
    "Noto Color Emoji",
    "Apple Color Emoji",
    "Segoe UI Emoji",
    "Twemoji Mozilla",
    // ── CJK Sans ─────────────────────────────────────────
    "Noto Sans CJK SC",
    "Noto Sans CJK JP",
    "Noto Sans CJK KR",
    "Noto Sans CJK TC",
    "Microsoft YaHei",
    "Yu Gothic",
    "Malgun Gothic",
    "Hiragino Sans",
    "PingFang SC",
    "PingFang TC",
    // ── RTL ──────────────────────────────────────────────
    "Noto Sans Arabic",
    "Noto Sans Hebrew",
    "Segoe UI",
    // ── Indic ────────────────────────────────────────────
    "Noto Sans Devanagari",
    "Noto Sans Tamil",
    "Noto Sans Bengali",
    "Noto Sans Telugu",
    "Noto Sans Gujarati",
    // ── Thai ─────────────────────────────────────────────
    "Noto Sans Thai",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curated_list_is_non_empty() {
        assert!(!CURATED_FALLBACK_FAMILIES.is_empty());
    }

    #[test]
    fn curated_list_has_emoji_coverage() {
        // Минимум один emoji-шрифт для каждой платформы должен быть
        // в списке — иначе на странице без CSS-emoji-family эмодзи
        // никогда не покроются.
        let has_emoji = CURATED_FALLBACK_FAMILIES.iter().any(|f| {
            f.contains("Emoji") || f.contains("Twemoji")
        });
        assert!(has_emoji);
    }

    #[test]
    fn curated_list_has_cjk_coverage() {
        // Хотя бы один CJK-шрифт.
        let has_cjk = CURATED_FALLBACK_FAMILIES.iter().any(|f| {
            f.contains("CJK")
                || f.contains("YaHei")
                || f.contains("Hiragino")
                || f.contains("PingFang")
                || f.contains("Yu Gothic")
                || f.contains("Malgun")
        });
        assert!(has_cjk);
    }

    #[test]
    fn curated_list_has_arabic_coverage() {
        let has_arabic = CURATED_FALLBACK_FAMILIES.iter().any(|f| f.contains("Arabic"));
        assert!(has_arabic);
    }

    #[test]
    fn curated_list_entries_are_non_empty() {
        // Защита от опечатки `""` — Renderer не должен получать пустое имя.
        for f in CURATED_FALLBACK_FAMILIES {
            assert!(!f.is_empty(), "пустое имя family запрещено");
        }
    }

    #[test]
    fn curated_list_no_duplicates() {
        // Идемпотентность Renderer::preload_fallback_chain покрывает
        // дубли через cache, но в const-списке они избыточны — лишняя
        // работа при первом проходе.
        let mut sorted: Vec<&&str> = CURATED_FALLBACK_FAMILIES.iter().collect();
        sorted.sort();
        sorted.dedup();
        assert_eq!(
            sorted.len(),
            CURATED_FALLBACK_FAMILIES.len(),
            "дубликаты в CURATED_FALLBACK_FAMILIES"
        );
    }

    #[test]
    fn curated_list_emoji_comes_before_cjk() {
        // Inв codepoint cascade emoji-шрифт должен быть проверен раньше,
        // чем CJK-шрифт, иначе у CJK-шрифтов с эмодзи в cmap (например,
        // Noto Sans CJK содержит эмодзи как mono-glyphs) выиграет CJK,
        // и эмодзи окажется чёрно-белым вместо цветного.
        let first_emoji_idx = CURATED_FALLBACK_FAMILIES
            .iter()
            .position(|f| f.contains("Emoji") || f.contains("Twemoji"));
        let first_cjk_idx = CURATED_FALLBACK_FAMILIES
            .iter()
            .position(|f| f.contains("CJK") || f.contains("YaHei"));
        if let (Some(e), Some(c)) = (first_emoji_idx, first_cjk_idx) {
            assert!(e < c, "emoji должно быть раньше CJK в cascade");
        }
    }
}
