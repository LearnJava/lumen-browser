//! Внедрение записей пакета совместимости в документ (COMPAT-PACK-CSS, ADR-034 §4).
//!
//! Две записи доходят до документа: `css` — изолированная таблица (сливается в
//! каскад после авторских листов, в `document.styleSheets` не видна), `hide_banner`
//! — скрытие элемента только при совпадении текста. Хост выбирает запись: документ
//! каждого фрейма получает свой набор по своему хосту. Единственный поставщик
//! записей — [`crate::lumen::compat`].

use lumen_dom::{Document, NodeData, NodeId, QualName};
use lumen_update_manifest::compat_pack::{Action, CompatEntry};

/// Что пакет применяет к одному документу. `CompatInject::none()` — ничего.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct CompatInject {
    /// Текст всех записей `css` (каждая ≤ [`lumen_update_manifest::compat_pack::MAX_CSS_BYTES`]).
    pub(crate) css: String,
    /// `(селектор, обязательный текст)` всех записей `hide_banner`.
    pub(crate) hide_banners: Vec<(String, String)>,
    /// Профиль первой записи `identity` (COMPAT-PACK-IDENTITY): его
    /// представляет весь документ — HTTP-клиенты страницы и JS-рантайм.
    pub(crate) identity: Option<lumen_core::IdentityProfile>,
}

impl CompatInject {
    pub(crate) fn none() -> Self {
        Self::default()
    }

    /// Записи уже отобраны по хосту документа (`CompatPack::entries_for_host`) —
    /// здесь только отображение действия в данные.
    pub(crate) fn from_entries(entries: &[CompatEntry]) -> Self {
        let mut inject = Self::none();
        for e in entries {
            match &e.action {
                Action::Css { css } => {
                    inject.css.push_str(css);
                    inject.css.push('\n');
                }
                Action::HideBanner { selector, text } => {
                    inject.hide_banners.push((selector.clone(), text.clone()));
                }
                // Записи приходят отсортированными по ранку; первая запись
                // identity и есть та, которую представляют слои документа.
                Action::Identity { profile } if inject.identity.is_none() => {
                    inject.identity = Some((*profile).into());
                }
                _ => {}
            }
        }
        inject
    }
}

/// Скрыть баннеры: элемент с `style` `display:none`, только если он отвечает
/// селектору записи **и** содержит её текст. Текст проверяется по подстроке —
/// запись «скрыть плашку о печенье» не должна прятать любой `div.banner`.
pub(crate) fn apply_hide_banners(doc: &mut Document, rules: &[(String, String)]) {
    if rules.is_empty() {
        return;
    }
    let mut hidden = Vec::new();
    for_each_element(doc, doc.root(), &mut |id| {
        for (selector, text) in rules {
            if lumen_layout::selector_query::matches_selector(doc, id, selector)
                && element_text(doc, id).contains(text.as_str())
            {
                hidden.push(id);
                break;
            }
        }
    });
    for id in hidden {
        append_style(doc, id, "display:none");
    }
}

/// Обход элементов документа в порядке документа.
fn for_each_element(doc: &Document, id: NodeId, f: &mut impl FnMut(NodeId)) {
    if doc.get(id).element_name().is_some() {
        f(id);
    }
    for &child in &doc.get(id).children {
        for_each_element(doc, child, f);
    }
}

/// Текст поддерева элемента — конкатенация текстовых узлов.
fn element_text(doc: &Document, id: NodeId) -> String {
    fn rec(doc: &Document, id: NodeId, out: &mut String) {
        match &doc.get(id).data {
            NodeData::Text(text) => out.push_str(text),
            NodeData::Element { .. } => {
                for &child in &doc.get(id).children {
                    rec(doc, child, out);
                }
            }
            _ => {}
        }
    }
    let mut out = String::new();
    for &child in &doc.get(id).children {
        rec(doc, child, &mut out);
    }
    out
}

