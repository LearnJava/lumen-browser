//! Алгоритм определения парсинг-режима по DOCTYPE (HTML5 §13.2.5.1
//! «The initial insertion mode»).
//!
//! Чистая функция от трёх полей DOCTYPE-токена. Возвращает один из
//! [`DocumentMode`] вариантов; tree_builder сохраняет результат в
//! [`Document::set_mode`] при обработке первого DOCTYPE-токена.
//!
//! Все сравнения — ASCII case-insensitive по spec («using ASCII
//! case-insensitive matching»). public_id/system_id различают
//! «отсутствует» (None) от «empty string» (Some("")) — это критично
//! для limited-quirks правил HTML 4.01 Frameset/Transitional.

use lumen_dom::DocumentMode;

/// Решение по §13.2.5.1. `public_id`/`system_id` — `None` если в
/// исходнике не было соответствующего keyword-а (PUBLIC/SYSTEM),
/// `Some("")` если был с пустой строкой.
pub fn detect_document_mode(
    name: &str,
    public_id: Option<&str>,
    system_id: Option<&str>,
) -> DocumentMode {
    // §13.2.5.1 шаг «set the Document to quirks mode»:
    // 1. name отличается от "html" (case-insensitive).
    if !name.eq_ignore_ascii_case("html") {
        return DocumentMode::Quirks;
    }

    // 2. public_id — один из EXACT_QUIRKS_PUBLIC_IDS (case-insensitive).
    if let Some(pub_id) = public_id
        && EXACT_QUIRKS_PUBLIC_IDS
            .iter()
            .any(|p| pub_id.eq_ignore_ascii_case(p))
    {
        return DocumentMode::Quirks;
    }

    // 3. system_id равен EXACT_QUIRKS_SYSTEM_ID (case-insensitive).
    if let Some(sys_id) = system_id
        && sys_id.eq_ignore_ascii_case(EXACT_QUIRKS_SYSTEM_ID)
    {
        return DocumentMode::Quirks;
    }

    // 4. public_id начинается (case-insensitive) с одного из
    //    QUIRKS_PUBLIC_ID_PREFIXES.
    if let Some(pub_id) = public_id
        && QUIRKS_PUBLIC_ID_PREFIXES
            .iter()
            .any(|p| ci_starts_with(pub_id, p))
    {
        return DocumentMode::Quirks;
    }

    // 5. system_id отсутствует AND public_id начинается с одного из
    //    FRAMESET_TRANSITIONAL_PREFIXES.
    if system_id.is_none()
        && let Some(pub_id) = public_id
        && FRAMESET_TRANSITIONAL_PREFIXES
            .iter()
            .any(|p| ci_starts_with(pub_id, p))
    {
        return DocumentMode::Quirks;
    }

    // §13.2.5.1 шаг «set the Document to limited-quirks mode»:
    // 6. public_id начинается с одного из XHTML_FRAMESET_PREFIXES.
    if let Some(pub_id) = public_id
        && XHTML_FRAMESET_PREFIXES
            .iter()
            .any(|p| ci_starts_with(pub_id, p))
    {
        return DocumentMode::LimitedQuirks;
    }

    // 7. system_id присутствует AND public_id начинается с одного из
    //    FRAMESET_TRANSITIONAL_PREFIXES.
    if system_id.is_some()
        && let Some(pub_id) = public_id
        && FRAMESET_TRANSITIONAL_PREFIXES
            .iter()
            .any(|p| ci_starts_with(pub_id, p))
    {
        return DocumentMode::LimitedQuirks;
    }

    DocumentMode::NoQuirks
}

/// ASCII case-insensitive prefix-match. Возвращает true если
/// `haystack` начинается с `needle` (без учёта регистра ASCII-букв).
/// Не-ASCII символы сравниваются побайтово (для DOCTYPE-идентификаторов
/// они не встречаются на практике — все они ASCII per spec).
fn ci_starts_with(haystack: &str, needle: &str) -> bool {
    let h = haystack.as_bytes();
    let n = needle.as_bytes();
    if h.len() < n.len() {
        return false;
    }
    h[..n.len()].eq_ignore_ascii_case(n)
}

/// PUBLIC ID, для которых quirks-mode выставляется по exact-match
/// (case-insensitive). Spec §13.2.5.1.
const EXACT_QUIRKS_PUBLIC_IDS: &[&str] = &[
    "-//W3O//DTD W3 HTML Strict 3.0//EN//",
    "-/W3C/DTD HTML 4.0 Transitional/EN",
    "HTML",
];

