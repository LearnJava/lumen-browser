//! Reading facts back out of a parsed [`Document`]: the page title, its inline
//! `<style>` text and the fingerprint of that text, plus the window-title
//! format the shell puts on the OS window.
//!
//! Moved out of `main.rs` by the SPLIT track (batch SH-3c); behaviour and
//! signatures are unchanged.

use crate::*;

/// РќР°Р№С‚Рё РїРµСЂРІС‹Р№ `<title>` РІ РґРµСЂРµРІРµ Рё СЃРєР»РµРёС‚СЊ РµРіРѕ С‚РµРєСЃС‚РѕРІС‹Рµ РґРµС‚Рё.
///
/// HTML5 СЂР°Р·СЂРµС€Р°РµС‚ С‚РѕР»СЊРєРѕ РѕРґРёРЅ `<title>` РІ `<head>`, РЅРѕ РјС‹ lenient-РїР°СЂСЃРµСЂ вЂ”
/// Р±РµСЂС‘Рј РїРµСЂРІС‹Р№ РІСЃС‚СЂРµС‡РЅС‹Р№. Р­РЅС‚РёС‚Рё СѓР¶Рµ РґРµРєРѕРґРёСЂРѕРІР°РЅС‹ tokenizer-РѕРј (RCDATA-СЂРµР¶РёРј).
pub(crate) fn extract_title(doc: &Document) -> Option<String> {
    let mut buf = String::new();
    if walk_title(doc, doc.root(), &mut buf) {
        let trimmed = buf.split_whitespace().collect::<Vec<_>>().join(" ");
        if !trimmed.is_empty() {
            return Some(trimmed);
        }
    }
    None
}

fn walk_title(doc: &Document, id: NodeId, out: &mut String) -> bool {
    let node = doc.get(id);
    if let NodeData::Element { name, .. } = &node.data
        && name.local == "title"
    {
        for &child in &node.children {
            if let NodeData::Text(s) = &doc.get(child).data {
                out.push_str(s);
            }
        }
        return true;
    }
    for &child in &node.children {
        if walk_title(doc, child, out) {
            return true;
        }
    }
    false
}

pub(crate) fn extract_style_blocks(doc: &Document) -> String {
    let mut out = String::new();
    walk_style_blocks(doc, doc.root(), &mut out);
    out
}

/// РҐСЌС€ С‚РµРєСЃС‚Р° РІСЃРµС… РёРЅР»Р°Р№РЅРѕРІС‹С… `<style>` РІ РїРѕСЂСЏРґРєРµ РґРѕРєСѓРјРµРЅС‚Р° (BUG-743).
///
/// РЎС‡РёС‚Р°РµС‚СЃСЏ РЅР° РєР°Р¶РґРѕРј СЂРµР»РµР№Р°СѓС‚Рµ, РїРѕСЌС‚РѕРјСѓ РЅРµ СЃРѕР±РёСЂР°РµС‚ СЃС‚СЂРѕРєСѓ: РѕР±С…РѕРґРёС‚ С‚Рµ Р¶Рµ
/// СѓР·Р»С‹, С‡С‚Рѕ Рё [`walk_style_blocks`], Рё С…СЌС€РёСЂСѓРµС‚ РёС… С‚РµРєСЃС‚ РїРѕ РєСѓСЃРєР°Рј. РњРµРЅСЏРµС‚СЃСЏ
/// РїСЂРё РІСЃС‚Р°РІРєРµ, СѓРґР°Р»РµРЅРёРё Рё РїСЂР°РІРєРµ Р»СЋР±РѕРіРѕ Р±Р»РѕРєР° вЂ” СЌС‚РѕРіРѕ РґРѕСЃС‚Р°С‚РѕС‡РЅРѕ, С‡С‚РѕР±С‹
/// РїРѕРЅСЏС‚СЊ, С‡С‚Рѕ РєР°СЃРєР°Рґ РїРѕСЂР° РїРµСЂРµСЃРѕР±СЂР°С‚СЊ.
pub(crate) fn inline_style_fingerprint(doc: &Document) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    hash_style_blocks(doc, doc.root(), &mut h);
    // РџСѓСЃС‚РѕР№ РґРѕРєСѓРјРµРЅС‚ Рё РґРѕРєСѓРјРµРЅС‚ Р±РµР· РµРґРёРЅРѕРіРѕ `<style>` РґРѕР»Р¶РЅС‹ РґР°РІР°С‚СЊ РѕРґРёРЅ С…СЌС€ вЂ”
    // РѕС‚РґРµР»СЊРЅР°СЏ СЃРѕР»СЊ РЅРµ РЅСѓР¶РЅР°, РЅРѕ РґР»РёРЅР° С†РµРїРѕС‡РєРё РІ РЅРµРіРѕ СѓР¶Рµ РІРѕС€Р»Р°.
    0_u8.hash(&mut h);
    h.finish()
}

