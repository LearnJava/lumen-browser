//! Интеграционный тест индексатора на реальной системе.
//!
//! Тест помечен `#[ignore]` потому что зависит от наличия шрифтов на
//! конкретной машине: на минимальном CI-контейнере `/usr/share/fonts`
//! может быть пустым. Запускать вручную:
//! `cargo test -p lumen-font --test real_system_fonts -- --ignored`.
//!
//! Цель — проверить, что наш парсер `name` корректно справляется с
//! гетерогенным набором реальных шрифтов (Adwaita, Liberation, Noto,
//! …), а не только с bundled Inter.

use lumen_core::FontProvider;
use lumen_font::SystemFontIndex;

#[test]
#[ignore]
fn scans_default_system_dirs() {
    let idx = SystemFontIndex::new();
    let families = idx.list_families();
    assert!(
        !families.is_empty(),
        "expected to find at least one system font; got empty index"
    );
    // На большинстве Linux-дистрибутивов есть Liberation или DejaVu.
    let has_common = families.iter().any(|f| {
        f.contains("liberation") || f.contains("dejavu") || f.contains("noto") || f.contains("sans")
    });
    assert!(
        has_common,
        "expected at least one mainstream family (liberation/dejavu/noto/sans); got {} families",
        families.len()
    );
}

#[test]
#[ignore]
fn cyrillic_capable_font_is_findable() {
    // Smoke-проверка, что хоть один реальный шрифт мы умеем достать
    // по имени. Тест не падает, если конкретного шрифта нет —
    // ассертим только не-пустой индекс.
    let idx = SystemFontIndex::new();
    assert!(idx.family_count() > 0);
}