/// SYSTEM ID, при exact-match которого выставляется quirks-mode.
/// Spec §13.2.5.1.
const EXACT_QUIRKS_SYSTEM_ID: &str = "http://www.ibm.com/data/dtd/v11/ibmxhtml1-transitional.dtd";

/// PUBLIC ID prefixes (case-insensitive), при которых выставляется
/// quirks-mode безусловно. Spec §13.2.5.1. Это все исторические
/// DTD-идентификаторы HTML 2.0 / 3.x / 4.0 не-Strict, Netscape /
/// Microsoft / прочие browser-specific DTD.
const QUIRKS_PUBLIC_ID_PREFIXES: &[&str] = &[
    "+//Silmaril//dtd html Pro v0r11 19970101//",
    "-//AS//DTD HTML 3.0 asWedit + extensions//",
    "-//AdvaSoft Ltd//DTD HTML 3.0 asWedit + extensions//",
    "-//IETF//DTD HTML 2.0 Level 1//",
    "-//IETF//DTD HTML 2.0 Level 2//",
    "-//IETF//DTD HTML 2.0 Strict Level 1//",
    "-//IETF//DTD HTML 2.0 Strict Level 2//",
    "-//IETF//DTD HTML 2.0 Strict//",
    "-//IETF//DTD HTML 2.0//",
    "-//IETF//DTD HTML 2.1E//",
    "-//IETF//DTD HTML 3.0//",
    "-//IETF//DTD HTML 3.2 Final//",
    "-//IETF//DTD HTML 3.2//",
    "-//IETF//DTD HTML 3//",
    "-//IETF//DTD HTML Level 0//",
    "-//IETF//DTD HTML Level 1//",
    "-//IETF//DTD HTML Level 2//",
    "-//IETF//DTD HTML Level 3//",
    "-//IETF//DTD HTML Strict Level 0//",
    "-//IETF//DTD HTML Strict Level 1//",
    "-//IETF//DTD HTML Strict Level 2//",
    "-//IETF//DTD HTML Strict Level 3//",
    "-//IETF//DTD HTML Strict//",
    "-//IETF//DTD HTML//",
    "-//Metrius//DTD Metrius Presentational//",
    "-//Microsoft//DTD Internet Explorer 2.0 HTML Strict//",
    "-//Microsoft//DTD Internet Explorer 2.0 HTML//",
    "-//Microsoft//DTD Internet Explorer 2.0 Tables//",
    "-//Microsoft//DTD Internet Explorer 3.0 HTML Strict//",
    "-//Microsoft//DTD Internet Explorer 3.0 HTML//",
    "-//Microsoft//DTD Internet Explorer 3.0 Tables//",
    "-//Netscape Comm. Corp.//DTD HTML//",
    "-//Netscape Comm. Corp.//DTD Strict HTML//",
    "-//O'Reilly and Associates//DTD HTML 2.0//",
    "-//O'Reilly and Associates//DTD HTML Extended 1.0//",
    "-//O'Reilly and Associates//DTD HTML Extended Relaxed 1.0//",
    "-//SQ//DTD HTML 2.0 HoTMetaL + extensions//",
    "-//SoftQuad Software//DTD HoTMetaL PRO 6.0::19990601::extensions to HTML 4.0//",
    "-//SoftQuad//DTD HoTMetaL PRO 4.0::19971010::extensions to HTML 4.0//",
    "-//Spyglass//DTD HTML 2.0 Extended//",
    "-//Sun Microsystems Corp.//DTD HotJava HTML//",
    "-//Sun Microsystems Corp.//DTD HotJava Strict HTML//",
    "-//W3C//DTD HTML 3 1995-03-24//",
    "-//W3C//DTD HTML 3.2 Draft//",
    "-//W3C//DTD HTML 3.2 Final//",
    "-//W3C//DTD HTML 3.2//",
    "-//W3C//DTD HTML 3.2S Draft//",
    "-//W3C//DTD HTML 4.0 Frameset//",
    "-//W3C//DTD HTML 4.0 Transitional//",
    "-//W3C//DTD HTML Experimental 19960712//",
    "-//W3C//DTD HTML Experimental 970421//",
    "-//W3C//DTD W3 HTML//",
    "-//W3O//DTD W3 HTML 3.0//",
    "-//WebTechs//DTD Mozilla HTML 2.0//",
    "-//WebTechs//DTD Mozilla HTML//",
];

/// HTML 4.01 Frameset/Transitional — двойственные: с system_id это
/// limited-quirks, без system_id — quirks. Spec §13.2.5.1.
const FRAMESET_TRANSITIONAL_PREFIXES: &[&str] = &[
    "-//W3C//DTD HTML 4.01 Frameset//",
    "-//W3C//DTD HTML 4.01 Transitional//",
];

