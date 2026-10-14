//! Сбор и подмена текстовых узлов для перевода страницы (UX-TRANSLATE).
//!
//! Модуль не знает про модель: [`collect_translatable`] отдаёт плоский список
//! строк в порядке документа, [`TranslationSession::apply`] подменяет `data`
//! узлов переводом и запоминает оригиналы, [`TranslationSession::revert`]
//! возвращает их. Разметка и атрибуты не затрагиваются.

use crate::{Document, NodeData, NodeId};

/// Элементы, текст которых переводить нельзя или незачем.
const SKIP_ELEMENTS: &[&str] = &[
    "script", "style", "noscript", "template", "textarea", "code", "pre", "kbd", "samp", "svg", "math", "iframe",
];

/// Текстовый узел, пригодный к переводу.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextSegment {
    pub node: NodeId,
    pub text: String,
}

fn has_letters(s: &str) -> bool {
    s.chars().any(char::is_alphabetic)
}

/// Язык страницы из `<html lang>` (основная подметка, `en-US` → `en`).
pub fn page_language(doc: &Document) -> Option<String> {
    let root = doc.document_element()?;
    let lang = doc.get(root).get_attr("lang")?;
    let primary = lang.split(['-', '_']).next()?.trim().to_ascii_lowercase();
    (!primary.is_empty()).then_some(primary)
}

/// Язык по преобладающей письменности текста — запасной вариант без `lang`.
/// Различает только письменности с однозначным языком (кириллица → `ru`,
/// иероглифы/кана → `ja`, хангыль → `ko`, арабская → `ar`, греческая → `el`);
/// латиницу не классифицирует (`None`). Нужны ≥ 20 букв и ≥ 60 % одной письменности.
pub fn detect_language(segments: &[TextSegment]) -> Option<String> {
    let mut counts = [0usize; 5];
    let mut total = 0usize;
    for c in segments.iter().flat_map(|s| s.text.chars()).filter(|c| c.is_alphabetic()) {
        total += 1;
        let i = match c as u32 {
            0x0400..=0x052F => 0,
            0x3040..=0x30FF | 0x4E00..=0x9FFF => 1,
            0xAC00..=0xD7AF | 0x1100..=0x11FF => 2,
            0x0600..=0x06FF => 3,
            0x0370..=0x03FF => 4,
            _ => continue,
        };
        counts[i] += 1;
    }
    if total < 20 {
        return None;
    }
    let (i, &n) = counts.iter().enumerate().max_by_key(|(_, n)| **n)?;
    (n * 10 >= total * 6).then(|| ["ru", "ja", "ko", "ar", "el"][i].to_string())
}

/// Текстовые узлы документа в порядке обхода, без служебных элементов,
/// поддеревьев с `translate="no"` и узлов без букв.
pub fn collect_translatable(doc: &Document) -> Vec<TextSegment> {
    let mut out = Vec::new();
    walk(doc, doc.root(), &mut out);
    out
}

fn walk(doc: &Document, id: NodeId, out: &mut Vec<TextSegment>) {
    let node = doc.get(id);
    match &node.data {
        NodeData::Text(s) => {
            if has_letters(s) {
                out.push(TextSegment { node: id, text: s.clone() });
            }
            return;
        }
        NodeData::Element { name, .. } => {
            if SKIP_ELEMENTS.contains(&name.local.as_str()) {
                return;
            }
            if node.get_attr("translate").is_some_and(|v| v.eq_ignore_ascii_case("no")) {
                return;
            }
        }
        NodeData::Comment(_) | NodeData::ProcessingInstruction { .. } | NodeData::Doctype { .. } => return,
        _ => {}
    }
    for &child in &node.children {
        walk(doc, child, out);
    }
}

/// Применённый перевод: хранит оригиналы для отката.
#[derive(Debug, Default)]
pub struct TranslationSession {
    originals: Vec<(NodeId, String)>,
}