/// Р РµРєСѓСЂСЃРёРІРЅР°СЏ РїРѕР»РѕРІРёРЅР° [`inline_style_fingerprint`].
fn hash_style_blocks(doc: &Document, id: NodeId, h: &mut impl std::hash::Hasher) {
    let node = doc.get(id);
    if let NodeData::Element { name, .. } = &node.data
        && name.local == "style"
    {
        for &child in &node.children {
            if let NodeData::Text(s) = &doc.get(child).data {
                h.write(s.as_bytes());
            }
        }
        h.write_u8(0xff);
        return;
    }
    for &child in &node.children {
        hash_style_blocks(doc, child, h);
    }
}

/// CSS СЃС‚СЂР°РЅРёС†С‹, РєРѕС‚РѕСЂС‹Р№ РґРёРЅР°РјРёС‡РµСЃРєРёР№ `<style>` РёР·РјРµРЅРёС‚СЊ РЅРµ РјРѕР¶РµС‚ (BUG-743).
///
/// РџРѕР·РІРѕР»СЏРµС‚ РїРµСЂРµСЃРѕР±СЂР°С‚СЊ РєР°СЃРєР°Рґ РїРѕСЃР»Рµ РїРѕР·РґРЅРµР№ РІСЃС‚Р°РІРєРё `<style>` С†РµР»РёРєРѕРј РёР·
/// РїР°РјСЏС‚Рё: С‚РµРєСЃС‚, РїСЂРёС‚СЏРЅСѓС‚С‹Р№ `@import`-Р°РјРё РёРЅР»Р°Р№РЅРѕРІС‹С… Р»РёСЃС‚РѕРІ (РїСЂРµС„РёРєСЃ вЂ” РїРѕ CSS
/// Cascade L4 В§6.5 РїСЂР°РІРёР»Р° РёРјРїРѕСЂС‚РёСЂРѕРІР°РЅРЅРѕРіРѕ Р»РёСЃС‚Р° РёРґСѓС‚ СЂР°РЅСЊС€Рµ), Рё С‚РµР»Р° РІРЅРµС€РЅРёС…
/// `<link rel=stylesheet>` (СЃСѓС„С„РёРєСЃ вЂ” РєР°Рє РїСЂРё РїРµСЂРІРѕР№ СЃР±РѕСЂРєРµ). РЎРµС‚РµРІС‹С… Р·Р°РїСЂРѕСЃРѕРІ
/// РїРµСЂРµСЃР±РѕСЂРєР° РЅРµ РґРµР»Р°РµС‚, РїРѕСЌС‚РѕРјСѓ `@import` РІРЅСѓС‚СЂРё *РЅРѕРІРѕРіРѕ* `<style>` РѕСЃС‚Р°РЅРµС‚СЃСЏ
/// РЅРµСЂР°Р·СЂРµС€С‘РЅРЅС‹Рј; СЌС‚Рѕ РѕСЃРѕР·РЅР°РЅРЅС‹Р№ СЂР°Р·РјРµРЅ вЂ” СЂРµР»РµР№Р°СѓС‚ РЅРµ РјРµСЃС‚Рѕ РґР»СЏ СЃРµС‚Рё.
#[derive(Clone)]
pub(crate) struct DynamicCssBase {
    /// РЎРѕРґРµСЂР¶РёРјРѕРµ `@import`-РѕРІ РёРЅР»Р°Р№РЅРѕРІС‹С… `<style>`, СЂР°Р·СЂРµС€С‘РЅРЅРѕРµ РїСЂРё Р·Р°РіСЂСѓР·РєРµ.
    pub(crate) imports_prefix: String,
    /// РЎРєР»РµРµРЅРЅС‹Рµ С‚РµР»Р° РІРЅРµС€РЅРёС… `<link rel=stylesheet>`.
    pub(crate) linked: String,
    /// РҐСЌС€ РёРЅР»Р°Р№РЅРѕРІС‹С… `<style>`, РёР· РєРѕС‚РѕСЂС‹С… СЃРѕР±СЂР°РЅ С‚РµРєСѓС‰РёР№ Р»РёСЃС‚.
    pub(crate) inline_fp: u64,
}

fn walk_style_blocks(doc: &Document, id: NodeId, out: &mut String) {
    let node = doc.get(id);
    if let NodeData::Element { name, .. } = &node.data
        && name.local == "style"
    {
        for &child in &node.children {
            if let NodeData::Text(s) = &doc.get(child).data {
                out.push_str(s);
                out.push('\n');
            }
        }
        return;
    }
    for &child in &node.children {
        walk_style_blocks(doc, child, out);
    }
}

/// Р¤РѕСЂРјР°С‚ Р·Р°РіРѕР»РѕРІРєР° РѕРєРЅР°. РЎ title РёР· СЃС‚СЂР°РЅРёС†С‹ вЂ” `"<title> вЂ” Lumen"`,
/// Р±РµР· вЂ” fallback РЅР° РІРµСЂСЃРёСЋ Р±РёР»РґР°.
pub(crate) fn window_title(page_title: Option<&str>) -> String {
    match page_title {
        Some(t) => format!("{t} вЂ” Lumen"),
        None => format!("Lumen {}", env!("CARGO_PKG_VERSION")),
    }
}

// в”Ђв”Ђ HTML5 Drag and Drop state (PH3-9) в”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђ
