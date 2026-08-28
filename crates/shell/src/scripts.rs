//! Everything about `<script>` elements on the page-load path: collecting them
//! in document order, resolving and fetching their sources, the import map,
//! the parser-insertion log replayed to `MutationObserver`, and the two
//! executors that hand the collected bodies to the JS runtime.
//!
//! Moved out of `main.rs` by the SPLIT track (batch SH-3c); behaviour and
//! signatures are unchanged.

use crate::*;

/// A `<script>` to execute: either an inline body or an external `src`.
///
/// Produced by [`collect_scripts_ordered`] in document order; external entries
/// are resolved + fetched by [`resolve_script_sources`].
pub(crate) enum ScriptSource {
    /// Inline `<script>` body (concatenated text children) plus the id of the
    /// `<script>` element itself, which backs `document.currentScript` while the
    /// body runs (BUG-486).
    Inline(NodeId, String),
    /// External `<script src="...">` вЂ” raw `src` attribute, resolved relative
    /// to the document base, plus the element's id (see [`ScriptSource::Inline`]).
    External(NodeId, String),
}

/// True for `type` values that designate an executable classic script
/// (HTML LS В§2.1.5 "JavaScript MIME type"). An absent/empty `type` is classic.
/// Everything else (`module`, `importmap`, `application/json`,
/// `application/ld+json`, `speculationrules`, templates) is data, not code.
pub(crate) fn is_classic_script_type(t: Option<&str>) -> bool {
    match t {
        None => true,
        Some(t) => {
            let t = t.trim();
            t.is_empty()
                || matches!(
                    t.to_ascii_lowercase().as_str(),
                    "text/javascript"
                        | "application/javascript"
                        | "application/ecmascript"
                        | "application/x-ecmascript"
                        | "application/x-javascript"
                        | "text/ecmascript"
                        | "text/javascript1.0"
                        | "text/javascript1.1"
                        | "text/javascript1.2"
                        | "text/javascript1.3"
                        | "text/javascript1.4"
                        | "text/javascript1.5"
                        | "text/jscript"
                        | "text/livescript"
                        | "text/x-ecmascript"
                        | "text/x-javascript"
                )
        }
    }
}

/// Walk the DOM in document order, classifying `<script>` elements into
/// `classic` and `module` execution lists (HTML LS В§8.1.3.1). Unlike
/// [`collect_inline_scripts`], external `<script src>` are recorded as
/// [`ScriptSource::External`] so the caller can fetch and execute their bodies
/// (BUG-164). `defer`/`async` are not modelled separately вЂ” the shell runs
/// every script synchronously in document order, which matches the eventual
/// classic-then-module execution in [`run_scripts_with_dom`].
pub(crate) fn collect_scripts_ordered(
    doc: &Document,
    id: NodeId,
    classic: &mut Vec<ScriptSource>,
    modules: &mut Vec<ScriptSource>,
) {
    let node = doc.get(id);
    if let NodeData::Element { name, .. } = &node.data
        && name.local == "script"
    {
        let script_type = node.get_attr("type").map(|t| t.trim());
        let is_module = script_type.is_some_and(|t| t.eq_ignore_ascii_case("module"));
        // Only module + classic-JS scripts execute; everything else is data.
        if !is_module && !is_classic_script_type(script_type) {
            return;
        }
        // `nomodule` (HTML LS В§4.12.1): РєР»Р°СЃСЃРёС‡РµСЃРєРёР№ СЃРєСЂРёРїС‚ СЃ СЌС‚РёРј Р°С‚СЂРёР±СѓС‚РѕРј вЂ”
        // Р·Р°РїР°СЃРЅР°СЏ СЃР±РѕСЂРєР° РґР»СЏ РґРІРёР¶РєР° Р‘Р•Р— РјРѕРґСѓР»РµР№, Рё РґРІРёР¶РѕРє СЃ РјРѕРґСѓР»СЏРјРё РѕР±СЏР·Р°РЅ РµС‘
        // РїСЂРѕРїСѓСЃС‚РёС‚СЊ. РџРѕРєР° РЅРµ РїСЂРѕРїСѓСЃРєР°Р»Рё, СЃР°Р№С‚ СЃ РїР°СЂРѕР№ module/nomodule РїРѕР»СѓС‡Р°Р»
        // РѕР±Рµ СЃР±РѕСЂРєРё СЂР°Р·РѕРј: legacy-Р±Р°РЅРґР» Рё СЃРѕРІСЂРµРјРµРЅРЅС‹Р№ РјРѕРЅС‚РёСЂРѕРІР°Р»РёСЃСЊ РІ РѕРґРёРЅ Рё
        // С‚РѕС‚ Р¶Рµ РєРѕСЂРµРЅСЊ Рё РіР°СЃРёР»Рё РґСЂСѓРі РґСЂСѓРіР° (Р¶РёРІРѕР№ РїСЂРёРјРµСЂ вЂ” С„РѕСЂРјР° РІС…РѕРґР°
        // id.tbank.ru, 2026-08-17).
        if !is_module && node.get_attr("nomodule").is_some() {
            return;
        }
        let target = if is_module { modules } else { classic };
        // `src` wins over inline body (HTML LS В§4.12.1 вЂ” inline ignored if set).
        if let Some(src) = node.get_attr("src") {
            let src = src.trim();
            if !src.is_empty() {
                target.push(ScriptSource::External(id, src.to_owned()));
            }
            return;
        }
        let mut text = String::new();
        for &child in &node.children {
            if let NodeData::Text(s) = &doc.get(child).data {
                text.push_str(s);
            }
        }
        if !text.trim().is_empty() {
            target.push(ScriptSource::Inline(id, text));
        }
        return;
    }
    for &child in &node.children {
        collect_scripts_ordered(doc, child, classic, modules);
    }
}