/// XHTML 1.0 Frameset/Transitional — limited-quirks безусловно
/// (наличие system_id не важно). Spec §13.2.5.1.
const XHTML_FRAMESET_PREFIXES: &[&str] = &[
    "-//W3C//DTD XHTML 1.0 Frameset//",
    "-//W3C//DTD XHTML 1.0 Transitional//",
];

#[cfg(test)]
mod tests {
    use super::*;

    // ──────── No-quirks (standards) cases ────────

    #[test]
    fn html5_doctype_is_no_quirks() {
        // `<!DOCTYPE html>` — нет PUBLIC/SYSTEM.
        assert_eq!(
            detect_document_mode("html", None, None),
            DocumentMode::NoQuirks
        );
    }

    #[test]
    fn html4_strict_with_system_is_no_quirks() {
        // HTML 4.01 Strict — single PUBLIC ID, не входит в quirks-набор.
        assert_eq!(
            detect_document_mode(
                "html",
                Some("-//W3C//DTD HTML 4.01//EN"),
                Some("http://www.w3.org/TR/html4/strict.dtd")
            ),
            DocumentMode::NoQuirks
        );
    }

    #[test]
    fn xhtml_strict_is_no_quirks() {
        assert_eq!(
            detect_document_mode(
                "html",
                Some("-//W3C//DTD XHTML 1.0 Strict//EN"),
                Some("http://www.w3.org/TR/xhtml1/DTD/xhtml1-strict.dtd")
            ),
            DocumentMode::NoQuirks
        );
    }

    #[test]
    fn xhtml_11_is_no_quirks() {
        assert_eq!(
            detect_document_mode(
                "html",
                Some("-//W3C//DTD XHTML 1.1//EN"),
                Some("http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd")
            ),
            DocumentMode::NoQuirks
        );
    }

    #[test]
    fn html5_legacy_compat_is_no_quirks() {
        // `<!DOCTYPE html SYSTEM "about:legacy-compat">` — это валидный
        // HTML5 опциональный вариант, no-quirks (system_id не quirks).
        assert_eq!(
            detect_document_mode("html", None, Some("about:legacy-compat")),
            DocumentMode::NoQuirks
        );
    }

    // ──────── Quirks cases ────────

    #[test]
    fn no_doctype_is_handled_by_caller() {
        // detect_document_mode зовётся только когда DOCTYPE есть.
        // Полное отсутствие DOCTYPE-токена — обрабатывается в tree_builder
        // (set_mode(Quirks)). Но если кто-то вызовет с пустым name, мы
        // тоже даём Quirks — name != "html".
        assert_eq!(
            detect_document_mode("", None, None),
            DocumentMode::Quirks
        );
    }

    #[test]
    fn wrong_name_is_quirks() {
        // DOCTYPE с именем не "html" → quirks.
        assert_eq!(
            detect_document_mode("svg", None, None),
            DocumentMode::Quirks
        );
    }

    #[test]
    fn name_case_insensitive() {
        assert_eq!(
            detect_document_mode("HTML", None, None),
            DocumentMode::NoQuirks
        );
        assert_eq!(
            detect_document_mode("Html", None, None),
            DocumentMode::NoQuirks
        );
    }

    #[test]
    fn exact_quirks_public_id_html() {
        // Голое `<!DOCTYPE html PUBLIC "HTML">` (без system_id) — quirks.
        assert_eq!(
            detect_document_mode("html", Some("HTML"), None),
            DocumentMode::Quirks
        );
    }

    #[test]
    fn exact_quirks_public_id_case_insensitive() {
        assert_eq!(
            detect_document_mode("html", Some("html"), None),
            DocumentMode::Quirks
        );
    }

    #[test]
    fn exact_quirks_system_id() {
        assert_eq!(
            detect_document_mode(
                "html",
                None,
                Some("http://www.ibm.com/data/dtd/v11/ibmxhtml1-transitional.dtd")
            ),
            DocumentMode::Quirks
        );
    }

    #[test]
    fn html_2_0_is_quirks() {
        assert_eq!(
            detect_document_mode(
                "html",
                Some("-//IETF//DTD HTML 2.0//EN"),
                Some("http://www.w3.org/MarkUp/html-spec/html-spec.dtd")
            ),
            DocumentMode::Quirks
        );
    }

    #[test]
    fn html_3_2_is_quirks() {
        assert_eq!(
            detect_document_mode(
                "html",
                Some("-//W3C//DTD HTML 3.2 Final//EN"),
                None
            ),
            DocumentMode::Quirks
        );
    }