impl TranslationSession {
    /// Подменяет текст узлов переводом (`None` — оставить оригинал).
    /// Возвращает число подменённых узлов. Узлы, чей текст изменился с момента
    /// сбора (скрипт страницы), пропускаются.
    pub fn apply(&mut self, doc: &mut Document, segments: &[TextSegment], translated: &[Option<String>]) -> usize {
        let mut n = 0;
        for (seg, tr) in segments.iter().zip(translated) {
            let Some(tr) = tr else { continue };
            let node = doc.get_mut(seg.node);
            let NodeData::Text(cur) = &mut node.data else { continue };
            if *cur != seg.text {
                continue;
            }
            // Пробелы по краям оригинала сохраняем: перевод приходит без них.
            let lead = &seg.text[..seg.text.len() - seg.text.trim_start().len()];
            let trail = &seg.text[seg.text.trim_end().len()..];
            let new = format!("{lead}{}{trail}", tr.trim());
            self.originals.push((seg.node, std::mem::replace(cur, new)));
            n += 1;
        }
        n
    }

    /// Применён ли перевод.
    pub fn is_active(&self) -> bool {
        !self.originals.is_empty()
    }

    /// Возвращает оригиналы; число восстановленных узлов.
    pub fn revert(&mut self, doc: &mut Document) -> usize {
        let mut n = 0;
        for (id, orig) in self.originals.drain(..).rev() {
            if let NodeData::Text(cur) = &mut doc.get_mut(id).data {
                *cur = orig;
                n += 1;
            }
        }
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc_with(html_children: &[(&str, &str)]) -> (Document, Vec<NodeId>) {
        let mut doc = Document::new();
        let root = doc.root();
        let html = doc.create_element(crate::QualName::html("html"));
        doc.append_child(root, html);
        let mut texts = Vec::new();
        for (tag, text) in html_children {
            let el = doc.create_element(crate::QualName::html(*tag));
            doc.append_child(html, el);
            let t = doc.create_text(*text);
            doc.append_child(el, t);
            texts.push(t);
        }
        (doc, texts)
    }

    #[test]
    fn collects_skipping_service_elements_and_non_letters() {
        let (doc, texts) = doc_with(&[("p", "Hello"), ("script", "var a"), ("span", "123"), ("pre", "code")]);
        let segs = collect_translatable(&doc);
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].node, texts[0]);
    }

    #[test]
    fn apply_and_revert_roundtrip_keeps_edge_whitespace() {
        let (mut doc, texts) = doc_with(&[("p", " Hello ")]);
        let segs = collect_translatable(&doc);
        let mut s = TranslationSession::default();
        assert_eq!(s.apply(&mut doc, &segs, &[Some("Привет".into())]), 1);
        assert!(matches!(&doc.get(texts[0]).data, NodeData::Text(t) if t == " Привет "));
        assert!(s.is_active());
        assert_eq!(s.revert(&mut doc), 1);
        assert!(matches!(&doc.get(texts[0]).data, NodeData::Text(t) if t == " Hello "));
        assert!(!s.is_active());
    }

    #[test]
    fn detects_language_by_script() {
        let seg = |t: &str| vec![TextSegment { node: NodeId(0), text: t.into() }];
        assert_eq!(detect_language(&seg("Это достаточно длинный русский текст для проверки")).as_deref(), Some("ru"));
        assert_eq!(detect_language(&seg("This is a long enough english text sample")), None);
        assert_eq!(detect_language(&seg("Привет")), None);
    }

    #[test]
    fn apply_skips_missing_and_changed_nodes() {
        let (mut doc, texts) = doc_with(&[("p", "One"), ("p", "Two")]);
        let segs = collect_translatable(&doc);
        if let NodeData::Text(t) = &mut doc.get_mut(texts[1]).data {
            *t = "Changed".into();
        }
        let mut s = TranslationSession::default();
        assert_eq!(s.apply(&mut doc, &segs, &[None, Some("Два".into())]), 0);
    }
}