/// РЎРєСЂРёРїС‚, РіРѕС‚РѕРІС‹Р№ Рє РёСЃРїРѕР»РЅРµРЅРёСЋ: С‚РµР»Рѕ РїР»СЋСЃ СЃРѕР±СЃС‚РІРµРЅРЅС‹Р№ Р°РґСЂРµСЃ РІРЅРµС€РЅРµРіРѕ С„Р°Р№Р»Р°.
///
/// РђРґСЂРµСЃ РЅСѓР¶РµРЅ С‚РѕР»СЊРєРѕ РјРѕРґСѓР»СЏРј вЂ” РѕРЅ СЃР»СѓР¶РёС‚ Р±Р°Р·РѕР№ РёС… РѕС‚РЅРѕСЃРёС‚РµР»СЊРЅС‹С… РёРјРїРѕСЂС‚РѕРІ
/// (`./chunk.js` Р±Р°РЅРґР»Р° СЃ CDN РѕР±СЏР·Р°РЅ СЂРµР·РѕР»РІРёС‚СЊСЃСЏ РѕС‚ CDN, Р° РЅРµ РѕС‚ РґРѕРєСѓРјРµРЅС‚Р°).
/// РЈ inline-СЃРєСЂРёРїС‚РѕРІ РµРіРѕ РЅРµС‚.
pub(crate) struct ResolvedScript {
    /// РЈР·РµР» `<script>`, РёР· РєРѕС‚РѕСЂРѕРіРѕ С‚РµР»Рѕ РІР·СЏС‚Рѕ (РґР»СЏ `document.currentScript`).
    pub(crate) node: NodeId,
    /// РСЃС…РѕРґРЅС‹Р№ С‚РµРєСЃС‚ СЃРєСЂРёРїС‚Р°.
    pub(crate) source: String,
    /// РђР±СЃРѕР»СЋС‚РЅС‹Р№ URL РІРЅРµС€РЅРµРіРѕ `<script src>`; `None` Сѓ inline Рё `file://`.
    pub(crate) url: Option<String>,
    /// РСЃС…РѕРґ Р·Р°РіСЂСѓР·РєРё РІРЅРµС€РЅРµРіРѕ С„Р°Р№Р»Р°: `Some(true)` вЂ” С‚РµР»Рѕ РїРѕР»СѓС‡РµРЅРѕ,
    /// `Some(false)` вЂ” РЅРµ РїРѕР»СѓС‡РµРЅРѕ (РІ `source` РїСѓСЃС‚Рѕ, РёСЃРїРѕР»РЅСЏС‚СЊ РЅРµС‡РµРіРѕ),
    /// `None` вЂ” СЃРєСЂРёРїС‚ РёРЅР»Р°Р№РЅРѕРІС‹Р№, РІРЅРµС€РЅРµРіРѕ С„Р°Р№Р»Р° Сѓ РЅРµРіРѕ РЅРµС‚.
    ///
    /// BUG-804: HTML LS В§4.12.1 С‚СЂРµР±СѓРµС‚ РІС‹СЃС‚СЂРµР»РёС‚СЊ `load` РЅР° СЌР»РµРјРµРЅС‚Рµ РїРѕСЃР»Рµ
    /// РёСЃРїРѕР»РЅРµРЅРёСЏ РІРЅРµС€РЅРµРіРѕ СЃРєСЂРёРїС‚Р° Рё `error` вЂ” РµСЃР»Рё С„Р°Р№Р» РЅРµ РїСЂРёС€С‘Р», Рё РґРµР»Р°РµС‚
    /// СЌС‚Рѕ **РЅРµР·Р°РІРёСЃРёРјРѕ РѕС‚ С‚РѕРіРѕ, РєС‚Рѕ РІСЃС‚Р°РІРёР» СЌР»РµРјРµРЅС‚**. РџР°СЂСЃРµСЂРЅС‹Р№ `<script>`
    /// РЅРµ РїСЂРѕС…РѕРґРёС‚ С‡РµСЂРµР· JS-С…СѓРє РІСЃС‚Р°РІРєРё (`_lumen_resource_track` Р·РЅР°РµС‚ С‚РѕР»СЊРєРѕ
    /// РѕР± СЌР»РµРјРµРЅС‚Р°С… РёР· `createElement`), РїРѕСЌС‚РѕРјСѓ РёСЃС…РѕРґ РµРіРѕ Р·Р°РіСЂСѓР·РєРё РёР·РІРµСЃС‚РµРЅ
    /// С‚РѕР»СЊРєРѕ Р·РґРµСЃСЊ вЂ” Рё РїРµСЂРµРґР°С‘С‚СЃСЏ РЅР° JS-СЃС‚РѕСЂРѕРЅСѓ РїСЂСЏРјРѕ РІ С†РёРєР»Рµ РёСЃРїРѕР»РЅРµРЅРёСЏ,
    /// РіРґРµ РїРѕСЂСЏРґРѕРє В«РІС‹РїРѕР»РЅРёР»Рё С‚РµР»Рѕ в†’ РІС‹СЃС‚СЂРµР»РёР»Рё `load`В» РїРѕР»СѓС‡Р°РµС‚СЃСЏ РґР°СЂРѕРј.
    /// `None` РЅРµ РґРёСЃРїР°С‚С‡РёС‚ РЅРёС‡РµРіРѕ: Сѓ РёРЅР»Р°Р№РЅРѕРІРѕРіРѕ СЃРєСЂРёРїС‚Р° В«from an external
    /// fileВ» Р»РѕР¶РЅРѕ, Рё СЃРѕР±С‹С‚РёСЏ РїРѕ СЃРїРµС†РёС„РёРєР°С†РёРё РЅРµС‚ РІРѕРІСЃРµ.
    pub(crate) external_ok: Option<bool>,
}

impl ResolvedScript {
    /// Р’РЅРµС€РЅРёР№ `<script src>`, С‚РµР»Рѕ РєРѕС‚РѕСЂРѕРіРѕ РїРѕР»СѓС‡РёС‚СЊ РЅРµ СѓРґР°Р»РѕСЃСЊ.
    ///
    /// РћСЃС‚Р°С‘С‚СЃСЏ РІ СЃРїРёСЃРєРµ СЂРѕРІРЅРѕ СЂР°РґРё СЃРІРѕРµРіРѕ `error` (BUG-804): С‚РµР»Р° РЅРµС‚, С‚Р°Рє С‡С‚Рѕ
    /// С†РёРєР» РёСЃРїРѕР»РЅРµРЅРёСЏ РµРіРѕ РїСЂРѕРїСѓСЃРєР°РµС‚, РЅРѕ СЌР»РµРјРµРЅС‚ РїРѕ HTML LS В§4.12.1 РѕР±СЏР·Р°РЅ
    /// СЃРѕРѕР±С‰РёС‚СЊ СЃС‚СЂР°РЅРёС†Рµ РѕР± РѕС‚РєР°Р·Рµ. Р—Р°РѕРґРЅРѕ СѓР·РµР» РѕСЃС‚Р°С‘С‚СЃСЏ РіСЂР°РЅРёС†РµР№ РѕС‚СЂРµР·РєР° РІ
    /// [`ParserInsertLog`] вЂ” РЅР°СЃС‚РѕСЏС‰РёР№ РїР°СЂСЃРµСЂ С‚РѕР¶Рµ РІСЃС‚Р°РІРёР» РµРіРѕ РІ РґРµСЂРµРІРѕ.
    fn failed(node: NodeId) -> Self {
        Self { node, source: String::new(), url: None, external_ok: Some(false) }
    }
}

/// Р’С‹СЃС‚СЂРµР»РёС‚СЊ `load`/`error` РЅР° СЌР»РµРјРµРЅС‚Рµ `<script>`, РєРѕС‚РѕСЂС‹Р№ РІСЃС‚Р°РІРёР» РїР°СЂСЃРµСЂ.
///
/// Р”РёСЃРїР°С‚С‡ СЃРёРЅС…СЂРѕРЅРЅС‹Р№, Р° РЅРµ Р·Р°РґР°С‡РµР№: В§4.12.1 СЃС‚СЂРµР»СЏРµС‚ `load` СЃСЂР°Р·Сѓ РїРѕСЃР»Рµ С‚РѕРіРѕ,
/// РєР°Рє С‚РµР»Рѕ РѕС‚СЂР°Р±РѕС‚Р°Р»Рѕ, С‚Рѕ РµСЃС‚СЊ Р”Рћ СЃР»РµРґСѓСЋС‰РµРіРѕ СЃРєСЂРёРїС‚Р° РґРѕРєСѓРјРµРЅС‚Р° вЂ” СЃС‚СЂР°РЅРёС†Р°,
/// РєРѕС‚РѕСЂР°СЏ РІ СЃР»РµРґСѓСЋС‰РµРј Р¶Рµ `<script>` С‡РёС‚Р°РµС‚ РІС‹СЃС‚Р°РІР»РµРЅРЅС‹Р№ РѕР±СЂР°Р±РѕС‚С‡РёРєРѕРј С„Р»Р°Рі,
/// РѕР±СЏР·Р°РЅР° РµРіРѕ СѓРІРёРґРµС‚СЊ. РћС‚Р»РѕР¶РёС‚СЊ СЃРѕР±С‹С‚РёРµ Р·Р°РґР°С‡РµР№ Р·РЅР°С‡РёР»Рѕ Р±С‹ РґРѕСЃС‚Р°РІРёС‚СЊ РµРіРѕ
/// РїРѕСЃР»Рµ РІСЃРµРіРѕ СЂР°Р·Р±РѕСЂР°, С‡С‚Рѕ Р»РѕРјР°РµС‚ СЌС‚РѕС‚ РїРѕСЂСЏРґРѕРє.
#[cfg(feature = "v8")]
pub(crate) fn fire_parser_script_event(
    rt: &lumen_js::v8_runtime::V8JsRuntime,
    node: NodeId,
    external_ok: Option<bool>,
) {
    use lumen_core::ext::JsRuntime as _;
    let Some(ok) = external_ok else { return };
    let kind = if ok { "load" } else { "error" };
    let _ = rt.eval(&format!("_lumen_resource_fire({}, '{kind}');", node.index()));
}