    #[test]
    fn netscape_dtd_is_quirks() {
        assert_eq!(
            detect_document_mode(
                "html",
                Some("-//Netscape Comm. Corp.//DTD HTML//EN"),
                None
            ),
            DocumentMode::Quirks
        );
    }

    #[test]
    fn html_4_0_frameset_is_quirks() {
        // HTML 4.0 (not 4.01) Frameset — всегда quirks (в QUIRKS_PUBLIC_ID_PREFIXES).
        assert_eq!(
            detect_document_mode(
                "html",
                Some("-//W3C//DTD HTML 4.0 Frameset//EN"),
                Some("http://www.w3.org/TR/REC-html40/frameset.dtd")
            ),
            DocumentMode::Quirks
        );
    }

    #[test]
    fn html_4_01_frameset_without_system_is_quirks() {
        // HTML 4.01 Frameset БЕЗ system_id → quirks.
        assert_eq!(
            detect_document_mode(
                "html",
                Some("-//W3C//DTD HTML 4.01 Frameset//EN"),
                None
            ),
            DocumentMode::Quirks
        );
    }

    #[test]
    fn html_4_01_transitional_without_system_is_quirks() {
        assert_eq!(
            detect_document_mode(
                "html",
                Some("-//W3C//DTD HTML 4.01 Transitional//EN"),
                None
            ),
            DocumentMode::Quirks
        );
    }

    #[test]
    fn prefix_match_case_insensitive() {
        // Lowercase variant `-//IETF//dtd html 2.0//en`.
        assert_eq!(
            detect_document_mode(
                "html",
                Some("-//ietf//dtd html 2.0//en"),
                None
            ),
            DocumentMode::Quirks
        );
    }

    // ──────── Limited-quirks cases ────────

    #[test]
    fn html_4_01_frameset_with_system_is_limited_quirks() {
        // С system_id (даже пустым) → limited-quirks.
        assert_eq!(
            detect_document_mode(
                "html",
                Some("-//W3C//DTD HTML 4.01 Frameset//EN"),
                Some("http://www.w3.org/TR/html4/frameset.dtd")
            ),
            DocumentMode::LimitedQuirks
        );
    }

    #[test]
    fn html_4_01_transitional_with_system_is_limited_quirks() {
        assert_eq!(
            detect_document_mode(
                "html",
                Some("-//W3C//DTD HTML 4.01 Transitional//EN"),
                Some("http://www.w3.org/TR/html4/loose.dtd")
            ),
            DocumentMode::LimitedQuirks
        );
    }

    #[test]
    fn html_4_01_frameset_with_empty_system_is_limited_quirks() {
        // system_id="" (empty string) — присутствует, не None. Это
        // ключевой кейс, для которого Token хранит Option<String>.
        assert_eq!(
            detect_document_mode(
                "html",
                Some("-//W3C//DTD HTML 4.01 Frameset//EN"),
                Some("")
            ),
            DocumentMode::LimitedQuirks
        );
    }

    #[test]
    fn xhtml_1_0_frameset_is_limited_quirks() {
        // XHTML 1.0 Frameset/Transitional — limited-quirks независимо
        // от system_id.
        assert_eq!(
            detect_document_mode(
                "html",
                Some("-//W3C//DTD XHTML 1.0 Frameset//EN"),
                Some("http://www.w3.org/TR/xhtml1/DTD/xhtml1-frameset.dtd")
            ),
            DocumentMode::LimitedQuirks
        );
    }

    #[test]
    fn xhtml_1_0_transitional_no_system_is_limited_quirks() {
        // Без system_id — всё равно limited-quirks (XHTML отличается от
        // HTML 4.01 этим правилом).
        assert_eq!(
            detect_document_mode(
                "html",
                Some("-//W3C//DTD XHTML 1.0 Transitional//EN"),
                None
            ),
            DocumentMode::LimitedQuirks
        );
    }

    // ──────── Edge cases ────────

    #[test]
    fn ci_starts_with_basic() {
        assert!(ci_starts_with("-//IETF//DTD HTML 2.0//EN", "-//IETF//"));
        assert!(ci_starts_with("-//ietf//dtd html 2.0//en", "-//IETF//"));
        assert!(!ci_starts_with("short", "longer prefix"));
        assert!(ci_starts_with("equal", "equal"));
        assert!(ci_starts_with("anything", ""));
    }

    #[test]
    fn unknown_public_id_is_no_quirks() {
        // Случайный PUBLIC ID, не входящий в списки — no-quirks.
        assert_eq!(
            detect_document_mode("html", Some("-//Future Corp//DTD HTML 99//EN"), None),
            DocumentMode::NoQuirks
        );
    }
}