/// Дописать `display:none` в `style`, не теряя прежних объявлений.
fn append_style(doc: &mut Document, id: NodeId, declaration: &str) {
    if let NodeData::Element { ref mut attrs, .. } = doc.get_mut(id).data {
        if let Some(attr) = attrs.iter_mut().find(|a| a.name.local.eq_ignore_ascii_case("style")) {
            attr.value.push(';');
            attr.value.push_str(declaration);
            return;
        }
        attrs.push(lumen_dom::Attribute { name: QualName::html("style"), value: declaration.to_owned() });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc_with_banner() -> Document {
        let mut doc = Document::new();
        let body = doc.create_element(QualName::html("body"));
        doc.append_child(doc.root(), body);
        let banner = doc.create_element(QualName::html("div"));
        set_attr(&mut doc, banner, "class", "banner");
        let text = doc.create_text("Мы используем печенье");
        doc.append_child(banner, text);
        doc.append_child(body, banner);
        let plain = doc.create_element(QualName::html("div"));
        set_attr(&mut doc, plain, "class", "banner");
        let text = doc.create_text("Обычный блок");
        doc.append_child(plain, text);
        doc.append_child(body, plain);
        doc
    }

    fn set_attr(doc: &mut Document, node: NodeId, name: &str, value: &str) {
        if let NodeData::Element { ref mut attrs, .. } = doc.get_mut(node).data {
            attrs.push(lumen_dom::Attribute { name: QualName::html(name), value: value.to_owned() });
        }
    }

    fn style_of(doc: &Document, id: NodeId) -> Option<String> {
        doc.get(id).get_attr("style").map(str::to_owned)
    }

    fn banner_ids(doc: &Document) -> Vec<NodeId> {
        lumen_layout::selector_query::query_all(doc, "div.banner")
    }

    #[test]
    fn hide_banner_hides_only_matching_text() {
        let mut doc = doc_with_banner();
        let ids = banner_ids(&doc);
        assert_eq!(ids.len(), 2);
        apply_hide_banners(&mut doc, &[(".banner".into(), "печенье".into())]);
        assert!(style_of(&doc, ids[0]).unwrap().contains("display:none"), "баннер с текстом скрыт");
        assert_eq!(style_of(&doc, ids[1]), None, "баннер без текста не тронут");
    }

    #[test]
    fn hide_banner_without_text_match_or_selector_miss_keeps_everything() {
        let mut doc = doc_with_banner();
        let ids = banner_ids(&doc);
        apply_hide_banners(&mut doc, &[(".banner".into(), "нет такого текста".into())]);
        assert!(ids.iter().all(|&id| style_of(&doc, id).is_none()));
        apply_hide_banners(&mut doc, &[(".other".into(), "печенье".into())]);
        assert!(ids.iter().all(|&id| style_of(&doc, id).is_none()));
    }

    #[test]
    fn from_entries_picks_css_and_hide_banner_only() {
        let entries = vec![
            CompatEntry {
                id: "a-css".into(),
                root_cause: "site-bug".into(),
                matches: vec!["a.com".into()],
                site_class: Default::default(),
                action: Action::Css { css: "#t{color:red}".into() },
                bug: "BUG-1".into(),
                review_by: "2027-01-31".into(),
                tests: vec!["t".into()],
                test_exemption: None,
                engine_max: None,
            },
            CompatEntry {
                id: "b-banner".into(),
                root_cause: "site-bug".into(),
                matches: vec!["a.com".into()],
                site_class: Default::default(),
                action: Action::HideBanner { selector: "#ad".into(), text: "реклама".into() },
                bug: "BUG-1".into(),
                review_by: "2027-01-31".into(),
                tests: vec!["t".into()],
                test_exemption: None,
                engine_max: None,
            },
            CompatEntry {
                id: "c-issue".into(),
                root_cause: "site-bug".into(),
                matches: vec!["a.com".into()],
                site_class: Default::default(),
                action: Action::KnownIssue { message: "медленно".into() },
                bug: "BUG-1".into(),
                review_by: "2027-01-31".into(),
                tests: vec!["t".into()],
                test_exemption: None,
                engine_max: None,
            },
        ];
        let inject = CompatInject::from_entries(&entries);
        assert_eq!(inject.css, "#t{color:red}\n");
        assert_eq!(inject.hide_banners, vec![("#ad".into(), "реклама".into())]);
        assert_eq!(inject.identity, None);
        // COMPAT-PACK-IDENTITY: первая запись identity задаёт профиль документа.
        let with_id = CompatInject::from_entries(&[CompatEntry {
            id: "d-id".into(),
            root_cause: "gate-ua".into(),
            matches: vec!["a.com".into()],
            site_class: lumen_update_manifest::compat_pack::SiteClass::Bank,
            action: Action::Identity { profile: lumen_update_manifest::compat_pack::IdentityProfile::FirefoxWin },
            bug: "BUG-1".into(),
            review_by: "2027-01-31".into(),
            tests: vec!["t".into()],
            test_exemption: None,
            engine_max: None,
        }]);
        assert_eq!(with_id.identity, Some(lumen_core::IdentityProfile::FirefoxWin));
    }
}