/// Р–СѓСЂРЅР°Р» РІСЃС‚Р°РІРѕРє, РєРѕС‚РѕСЂС‹Рµ СЃРґРµР»Р°Р» РїР°СЂСЃРµСЂ, вЂ” РґР»СЏ `MutationObserver` (BUG-827).
///
/// РЁРµР»Р» СЂР°Р·Р±РёСЂР°РµС‚ РґРѕРєСѓРјРµРЅС‚ С†РµР»РёРєРѕРј Рё С‚РѕР»СЊРєРѕ РїРѕС‚РѕРј РёСЃРїРѕР»РЅСЏРµС‚ СЃРєСЂРёРїС‚С‹, РїРѕСЌС‚РѕРјСѓ Рє
/// РјРѕРјРµРЅС‚Сѓ, РєРѕРіРґР° СЃС‚СЂР°РЅРёС‡РЅС‹Р№ `new MutationObserver(вЂ¦).observe(вЂ¦)` РІРѕРѕР±С‰Рµ РјРѕР¶РµС‚
/// Р±С‹С‚СЊ РІС‹РїРѕР»РЅРµРЅ, РґРµСЂРµРІРѕ СѓР¶Рµ РїРѕСЃС‚СЂРѕРµРЅРѕ Рё В«РІСЃС‚Р°РІР»СЏС‚СЊВ» РЅРµС‡РµРіРѕ вЂ” Р·Р°РїРёСЃРµР№ Рѕ
/// РїР°СЂСЃРµСЂРЅС‹С… СѓР·Р»Р°С… РЅРµ РІРѕР·РЅРёРєР°Р»Рѕ РЅРё РѕРґРЅРѕР№, С…РѕС‚СЏ DOM В§4.3 РІРµС€Р°РµС‚ РїРѕСЃС‚Р°РЅРѕРІРєСѓ
/// Р·Р°РїРёСЃРё РЅР° СЃР°Рј С€Р°Рі В«insert a nodeВ», Р° РЅРµ РЅР° РєРѕРЅРєСЂРµС‚РЅС‹Р№ API: СѓР·РµР», РЅР°РїРёСЃР°РЅРЅС‹Р№
/// РїР°СЂСЃРµСЂРѕРј, РѕР±СЏР·Р°РЅ РґР°С‚СЊ `childList`-Р·Р°РїРёСЃСЊ СЂРѕРІРЅРѕ С‚Р°Рє Р¶Рµ, РєР°Рє `appendChild`.
///
/// Р–СѓСЂРЅР°Р» РІРѕСЃСЃС‚Р°РЅР°РІР»РёРІР°РµС‚ С‚РѕС‚ РїРѕСЂСЏРґРѕРє, РІ РєРѕС‚РѕСЂРѕРј РїРѕС‚РѕРєРѕРІС‹Р№ РїР°СЂСЃРµСЂ РІСЃС‚Р°РІР»СЏР» Р±С‹
/// СѓР·Р»С‹ (РѕР±С…РѕРґ РґРµСЂРµРІР° РІ document order), Рё СЂРµР¶РµС‚ РµРіРѕ РіСЂР°РЅРёС†Р°РјРё РёСЃРїРѕР»РЅСЏРµРјС‹С…
/// РєР»Р°СЃСЃРёС‡РµСЃРєРёС… `<script>`: РїРµСЂРµРґ СЃРєСЂРёРїС‚РѕРј K РЅР° JS-СЃС‚РѕСЂРѕРЅСѓ СѓС…РѕРґРёС‚ РІСЃС‘, С‡С‚Рѕ
/// РЅР°СЃС‚РѕСЏС‰РёР№ РїР°СЂСЃРµСЂ РІСЃС‚Р°РІРёР» Р±С‹ РґРѕ РЅРµРіРѕ, РІРєР»СЋС‡Р°СЏ СЃР°Рј СЌР»РµРјРµРЅС‚ `<script>` Рё РµРіРѕ
/// С‚РµРєСЃС‚. РћСЃС‚Р°С‚РѕРє РґРѕРєСѓРјРµРЅС‚Р° СѓС…РѕРґРёС‚ РїРѕСЃР»Рµ РїРѕСЃР»РµРґРЅРµРіРѕ РєР»Р°СЃСЃРёС‡РµСЃРєРѕРіРѕ СЃРєСЂРёРїС‚Р° вЂ”
/// РѕС‚Р»РѕР¶РµРЅРЅС‹Рµ РјРѕРґСѓР»Рё РїРѕ HTML LS В§8.1.3.1 РёСЃРїРѕР»РЅСЏСЋС‚СЃСЏ СѓР¶Рµ РїРѕСЃР»Рµ СЂР°Р·Р±РѕСЂР°.
pub(crate) struct ParserInsertLog {
    /// `(СЂРѕРґРёС‚РµР»СЊ, РІСЃС‚Р°РІР»РµРЅРЅС‹Р№ СЂРµР±С‘РЅРѕРє)` РІ РїРѕСЂСЏРґРєРµ РґРµСЂРµРІР°.
    pub(crate) pairs: Vec<(usize, usize)>,
    /// Р”Р»СЏ РєР°Р¶РґРѕРіРѕ РёСЃРїРѕР»РЅСЏРµРјРѕРіРѕ `<script>` вЂ” РєРѕРЅРµС† РµРіРѕ РїРѕРґРґРµСЂРµРІР° РІ `pairs`.
    pub(crate) script_end: HashMap<NodeId, usize>,
    /// РЎРєРѕР»СЊРєРѕ РїР°СЂ СѓР¶Рµ РѕС‚РґР°РЅРѕ (РёР»Рё РїСЂРѕРїСѓС‰РµРЅРѕ) вЂ” РіСЂР°РЅРёС†Р° СЃР»РµРґСѓСЋС‰РµРіРѕ РѕС‚СЂРµР·РєР°.
    pub(crate) cursor: usize,
}

