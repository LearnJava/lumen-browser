//! Reading facts back out of a parsed [`Document`]: the page title, its inline
//! `<style>` text and the fingerprint of that text, plus the window-title
//! format the shell puts on the OS window.
//!
//! Moved out of `main.rs` by the SPLIT track (batch SH-3c); behaviour and
//! signatures are unchanged.

use crate::*;

/// Найти первый `<title>` в дереве и склеить его текстовые дети.
///
/// HTML5 разрешает только один `<title>` в `<head>`, но мы lenient-парсер —
/// берём первый встречный. Энтити уже декодированы tokenizer-ом (RCDATA-режим).
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

/// Хэш текста всех инлайновых `<style>` в порядке документа (BUG-743).
///
/// Считается на каждом релейауте, поэтому не собирает строку: обходит те же
/// узлы, что и [`walk_style_blocks`], и хэширует их текст по кускам. Меняется
/// при вставке, удалении и правке любого блока — этого достаточно, чтобы
/// понять, что каскад пора пересобрать.
pub(crate) fn inline_style_fingerprint(doc: &Document) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    hash_style_blocks(doc, doc.root(), &mut h);
    // Пустой документ и документ без единого `<style>` должны давать один хэш —
    // отдельная соль не нужна, но длина цепочки в него уже вошла.
    0_u8.hash(&mut h);
    h.finish()
}

/// Рекурсивная половина [`inline_style_fingerprint`].
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

/// Fingerprint of every `<link>` element's `rel`/`href`/`media` in document
/// order (BUG-443).
///
/// The sibling of [`inline_style_fingerprint`] for the *external* half of the
/// cascade. Since BUG-443 the shell collects the page CSS **before** running
/// the document's scripts, so it needs to know whether those scripts touched
/// the set of linked stylesheets — a script-inserted `<link rel=stylesheet>`
/// must still reach the first cascade, and the only way to notice one is to
/// compare this hash across script execution. Cheap: no fetch, no string
/// building, one tree walk.
pub(crate) fn stylesheet_link_fingerprint(doc: &Document) -> u64 {
    use std::hash::Hasher;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    hash_link_elements(doc, doc.root(), &mut h);
    h.write_u8(0);
    h.finish()
}

/// Р екурсивная половина [`stylesheet_link_fingerprint`].
fn hash_link_elements(doc: &Document, id: NodeId, h: &mut impl std::hash::Hasher) {
    let node = doc.get(id);
    if let NodeData::Element { name, attrs } = &node.data
        && name.local == "link"
    {
        for a in attrs {
            h.write(a.name.local.as_bytes());
            h.write_u8(0x1e);
            h.write(a.value.as_bytes());
            h.write_u8(0x1f);
        }
        h.write_u8(0xff);
    }
    for &child in &node.children {
        hash_link_elements(doc, child, h);
    }
}

/// CSS страницы, который динамический `<style>` изменить не может (BUG-743).
///
/// Позволяет пересобрать каскад после поздней вставки `<style>` целиком из
/// памяти: текст, притянутый `@import`-ами инлайновых листов (префикс — по CSS
/// Cascade L4 §6.5 правила импортированного листа идут раньше), и тела внешних
/// `<link rel=stylesheet>` (суффикс — как при первой сборке). Сетевых запросов
/// пересборка не делает, поэтому `@import` внутри *нового* `<style>` останется
/// неразрешённым; это осознанный размен — релейаут не место для сети.
#[derive(Clone)]
pub(crate) struct DynamicCssBase {
    /// Содержимое `@import`-ов инлайновых `<style>`, разрешённое при загрузке.
    pub(crate) imports_prefix: String,
    /// Склеенные тела внешних `<link rel=stylesheet>`.
    pub(crate) linked: String,
    /// Хэш инлайновых `<style>`, из которых собран текущий лист.
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

/// Формат заголовка окна. С title из страницы — `"<title> — Lumen"`,
/// без — fallback на версию билда.
pub(crate) fn window_title(page_title: Option<&str>) -> String {
    match page_title {
        Some(t) => format!("{t} — Lumen"),
        None => format!("Lumen {}", env!("CARGO_PKG_VERSION")),
    }
}

// ── HTML5 Drag and Drop state (PH3-9) ────────────────────────────────────────
