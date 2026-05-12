//! Реализация trait-а `lumen_core::ext::EncodingDetector` поверх нашего
//! пайплайна BOM + meta + content-type + UTF-8 + эвристика.
//!
//! Нужна для случаев, когда потребитель работает абстрактно через trait
//! (например, плагин, swap на пользовательский детектор для тестов).
//! Прямой вызов [`crate::detect`] остаётся доступен.

use lumen_core::ext::EncodingDetector;

use crate::detect;

/// Детектор кодировок по умолчанию.
///
/// Stateless и cheap-to-construct, поэтому отдельного `new()` не нужно —
/// `HeuristicDetector` хватит.
#[derive(Debug, Default, Clone, Copy)]
pub struct HeuristicDetector;

impl EncodingDetector for HeuristicDetector {
    fn detect(&self, bytes: &[u8], content_type_hint: Option<&str>) -> Option<&'static str> {
        Some(detect(bytes, content_type_hint).name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trait_returns_utf8_for_bom() {
        let d = HeuristicDetector;
        assert_eq!(d.detect(b"\xEF\xBB\xBFhello", None), Some("utf-8"));
    }

    #[test]
    fn trait_returns_cp1251_for_meta() {
        let d = HeuristicDetector;
        let html = b"<meta charset=windows-1251>";
        assert_eq!(d.detect(html, None), Some("windows-1251"));
    }
}