impl ParserInsertLog {
    /// РћР±РѕР№С‚Рё РґРµСЂРµРІРѕ `doc` Рё Р·Р°РїРѕРјРЅРёС‚СЊ РіСЂР°РЅРёС†С‹ РїРѕРґРґРµСЂРµРІСЊРµРІ СѓР·Р»РѕРІ `scripts`.
    pub(crate) fn build(doc: &Document, scripts: &[ResolvedScript]) -> Self {
        let mut log = Self { pairs: Vec::new(), script_end: HashMap::new(), cursor: 0 };
        // Р‘РµР· РєР»Р°СЃСЃРёС‡РµСЃРєРёС… СЃРєСЂРёРїС‚РѕРІ РЅР°Р±Р»СЋРґР°С‚РµР»СЏ СЃС‚Р°РІРёС‚СЊ РЅРµРєРѕРјСѓ: РјРѕРґСѓР»Рё РїРѕ
        // В§8.1.3.1 РѕС‚Р»РѕР¶РµРЅС‹ Рё РёСЃРїРѕР»РЅСЏСЋС‚СЃСЏ, РєРѕРіРґР° РїР°СЂСЃРµСЂ СѓР¶Рµ РІСЃС‘ РІСЃС‚Р°РІРёР».
        if scripts.is_empty() {
            return log;
        }
        let boundaries: std::collections::HashSet<NodeId> =
            scripts.iter().map(|s| s.node).collect();
        // РЎР°Рј РєРѕСЂРµРЅСЊ РґРѕРєСѓРјРµРЅС‚Р° РЅРёРѕС‚РєСѓРґР° РЅРµ РІСЃС‚Р°РІР»СЏРµС‚СЃСЏ вЂ” РЅР°С‡РёРЅР°РµРј СЃ РµРіРѕ РґРµС‚РµР№.
        log.walk(doc, doc.root(), &boundaries);
        log
    }

    fn walk(&mut self, doc: &Document, id: NodeId, boundaries: &std::collections::HashSet<NodeId>) {
        for &child in &doc.get(id).children {
            self.pairs.push((id.index(), child.index()));
            self.walk(doc, child, boundaries);
            if boundaries.contains(&child) {
                self.script_end.insert(child, self.pairs.len());
            }
        }
    }

    /// Р“СЂР°РЅРёС†Р° РѕС‚СЂРµР·РєР°: РєРѕРЅРµС† РїРѕРґРґРµСЂРµРІР° `upto` Р»РёР±Рѕ РІРµСЃСЊ РѕСЃС‚Р°С‚РѕРє РїСЂРё `None`.
    pub(crate) fn segment_end(&self, upto: Option<NodeId>) -> usize {
        match upto {
            Some(n) => self.script_end.get(&n).copied().unwrap_or(self.pairs.len()),
            None => self.pairs.len(),
        }
    }
}

/// РћС‚РґР°С‚СЊ JS-СЃС‚РѕСЂРѕРЅРµ РїР°СЂСЃРµСЂРЅС‹Рµ РІСЃС‚Р°РІРєРё РІРїР»РѕС‚СЊ РґРѕ `upto` (СЃРј. [`ParserInsertLog`]).
///
/// РќР°Р±Р»СЋРґР°С‚РµР»РµР№ РЅРµС‚ вЂ” СЃС‚СЂРѕРєСѓ РЅРµ СЃС‚СЂРѕРёРј РІРѕРІСЃРµ: Р·Р°РїРёСЃСЊ, РїРѕСЃС‚Р°РІР»РµРЅРЅР°СЏ РґРѕ
/// `observe()`, РІСЃС‘ СЂР°РІРЅРѕ РЅРёРєРѕРјСѓ РЅРµ РґРѕСЃС‚Р°РІР»СЏРµС‚СЃСЏ, Р° СЃРµСЂРёР°Р»РёР·Р°С†РёСЏ РІСЃС‚Р°РІРѕРє С†РµР»РѕРіРѕ
/// РґРѕРєСѓРјРµРЅС‚Р° РЅРµ Р±РµСЃРїР»Р°С‚РЅР°. РљСѓСЂСЃРѕСЂ РґРІРёРіР°РµС‚СЃСЏ РІ РѕР±РѕРёС… СЃР»СѓС‡Р°СЏС….
#[cfg(feature = "v8")]
pub(crate) fn flush_parser_inserts(
    log: &mut ParserInsertLog,
    upto: Option<NodeId>,
    rt: &lumen_js::v8_runtime::V8JsRuntime,
) {
    use lumen_core::ext::JsRuntime as _;
    use std::fmt::Write as _;

    let end = log.segment_end(upto);
    if end <= log.cursor {
        return;
    }
    let observing = matches!(
        rt.eval("_lumen_mo_observing()"),
        Ok(lumen_core::ext::JsValue::Bool(true))
    );
    if observing {
        let mut js = String::with_capacity((end - log.cursor) * 12 + 32);
        js.push_str("_lumen_mo_parser_inserted([");
        for (i, (parent, child)) in log.pairs[log.cursor..end].iter().enumerate() {
            if i > 0 {
                js.push(',');
            }
            let _ = write!(js, "{parent},{child}");
        }
        js.push_str("]);");
        if let Err(e) = rt.eval(&js) {
            eprintln!("MutationObserver: РїР°СЂСЃРµСЂРЅС‹Рµ РІСЃС‚Р°РІРєРё РЅРµ РґРѕСЃС‚Р°РІР»РµРЅС‹: {e}");
        }
    }
    log.cursor = end;
}

/// Resolve [`ScriptSource`] items to JS source strings in document order,
/// fetching external `<script src>` bodies via the subresource fetcher
/// (mirrors [`load_linked_stylesheets`]). A failed fetch is logged and kept in
/// the list with an empty body and `external_ok: Some(false)` вЂ” one broken
/// script must not abort the rest of the page, but it still owes its element an
/// `error` event (BUG-804), so it may not be dropped here.
pub(crate) fn resolve_script_sources(
    items: &[ScriptSource],
    base: &ResourceBase,
    sink: &Arc<dyn EventSink>,
    cookie_jar: Option<Arc<lumen_storage::CookieJar>>,
) -> Vec<ResolvedScript> {
    // Р’РЅРµС€РЅРёРµ `<script src>` РіСЂСѓР·СЏС‚СЃСЏ РїР°СЂР°Р»Р»РµР»СЊРЅРѕ (СЃРµС‚СЊ вЂ” РіР»Р°РІРЅС‹Р№ С‚РѕСЂРјРѕР·), РЅРѕ
    // СЂРµР·СѓР»СЊС‚Р°С‚ СЃРѕР±РёСЂР°РµС‚СЃСЏ СЃС‚СЂРѕРіРѕ РІ РёСЃС…РѕРґРЅРѕРј РїРѕСЂСЏРґРєРµ: РєР»Р°СЃСЃРёС‡РµСЃРєРёРµ СЃРєСЂРёРїС‚С‹
    // РѕР±СЏР·Р°РЅС‹ РІС‹РїРѕР»РЅСЏС‚СЊСЃСЏ РІ РїРѕСЂСЏРґРєРµ РґРѕРєСѓРјРµРЅС‚Р° (HTML LS В§8.1.3.1). Inline-С‚РµР»Р°
    // РїСЂРѕС…РѕРґСЏС‚ РЅР°СЃРєРІРѕР·СЊ Р±РµР· СЃРµС‚Рё.
    let fetched = parallel_map(items, |_, item| match item {
        ScriptSource::Inline(nid, body) => Some(ResolvedScript {
            node: *nid,
            source: body.clone(),
            url: None,
            external_ok: None,
        }),
        ScriptSource::External(nid, src) => match base.resolve(src) {
            ResolvedResource::File(path) => match std::fs::read_to_string(&path) {
                Ok(content) => {
                    eprintln!("Р—Р°РіСЂСѓР¶РµРЅ СЃРєСЂРёРїС‚: {}", path.display());
                    Some(ResolvedScript {
                        node: *nid,
                        source: content,
                        url: None,
                        external_ok: Some(true),
                    })
                }
                Err(e) => {
                    eprintln!("РџСЂРѕРїСѓСЃРє СЃРєСЂРёРїС‚Р° {}: {e}", path.display());
                    Some(ResolvedScript::failed(*nid))
                }
            },
            ResolvedResource::Url(url) => {
                use lumen_core::url::Url;
                use lumen_network::RequestDestination;
                let sub_url = match Url::parse(&url) {
                    Ok(u) => u,
                    Err(e) => {
                        eprintln!("РџСЂРѕРїСѓСЃРє СЃРєСЂРёРїС‚Р° {url}: {e}");
                        return Some(ResolvedScript::failed(*nid));
                    }
                };
                // BUG-171: read through the prefetch cache so a script already
                // warmed by the streaming thread returns instantly instead of
                // blocking the UI thread on the socket. On a miss this fetches the
                // exact same bytes via the same client (script order preserved).
                // PERF-1: one span per external script fetch.
                let mut fetch_span = lumen_core::trace::span(format!("script {url}"), "net");
                let bytes = crate::prefetch::PREFETCH_CACHE.fetch_current(&url, || {
                    let client = base.http_client_for_subresource(sink.clone(), cookie_jar.clone());
                    client
                        .fetch_subresource(&sub_url, RequestDestination::Script)
                        .map_err(|e| e.to_string())
                });
                match bytes {
                    Ok(bytes) => {
                        eprintln!("Р—Р°РіСЂСѓР¶РµРЅ СЃРєСЂРёРїС‚: {url}");
                        fetch_span.set_bytes(bytes.len());
                        Some(ResolvedScript {
                            node: *nid,
                            source: String::from_utf8_lossy(&bytes[..]).into_owned(),
                            // РђР±СЃРѕР»СЋС‚РЅС‹Р№ Р°РґСЂРµСЃ СЃР°РјРѕРіРѕ СЃРєСЂРёРїС‚Р° вЂ” Р±Р°Р·Р°
                            // РѕС‚РЅРѕСЃРёС‚РµР»СЊРЅС‹С… РёРјРїРѕСЂС‚РѕРІ РІРЅСѓС‚СЂРё РјРѕРґСѓР»СЏ.
                            url: Some(url.clone()),
                            external_ok: Some(true),
                        })
                    }
                    Err(e) => {
                        eprintln!("РџСЂРѕРїСѓСЃРє СЃРєСЂРёРїС‚Р° {url}: {e}");
                        Some(ResolvedScript::failed(*nid))
                    }
                }
            }
        },
    });
    fetched.into_iter().flatten().collect()
}

/// Collect `<script>` elements from the DOM, separating classic from module scripts.
///
/// `scripts` receives classic `<script>` bodies (no `type` attribute, or `type=text/javascript`).
/// `module_scripts` receives `<script type=module>` bodies (HTML LS В§8.1.3.1).
/// Both skip `<script src="...">` (external-only) and empty inline bodies.
pub(crate) fn collect_inline_scripts(
    doc: &Document,
    id: NodeId,
    scripts: &mut Vec<String>,
    module_scripts: &mut Vec<String>,
) {
    let node = doc.get(id);
    if let NodeData::Element { name, .. } = &node.data
        && name.local == "script"
    {
        let script_type = node.get_attr("type").map(|t| t.trim());
        let is_module = script_type.is_some_and(|t| t.eq_ignore_ascii_case("module"));
        let is_importmap = script_type.is_some_and(|t| t.eq_ignore_ascii_case("importmap"));
        // РўРѕС‚ Р¶Рµ РїСЂРѕРїСѓСЃРє `nomodule`, С‡С‚Рѕ Рё РІ `collect_scripts_ordered`.
        if !is_module && !is_importmap && node.get_attr("nomodule").is_some() {
            return;
        }

        let mut text = String::new();
        for &child in &node.children {
            if let NodeData::Text(s) = &doc.get(child).data {
                text.push_str(s);
            }
        }
        if !text.trim().is_empty() {
            if is_importmap {
                // Import maps are handled separately by the caller
                // For now, skip them here; caller will collect them separately
            } else if is_module {
                module_scripts.push(text);
            } else {
                scripts.push(text);
            }
        }
        return;
    }
    for &child in &node.children {
        collect_inline_scripts(doc, child, scripts, module_scripts);
    }
}

/// Collect the first `<script type="importmap">` import map from the document.
///
/// Returns the parsed ImportMap if found, or None if not present or invalid JSON.
#[cfg(feature = "v8")]
pub(crate) fn collect_import_map(doc: &Document) -> Option<lumen_js::esm::ImportMap> {
    collect_import_map_impl(doc, doc.root())
}

#[cfg(feature = "v8")]
fn collect_import_map_impl(
    doc: &Document,
    id: NodeId,
) -> Option<lumen_js::esm::ImportMap> {
    let node = doc.get(id);
    if let NodeData::Element { name, .. } = &node.data
        && name.local == "script"
    {
        let script_type = node.get_attr("type").map(|t| t.trim());
        let is_importmap = script_type.is_some_and(|t| t.eq_ignore_ascii_case("importmap"));

        if is_importmap {
            let mut text = String::new();
            for &child in &node.children {
                if let NodeData::Text(s) = &doc.get(child).data {
                    text.push_str(s);
                }
            }
            if let Some(map) = lumen_js::esm::ImportMap::parse(&text) {
                return Some(map);
            }
        }
    }
    for &child in &node.children {
        if let Some(map) = collect_import_map_impl(doc, child) {
            return Some(map);
        }
    }
    None
}

/// Р’С‹РїРѕР»РЅРёС‚СЊ inline `<script>` Р±Р»РѕРєРё СЃ DOM-РґРѕСЃС‚СѓРїРѕРј (V8 + install_dom).
///
/// РџСЂРёРЅРёРјР°РµС‚ `doc` РїРѕ Р·РЅР°С‡РµРЅРёСЋ, РѕР±РѕСЂР°С‡РёРІР°РµС‚ РІ `Arc<Mutex<>>` РЅР° РІСЂРµРјСЏ РІС‹РїРѕР»РЅРµРЅРёСЏ
/// Р’С‹РїРѕР»РЅСЏРµС‚ inline `<script>` Р±Р»РѕРєРё С‡РµСЂРµР· V8 (РµСЃР»Рё feature РІРєР»СЋС‡С‘РЅ),
/// РІРѕР·РІСЂР°С‰Р°РµС‚ `(Arc<Mutex<Document>>, Option<JsNavigateRequest>, Option<Arc<dyn PersistentJs>>)`.
///
/// Р”РѕРєСѓРјРµРЅС‚ РѕР±РѕСЂР°С‡РёРІР°РµС‚СЃСЏ РІ `Arc<Mutex>` С‡С‚РѕР±С‹ JS-Р·Р°РјС‹РєР°РЅРёСЏ Рё layout-РєРѕРґ
/// РјРѕРіР»Рё СЂР°Р·РґРµР»РёС‚СЊ РґРѕСЃС‚СѓРї Р±РµР· Р»РёС€РЅРёС… РєР»РѕРЅРѕРІ. Persistent runtime РІРѕР·РІСЂР°С‰Р°РµС‚СЃСЏ
/// РєР°Рє `PersistentJs` РґР»СЏ РґРёСЃРїР°С‚С‡Р° СЃРѕР±С‹С‚РёР№ РїРѕСЃР»Рµ Р·Р°РіСЂСѓР·РєРё СЃС‚СЂР°РЅРёС†С‹.
///
/// `page_url` РїСЂРѕР±СЂР°СЃС‹РІР°РµС‚СЃСЏ РІ `window.location` (РёРЅРёС†РёР°Р»РёР·Р°С†РёСЏ).
/// `fetch_provider` РїСЂРѕР±СЂР°СЃС‹РІР°РµС‚СЃСЏ РІ `window.fetch()`.
/// `ws_provider` РїСЂРѕР±СЂР°СЃС‹РІР°РµС‚СЃСЏ РІ `new WebSocket(url)`.
/// `sse_provider` РїСЂРѕР±СЂР°СЃС‹РІР°РµС‚СЃСЏ РІ `new EventSource(url)`.
/// `ls_store` вЂ” localStorage partition РґР»СЏ С‚РµРєСѓС‰РµРіРѕ origin (persists across reloads).
/// `ss_store` вЂ” sessionStorage partition РІРєР»Р°РґРєРё РґР»СЏ С‚РѕРіРѕ Р¶Рµ origin (BUG-836):
/// Р¶РёРІС‘С‚, РїРѕРєР° Р¶РёРІР° РІРєР»Р°РґРєР°, Рё РїРµСЂРµР¶РёРІР°РµС‚ СЃРјРµРЅСѓ РґРѕРєСѓРјРµРЅС‚Р°.
/// `None` = no network (sandboxed context РёР»Рё РѕС‚РєР»СЋС‡С‘РЅ v8 feature).
/// `scripts` / `module_scripts` вЂ” СѓР¶Рµ СЂР°Р·СЂРµС€С‘РЅРЅС‹Рµ С‚РµР»Р° classic / module СЃРєСЂРёРїС‚РѕРІ
/// РІ РїРѕСЂСЏРґРєРµ РґРѕРєСѓРјРµРЅС‚Р°, РІРєР»СЋС‡Р°СЏ РґРѕР·Р°РіСЂСѓР¶РµРЅРЅС‹Рµ РІРЅРµС€РЅРёРµ `<script src>` (BUG-164);
/// СЃРѕР±РёСЂР°СЋС‚СЃСЏ РІС‹Р·С‹РІР°СЋС‰РёРј С‡РµСЂРµР· [`collect_scripts_ordered`] + [`resolve_script_sources`].
#[allow(clippy::needless_return)] // `return` inside #[cfg] block is needed for correct control flow
#[allow(unused_variables, clippy::type_complexity, clippy::too_many_arguments)]
pub(crate) fn run_scripts_with_dom(
    doc: Document,
    sandbox: lumen_core::SandboxFlags,
    page_url: &str,
    fetch_provider: Option<Arc<dyn lumen_core::ext::JsFetchProvider>>,
    ws_provider: Option<Arc<dyn lumen_core::ext::JsWebSocketProvider>>,
    sse_provider: Option<Arc<dyn lumen_core::ext::JsSseProvider>>,
    ls_store: Option<Arc<Mutex<lumen_core::WebStorage>>>,
    ss_store: Option<Arc<Mutex<lumen_core::WebStorage>>>,
    idb_backend: Option<Arc<dyn lumen_core::ext::IdbBackend>>,
    sw_backend: Option<Arc<dyn lumen_core::ext::SwBackend>>,
    sw_worker_store: Option<lumen_core::ext::SwWorkerStore>,
    // PH3-20: shared Cache API backend. Passed to `install_dom` so the page's
    // `caches` API and any activating SW execution thread read/write the same
    // store (the SW serves cache-first responses the page previously cached).
    cache_backend: Option<Arc<dyn lumen_core::ext::CacheBackend>>,
    cookie_banner_dismiss: bool,
    deterministic: deterministic::DetConfig,
    cross_origin_isolated: bool,
    extra_scripts: &[String],
    scripts: Vec<ResolvedScript>,
    module_scripts: Vec<ResolvedScript>,
    // BUG-480 срез 8: создать рантайм даже при отсутствии парсерных скриптов
    // (фреймы: получатель кросс-фреймовых конвертов). Sandbox=SCRIPTS всё
    // равно побеждает — он запрещает исполнение целиком.
    always_runtime: bool,
) -> (Arc<Mutex<Document>>, Option<JsNavigateRequest>, Option<Arc<dyn PersistentJs>>) {
    // `scripts` / `module_scripts` are already resolved by the caller in
    // document order, including fetched external `<script src>` bodies (BUG-164).
    // Import map must be captured before `doc` moves into the Arc and applied
    // to the runtime before any module evaluation (HTML LS §8.1.6.2).
    #[cfg(feature = "v8")]
    let import_map = collect_import_map(&doc);
    // BUG-827: порядок парсерных вставок надо снять до того, как `doc` уйдет в
    // Arc, — дальше исполнение скриптов уже начнет менять дерево.
    #[cfg(feature = "v8")]
    let mut parser_inserts = ParserInsertLog::build(&doc, &scripts);

    let doc_arc = Arc::new(Mutex::new(doc));

    if !always_runtime && scripts.is_empty() && module_scripts.is_empty() && extra_scripts.is_empty()
    {
        return (doc_arc, None, None);
    }
    if sandbox.contains(lumen_core::SandboxFlags::SCRIPTS) {
        eprintln!(
            "sandbox: Р·Р°Р±Р»РѕРєРёСЂРѕРІР°РЅРѕ {} СЃРєСЂРёРїС‚(РѕРІ) + {} РјРѕРґСѓР»(РµР№) (sandbox=scripts)",
            scripts.len(), module_scripts.len()
        );
        return (doc_arc, None, None);
    }

    // Ph3 V8 migration S4. Since S12b-23 the import map and `eval_module` are
    // wired here too; since S12b-G6 (BUG-548) `set_cookie_banner_dismiss` is
    // wired too.
    #[cfg(feature = "v8")]
    {
        use lumen_core::ext::JsRuntime as _;
        match lumen_js::v8_runtime::V8JsRuntime::new() {
            Ok(mut rt) => {
                rt.set_cookie_banner_dismiss(cookie_banner_dismiss);
                if deterministic.enabled {
                    rt.set_deterministic_mode(true, deterministic.rng_seed, deterministic.monotonic_clock);
                }
                if let Some(store) = sw_worker_store {
                    rt = rt.with_sw_worker_store(store);
                }
                // BUG-836: the tab owns sessionStorage, not the document.
                if let Some(store) = ss_store {
                    rt = rt.with_session_storage(store);
                }
                if let Err(e) = rt.install_dom(Arc::clone(&doc_arc), page_url, fetch_provider, ws_provider, sse_provider, ls_store, idb_backend, sw_backend, cache_backend, None, cross_origin_isolated) {
                    eprintln!("JS DOM init failed: {e}");
                }
                // Must precede module evaluation: bare specifiers resolve
                // through the map (HTML LS В§8.1.6.2).
                if let Some(map) = import_map {
                    rt.set_import_map(map);
                }
                // BUG-839: hand over the subresource loads that already
                // finished, before the page's first script runs. The document's
                // stylesheets and scripts are fetched *during parsing*, i.e.
                // before this runtime exists, and WPT's
                // `performance-timeline/case-sensitivity.any.js` reads
                // `getEntriesByType('resource')` synchronously at the top of
                // that first script вЂ” the shell's once-per-event-loop-step
                // drain is far too late for it. That drain still covers the
                // tail (images, anything started later); this take is
                // unconditional because the suspend flag exists to keep those
                // very rows away from the *outgoing* document, and this caller
                // is the incoming one.
                if let Some(json) = crate::resource_timing::rows_to_json(
                    &crate::resource_timing::take_rows_unconditionally(),
                ) {
                    let _ = rt.eval(&format!(
                        "_lumen_deliver_resource_timings({})",
                        js_string_literal(&json)
                    ));
                }
                // Classic scripts run first (HTML LS В§8.1.3 execution order).
                for ResolvedScript { node: nid, source: src, external_ok, .. } in &scripts {
                    // BUG-827: Рє СЌС‚РѕРјСѓ РјРѕРјРµРЅС‚Сѓ РЅР°СЃС‚РѕСЏС‰РёР№ РїР°СЂСЃРµСЂ СѓР¶Рµ РІСЃС‚Р°РІРёР» РІСЃС‘,
                    // С‡С‚Рѕ СЃС‚РѕРёС‚ РІ РґРѕРєСѓРјРµРЅС‚Рµ РІС‹С€Рµ СЌС‚РѕРіРѕ СЃРєСЂРёРїС‚Р°, Рё СЃР°Рј РµРіРѕ
                    // СЌР»РµРјРµРЅС‚ вЂ” РЅР°Р±Р»СЋРґР°С‚РµР»СЊ, РїРѕСЃС‚Р°РІР»РµРЅРЅС‹Р№ РїСЂРµРґС‹РґСѓС‰РёРј СЃРєСЂРёРїС‚РѕРј,
                    // РѕР±СЏР·Р°РЅ СѓРІРёРґРµС‚СЊ СЌС‚Рё РІСЃС‚Р°РІРєРё Р·Р°РїРёСЃСЏРјРё.
                    flush_parser_inserts(&mut parser_inserts, Some(*nid), &rt);
                    // BUG-804: РІРЅРµС€РЅРёР№ С„Р°Р№Р» РЅРµ РїСЂРёС€С‘Р» вЂ” РёСЃРїРѕР»РЅСЏС‚СЊ РЅРµС‡РµРіРѕ, РЅРѕ
                    // СЌР»РµРјРµРЅС‚ РѕР±СЏР·Р°РЅ СЃРѕРѕР±С‰РёС‚СЊ РѕР± РѕС‚РєР°Р·Рµ РЅР° СЃРІРѕС‘Рј РјРµСЃС‚Рµ РІ
                    // РїРѕСЂСЏРґРєРµ РґРѕРєСѓРјРµРЅС‚Р°.
                    if *external_ok == Some(false) {
                        fire_parser_script_event(&rt, *nid, *external_ok);
                        continue;
                    }
                    // BUG-486: `document.currentScript` must name the element
                    // being executed for the whole body and nothing else, so the
                    // push/pop pair brackets the eval вЂ” including the error paths
                    // below, or one throwing script would leave a stale value
                    // behind for every script after it.
                    let _ = rt.eval(&format!("_lumen_push_current_script({});", nid.index()));
                    // eval_and_report (not the plain trait eval()) вЂ” this is
                    // the genuine top-level page-script execution boundary,
                    // so an uncaught exception must also reach the page's own
                    // window 'error'/onerror listeners (BUG-591), not just
                    // this stderr line.
                    match rt.eval_and_report(src) {
                        Ok(_) => {}
                        Err(lumen_core::JsError::NotImplemented) => {
                            eprintln!(
                                "script: engine=v8, РІС‹РїРѕР»РЅРµРЅРёРµ РїСЂРѕРїСѓС‰РµРЅРѕ ({} Р±Р°Р№С‚)",
                                src.len()
                            );
                        }
                        Err(e) => eprintln!("script error: {e}"),
                    }
                    let _ = rt.eval("_lumen_pop_current_script();");
                    // В§4.12.1 В«execute the script blockВ», РїРѕСЃР»РµРґРЅРёР№ С€Р°Рі:
                    // РІРЅРµС€РЅРёР№ РєР»Р°СЃСЃРёС‡РµСЃРєРёР№ СЃРєСЂРёРїС‚ СЃС‚СЂРµР»СЏРµС‚ `load` СЃСЂР°Р·Сѓ РїРѕСЃР»Рµ
                    // С‚РµР»Р°. РРЅР»Р°Р№РЅРѕРІС‹Р№ вЂ” РЅРёС‡РµРіРѕ (`external_ok` = `None`).
                    fire_parser_script_event(&rt, *nid, *external_ok);
                }
                // BUG-827: С…РІРѕСЃС‚ РґРѕРєСѓРјРµРЅС‚Р° РїР°СЂСЃРµСЂ РІСЃС‚Р°РІРёР» РµС‰С‘ РґРѕ С‚РѕРіРѕ, РєР°Рє
                // РѕС‚Р»РѕР¶РµРЅРЅС‹Рµ РјРѕРґСѓР»Рё РЅР°С‡Р°Р»Рё РёСЃРїРѕР»РЅСЏС‚СЊСЃСЏ, вЂ” РѕС‚РґР°С‘Рј РµРіРѕ РѕРґРЅРёРј
                // РѕС‚СЂРµР·РєРѕРј Р·РґРµСЃСЊ, РїРѕРєР° РЅР°Р±Р»СЋРґР°С‚РµР»СЊ РїРѕСЃР»РµРґРЅРµРіРѕ РєР»Р°СЃСЃРёС‡РµСЃРєРѕРіРѕ
                // СЃРєСЂРёРїС‚Р° РµС‰С‘ РјРѕР¶РµС‚ РµРіРѕ СѓСЃР»С‹С€Р°С‚СЊ.
                flush_parser_inserts(&mut parser_inserts, None, &rt);
                // Module scripts run after classic scripts (HTML LS В§8.1.3.1 deferred).
                // No `currentScript` bracket: it is `null` inside a module by spec.
                for item in &module_scripts {
                    // BUG-804: РІРЅРµС€РЅРёР№ РјРѕРґСѓР»СЊ, С‡РµР№ С„Р°Р№Р» РЅРµ РїСЂРёС€С‘Р», РѕР±СЏР·Р°РЅ
                    // РІС‹СЃС‚СЂРµР»РёС‚СЊ `error` СЂРѕРІРЅРѕ С‚Р°Рє Р¶Рµ, РєР°Рє РєР»Р°СЃСЃРёС‡РµСЃРєРёР№.
                    if item.external_ok == Some(false) {
                        fire_parser_script_event(&rt, item.node, item.external_ok);
                        continue;
                    }
                    let src = &item.source;
                    // Р’РЅРµС€РЅРёР№ РјРѕРґСѓР»СЊ РёСЃРїРѕР»РЅСЏРµС‚СЃСЏ РїРѕРґ РЎР’РћРРњ Р°РґСЂРµСЃРѕРј: РѕС‚ РЅРµРіРѕ
                    // СЃС‡РёС‚Р°СЋС‚СЃСЏ РµРіРѕ РѕС‚РЅРѕСЃРёС‚РµР»СЊРЅС‹Рµ РёРјРїРѕСЂС‚С‹. РЈ inline-РјРѕРґСѓР»СЏ
                    // Р°РґСЂРµСЃР° РЅРµС‚ вЂ” Р±Р°Р·Р° РѕСЃС‚Р°С‘С‚СЃСЏ Р°РґСЂРµСЃРѕРј СЃС‚СЂР°РЅРёС†С‹.
                    // eval_module_at_and_report/eval_module_and_report (not the
                    // plain trait methods) вЂ” this is the top-level page-script
                    // boundary, so a runtime error in the module body must also
                    // reach window 'error'/onerror (BUG-591); a load/link
                    // failure stays unreported here (belongs to the script
                    // element's own 'error' event instead).
                    let outcome = match &item.url {
                        Some(url) => rt.eval_module_at_and_report(url, src),
                        None => rt.eval_module_and_report(src),
                    };
                    match outcome {
                        Ok(()) => {}
                        Err(lumen_core::JsError::NotImplemented) => {
                            eprintln!(
                                "module: engine=v8, РІС‹РїРѕР»РЅРµРЅРёРµ РїСЂРѕРїСѓС‰РµРЅРѕ ({} Р±Р°Р№С‚)",
                                src.len()
                            );
                        }
                        Err(e) => eprintln!("module error: {e}"),
                    }
                    // BUG-804: РІРЅРµС€РЅРёР№ РјРѕРґСѓР»СЊ СЃС‚СЂРµР»СЏРµС‚ `load` РїРѕСЃР»Рµ РІС‹С‡РёСЃР»РµРЅРёСЏ
                    // вЂ” РІРєР»СЋС‡Р°СЏ СЃР»СѓС‡Р°Р№, РєРѕРіРґР° С‚РµР»Рѕ Р±СЂРѕСЃРёР»Рѕ: РёСЃРєР»СЋС‡РµРЅРёРµ СѓС…РѕРґРёС‚ РІ
                    // window `error` (BUG-591), Р° СЌР»РµРјРµРЅС‚ РІСЃС‘ СЂР°РІРЅРѕ СЃРѕРѕР±С‰Р°РµС‚ РѕР±
                    // СѓСЃРїРµС€РЅРѕР№ Р·Р°РіСЂСѓР·РєРµ. РћСЃС‚Р°С‚РѕРє: РїСЂРѕРІР°Р» РЎР’РЇР—Р«Р’РђРќРРЇ (РЅРµ РЅР°С€С‘Р»СЃСЏ
                    // РёРјРїРѕСЂС‚ РІРЅСѓС‚СЂРё) РїРѕ СЃРїРµС†РёС„РёРєР°С†РёРё РґРѕР»Р¶РµРЅ РґР°С‚СЊ `error`, РЅРѕ
                    // `ModuleFailure` РґРѕ СЃСЋРґР° РЅРµ РґРѕС…РѕРґРёС‚ вЂ” `JsResult` РµРіРѕ
                    // СЃС…Р»РѕРїС‹РІР°РµС‚, Рё Р·РґРµСЃСЊ С‚РѕР¶Рµ РІС‹Р№РґРµС‚ `load`.
                    fire_parser_script_event(&rt, item.node, item.external_ok);
                }
                // Extension content scripts run last (after all page scripts).
                for src in extra_scripts {
                    match rt.eval(src) {
                        Ok(_) => {}
                        Err(lumen_core::JsError::NotImplemented) => {
                            eprintln!(
                                "extension: engine=v8, РІС‹РїРѕР»РЅРµРЅРёРµ РїСЂРѕРїСѓС‰РµРЅРѕ ({} Р±Р°Р№С‚)",
                                src.len()
                            );
                        }
                        Err(e) => eprintln!("extension script error: {e}"),
                    }
                }
                let nav_req = rt.take_navigate_request().map(|r| match r {
                    lumen_js::NavigateRequest::Push(u)    => JsNavigateRequest::Push(u),
                    lumen_js::NavigateRequest::Replace(u) => JsNavigateRequest::Replace(u),
                    lumen_js::NavigateRequest::Reload     => JsNavigateRequest::Reload,
                    lumen_js::NavigateRequest::SubmitForm { form, submitter } =>
                        JsNavigateRequest::SubmitForm { form, submitter },
                });
                // Keep rt alive: return as PersistentJs so event handlers work after load.
                let ctx: Arc<dyn PersistentJs> = Arc::new(V8PersistentJs { rt });
                return (doc_arc, nav_req, Some(ctx));
            }
            Err(e) => {
                eprintln!("V8 init failed: {e}");
                return (doc_arc, None, None);
            }
        }
    }

    #[cfg(not(feature = "v8"))]
    {
        let _ = page_url;
        let _ = fetch_provider;
        let _ = ws_provider;
        let _ = sse_provider;
        use lumen_core::ext::JsRuntime as _;
        for (_, src) in &scripts {
            match lumen_core::NullJsRuntime.eval(src) {
                Ok(_) => {}
                Err(lumen_core::JsError::NotImplemented) => {
                    eprintln!(
                        "script: engine=null, РІС‹РїРѕР»РЅРµРЅРёРµ РїСЂРѕРїСѓС‰РµРЅРѕ ({} Р±Р°Р№С‚)",
                        src.len()
                    );
                }
                Err(e) => eprintln!("script error: {e}"),
            }
        }
        (doc_arc, None, None)
    }
}

/// Р’С‹РїРѕР»РЅРёС‚СЊ inline `<script>` Р±Р»РѕРєРё РµСЃР»Рё sandbox РїРѕР·РІРѕР»СЏРµС‚, РёРЅР°С‡Рµ Р·Р°Р±Р»РѕРєРёСЂРѕРІР°С‚СЊ.
///
/// `SandboxFlags::SCRIPTS` СѓСЃС‚Р°РЅРѕРІР»РµРЅ вЂ” СЃРєСЂРёРїС‚С‹ Р·Р°РїСЂРµС‰РµРЅС‹; С„СѓРЅРєС†РёСЏ Р»РѕРіРёСЂСѓРµС‚
/// РєРѕР»РёС‡РµСЃС‚РІРѕ Р·Р°Р±Р»РѕРєРёСЂРѕРІР°РЅРЅС‹С… Рё РІРѕР·РІСЂР°С‰Р°РµС‚ 0. РРЅР°С‡Рµ РєР°Р¶РґС‹Р№ СЃРєСЂРёРїС‚ РїРµСЂРµРґР°С‘С‚СЃСЏ
/// РІ `runtime.eval()`; Р±РµР· feature `v8` СЌС‚Рѕ NullJsRuntime в†’ `NotImplemented`.
/// Р’РѕР·РІСЂР°С‰Р°РµС‚ С‡РёСЃР»Рѕ СЃРєСЂРёРїС‚РѕРІ, РїРµСЂРµРґР°РЅРЅС‹С… РІ runtime.
#[cfg(test)]
pub(crate) fn run_scripts(
    doc: &Document,
    sandbox: lumen_core::SandboxFlags,
    runtime: &dyn lumen_core::JsRuntime,
) -> usize {
    let mut scripts: Vec<String> = Vec::new();
    let mut _module_scripts: Vec<String> = Vec::new();
    collect_inline_scripts(doc, doc.root(), &mut scripts, &mut _module_scripts);
    if scripts.is_empty() {
        return 0;
    }
    if sandbox.contains(lumen_core::SandboxFlags::SCRIPTS) {
        eprintln!(
            "sandbox: Р·Р°Р±Р»РѕРєРёСЂРѕРІР°РЅРѕ {} СЃРєСЂРёРїС‚(РѕРІ) (sandbox=scripts)",
            scripts.len()
        );
        return 0;
    }
    for src in &scripts {
        match runtime.eval(src) {
            Ok(_) => {}
            Err(lumen_core::JsError::NotImplemented) => {
                eprintln!(
                    "script: engine={}, РІС‹РїРѕР»РЅРµРЅРёРµ РїСЂРѕРїСѓС‰РµРЅРѕ ({} Р±Р°Р№С‚)",
                    runtime.engine_name(),
                    src.len()
                );
            }
            Err(e) => {
                eprintln!("script error: {e}");
            }
        }
    }
    scripts.len()
}
