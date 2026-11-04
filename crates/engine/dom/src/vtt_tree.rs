//! Поддерево cue в UA-тени `<video>` (WebVTT §7.2/§7.4, срез VTT-RENDER-S3).
//!
//! Cue — настоящие DOM-узлы в закрытой UA-тени `<video>` (решение брифа
//! VTT-RENDER п.1), а не оверлей поверх display list: окно, `--screenshot`,
//! `--ipc-server` и CPU-растр видят их одним и тем же кодом layout/paint.
//!
//! Структура поддерева:
//! `div[data-lumen-vtt=container]` (обычный блок в потоке) >
//! `div[data-lumen-vtt=cue]` (`position:absolute`, геометрия в `style`
//! из [`cue_box_css`]) > `span[data-lumen-vtt=bg]` (фон) >
//! узлы cue text из [`parse_cue_text`]: `c` -> `span`,
//! `i`/`b`/`u`/`ruby`/`rt` -> одноимённые элементы, `v` -> `span` +
//! `data-lumen-vtt-voice`, `lang` -> `span` + `lang`; у каждого элемента
//! cue text — `data-lumen-vtt-type` и `class` из классов; текст — текстовый
//! узел, таймстамп пропускается.
//!
//! Роль узла достоверна только по принадлежности UA-тени
//! ([`Document::ua_vtt_role`]): поддельный `data-lumen-vtt` в светлом
//! дереве роль не получает, потому что цепочка его родителей не содержит
//! узла `ShadowRoot`.

use crate::vtt_geometry::{CueSettings, cue_box_css};
use crate::vtt_text::{CueNode, CueTag, parse_cue_text};
use crate::{Attribute, Document, NodeData, NodeId, QualName};

/// Атрибут-роль узла поддерева cue (значения `container`/`cue`/`bg`).
///
/// Тип узла cue text (тег WebVTT) хранится отдельно, в
/// `data-lumen-vtt-type`, чтобы роли и типы не смешивались в одном
/// пространстве значений.
const ROLE_ATTR: &str = "data-lumen-vtt";

/// Cue для рендера — единица обмена между JS-шимом и DOM (срез S5 вызывает
/// [`Document::set_vtt_cues`] с этим списком).
#[derive(Debug, Clone, PartialEq)]
pub struct CueRender {
    /// Текст cue с разметкой (payload между строкой тайминга и концом блока).
    pub text: String,
    /// Обработанные настройки позиционирования (§7.2); геометрия бокса
    /// пишется в `style` бокса cue строкой CSS ([`cue_box_css`]).
    pub settings: CueSettings,
    /// Идентификатор cue из файла WebVTT (пустая строка, если не задан).
    pub id: String,
}

/// Роль узла в UA-тени `<video>` — см. [`Document::ua_vtt_role`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UaVttRole {
    /// Контейнер cue — `div[data-lumen-vtt=container]`, единственный ребёнок
    /// теневого корня (§7.2: обычный блок в потоке, `overflow: hidden`).
    Container,
    /// Бокс cue — `div[data-lumen-vtt=cue]`, `position:absolute` внутри
    /// контейнера; геометрия — в его атрибуте `style`.
    CueBox,
    /// Фоновый `span[data-lumen-vtt=bg]` внутри бокса cue.
    Background,
    /// Узел WebVTT в поддереве бокса cue: элементы cue text и их текст
    /// (роль по принадлежности тени, не по атрибуту).
    Node,
}

impl Document {
    /// Заменяет поддерево cue в UA-тени `host` на боксы из `cues`
    /// (WebVTT §7.2, срез VTT-RENDER-S3).
    ///
    /// Узел `host` обязан быть `<video>` с живой UA-тенью
    /// ([`Document::attach_ua_shadow_root`]); иначе — no-op, включая вызов
    /// с непустым списком. Прежний контейнер (если был) отсоединяется от
    /// дерева, порядок cue в `cues` сохраняется в порядке детей контейнера.
    /// Пустой список только удаляет контейнер.
    ///
    /// Узлы создаются напрямую, как содержимое UA-тени
    /// ([`Document::attach_ua_shadow_root`]): это не мутация, наблюдаемая
    /// страницей — закрытая тень ей недоступна, `create_element` здесь не
    /// добавляет ветвлений.
    pub fn set_vtt_cues(&mut self, host: NodeId, cues: &[CueRender]) {
        let Some(sr) = self.vtt_shadow_root(host) else {
            return;
        };
        // Прежний контейнер — ровно один по построению; узел остаётся в
        // арене сиротой (Phase 0: очистки арены нет, как у заменённой
        // `attach_shadow` тени).
        let old = self
            .get(sr)
            .children
            .iter()
            .copied()
            .find(|&child| self.get(child).get_attr(ROLE_ATTR) == Some("container"));
        if let Some(old) = old {
            self.detach(old);
        }
        if cues.is_empty() {
            return;
        }
        let container = self.vtt_element("div", &[(ROLE_ATTR, "container")]);
        self.append_child(sr, container);
        for cue in cues {
            let style = cue_box_css(&cue.settings);
            let cue_box = self.vtt_element("div", &[(ROLE_ATTR, "cue"), ("style", &style)]);
            let bg = self.vtt_element("span", &[(ROLE_ATTR, "bg")]);
            self.append_child(cue_box, bg);
            self.append_cue_nodes(bg, &parse_cue_text(&cue.text));
            self.append_child(container, cue_box);
        }
    }

    /// Роль `id` в UA-тени `<video>` и хост той тени
    /// (срез VTT-RENDER-S3; образец — [`Document::ua_slot_role`]).
    ///
    /// Роль по структуре: узел обязан лежать в *UA*-тени (авторская
    /// `attachShadow` поверх `<video>` роль обнуляет —
    /// [`Document::ua_shadow_host`] не признаёт её) хоста-`<video>`.
    /// `container`/`cue`/`bg` различаются атрибутом-ролью; прочие узлы
    /// тени — [`UaVttRole::Node`]. Поддельный `data-lumen-vtt` в светлом
    /// дереве роли не получает: в цепочке его родителей нет узла
    /// `ShadowRoot`. Для узла вне UA-тени `<video>` — `None`.
    pub fn ua_vtt_role(&self, id: NodeId) -> Option<(UaVttRole, NodeId)> {
        // Подъём до ближайшего узла ShadowRoot: у корня тени `parent` не
        // выставлен (`attach_shadow`), поэтому светлое дерево всегда
        // заканчивается `None`, не доходя до чужой тени.
        let mut cur = self.nodes.get(id.index())?.parent?;
        loop {
            let node = self.nodes.get(cur.index())?;
            if matches!(node.data, NodeData::ShadowRoot { .. }) {
                break;
            }
            cur = node.parent?;
        }
        let host = self.ua_shadow_host(cur)?;
        if self.get(host).element_name()?.local.as_str() != "video" {
            return None;
        }
        let role = match self.get(id).get_attr(ROLE_ATTR) {
            Some("container") => UaVttRole::Container,
            Some("cue") => UaVttRole::CueBox,
            Some("bg") => UaVttRole::Background,
            _ => UaVttRole::Node,
        };
        Some((role, host))
    }

    /// Теневой корень `host`, если тот — живой UA-корень хоста-`<video>`.
    fn vtt_shadow_root(&self, host: NodeId) -> Option<NodeId> {
        let sr = self.shadow_root_of(host)?;
        if self.ua_shadow_host(sr) != Some(host) {
            return None;
        }
        if self.get(host).element_name()?.local.as_str() != "video" {
            return None;
        }
        Some(sr)
    }

    /// Создаёт элемент с атрибутами как узел UA-тени (без наблюдаемых
    /// страницей мутаций): `create_element` + прямой набор атрибутов.
    fn vtt_element(&mut self, name: &str, attrs: &[(&str, &str)]) -> NodeId {
        let id = self.create_element(QualName::html(name));
        if let NodeData::Element { attrs: slot, .. } = &mut self.get_mut(id).data {
            slot.extend(attrs.iter().map(|&(key, value)| Attribute {
                name: QualName::html(key),
                value: value.to_string(),
            }));
        }
        id
    }

    /// Кладёт узлы cue text ([`parse_cue_text`]) в `parent`, сохраняя
    /// порядок; таймстампы пропускаются (бриф среза S3).
    fn append_cue_nodes(&mut self, parent: NodeId, nodes: &[CueNode]) {
        for node in nodes {
            match node {
                CueNode::Text(text) => {
                    let text_node = self.create_text(text.clone());
                    self.append_child(parent, text_node);
                }
                CueNode::Timestamp(_) => {}
                CueNode::Element {
                    tag,
                    classes,
                    annotation,
                    children,
                } => {
                    let el =
                        self.append_cue_element(parent, *tag, classes, annotation.as_deref());
                    self.append_cue_nodes(el, children);
                }
            }
        }
    }

    /// Создаёт элемент cue text по тегу и вешает его на `parent`.
    ///
    /// `c` -> `span`; `i`/`b`/`u`/`ruby`/`rt` -> одноимённые элементы;
    /// `v` -> `span` + `data-lumen-vtt-voice`; `lang` -> `span` + `lang`;
    /// везде `data-lumen-vtt-type` и, при наличии, `class`.
    fn append_cue_element(
        &mut self,
        parent: NodeId,
        tag: CueTag,
        classes: &[String],
        annotation: Option<&str>,
    ) -> NodeId {
        let (name, kind) = match tag {
            CueTag::C => ("span", "c"),
            CueTag::I => ("i", "i"),
            CueTag::B => ("b", "b"),
            CueTag::U => ("u", "u"),
            CueTag::Ruby => ("ruby", "ruby"),
            CueTag::Rt => ("rt", "rt"),
            CueTag::V => ("span", "v"),
            CueTag::Lang => ("span", "lang"),
        };
        // Порядок классов из [`parse_cue_text`] сохраняется.
        let joined = if classes.is_empty() {
            None
        } else {
            Some(classes.join(" "))
        };
        let mut attrs: Vec<(&str, &str)> = vec![("data-lumen-vtt-type", kind)];
        if let Some(joined) = &joined {
            attrs.push(("class", joined));
        }
        let annotation_attr: Option<(&str, &str)> = match tag {
            CueTag::V => annotation.map(|a| ("data-lumen-vtt-voice", a)),
            CueTag::Lang => annotation.map(|a| ("lang", a)),
            _ => None,
        };
        if let Some((key, value)) = annotation_attr {
            attrs.push((key, value));
        }
        let el = self.vtt_element(name, &attrs);
        self.append_child(parent, el);
        el
    }
}

#[cfg(test)]
mod tests {
    use super::{CueRender, CueSettings, Document, NodeData, QualName, UaVttRole};

    /// `<video>` под корнем документа: `create_element` сам вешает UA-тень.
    fn video_doc() -> (Document, crate::NodeId) {
        let mut doc = Document::new();
        let video = doc.create_element(QualName::html("video"));
        doc.append_child(doc.root(), video);
        (doc, video)
    }

    fn cue(text: &str) -> CueRender {
        CueRender {
            text: text.to_string(),
            settings: CueSettings::default(),
            id: String::new(),
        }
    }

    fn set_attr(doc: &mut Document, id: crate::NodeId, name: &str, value: &str) {
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(id).data {
            attrs.push(crate::Attribute {
                name: QualName::html(name),
                value: value.to_string(),
            });
        }
    }

    /// Первый ребёнок `parent` с атрибутом `name = value`.
    fn child_with_attr(doc: &Document, parent: crate::NodeId, name: &str, value: &str) -> Option<crate::NodeId> {
        doc.get(parent)
            .children
            .iter()
            .copied()
            .find(|&child| doc.get(child).get_attr(name) == Some(value))
    }

    fn attr(doc: &Document, id: crate::NodeId, name: &str) -> String {
        doc.get(id).get_attr(name).unwrap_or_default().to_string()
    }

    /// Имя локального тега элемента (пустая строка — не элемент).
    fn tag_of(doc: &Document, id: crate::NodeId) -> String {
        doc.get(id)
            .element_name()
            .map(|n| n.local.as_str().to_owned())
            .unwrap_or_default()
    }

    fn text_of(doc: &Document, id: crate::NodeId) -> String {
        match &doc.get(id).data {
            NodeData::Text(s) => s.clone(),
            _ => String::new(),
        }
    }

    fn children(doc: &Document, id: crate::NodeId) -> Vec<crate::NodeId> {
        doc.get(id).children.clone()
    }

    /// Структурные ожидания брифа п.1: один контейнер, боксы cue в порядке
    /// ввода, разметка второго cue — `bg > span[v/Fred] > i > "x"`.
    #[test]
    fn two_cues_build_container_and_ordered_boxes() {
        let (mut doc, video) = video_doc();
        doc.set_vtt_cues(video, &[cue("hi"), cue("<v Fred><i>x</i></v>")]);

        let sr = doc.shadow_root_of(video).unwrap();
        assert_eq!(children(&doc, sr).len(), 1, "в теневом корне только контейнер");
        let container = children(&doc, sr)[0];
        assert_eq!(tag_of(&doc, container), "div");
        assert_eq!(attr(&doc, container, "data-lumen-vtt"), "container");
        assert_eq!(
            doc.ua_vtt_role(container),
            Some((UaVttRole::Container, video))
        );

        let boxes = children(&doc, container);
        assert_eq!(boxes.len(), 2, "боксы cue — в порядке ввода");
        for b in &boxes {
            assert_eq!(tag_of(&doc, *b), "div");
            assert_eq!(attr(&doc, *b, "data-lumen-vtt"), "cue");
            assert!(
                attr(&doc, *b, "style").starts_with("position:absolute"),
                "геометрия cue-бокса из cue_box_css в style: {}",
                attr(&doc, *b, "style")
            );
            assert_eq!(doc.ua_vtt_role(*b), Some((UaVttRole::CueBox, video)));
        }

        // Первый cue «hi»: bg > текст «hi».
        let bg1 = child_with_attr(&doc, boxes[0], "data-lumen-vtt", "bg").unwrap();
        assert_eq!(tag_of(&doc, bg1), "span");
        assert_eq!(doc.ua_vtt_role(bg1), Some((UaVttRole::Background, video)));
        assert_eq!(children(&doc, bg1).len(), 1);
        assert_eq!(text_of(&doc, children(&doc, bg1)[0]), "hi");
        assert_eq!(
            doc.ua_vtt_role(children(&doc, bg1)[0]),
            Some((UaVttRole::Node, video)),
            "текст внутри бокса cue — роль Node по принадлежности тени"
        );

        // Второй cue: bg > span[data-lumen-vtt-type=v][data-lumen-vtt-voice=Fred] > i > «x».
        let bg2 = child_with_attr(&doc, boxes[1], "data-lumen-vtt", "bg").unwrap();
        let voice = child_with_attr(&doc, bg2, "data-lumen-vtt-type", "v").unwrap();
        assert_eq!(tag_of(&doc, voice), "span");
        assert_eq!(attr(&doc, voice, "data-lumen-vtt-voice"), "Fred");
        assert_eq!(children(&doc, voice).len(), 1);
        let italic = children(&doc, voice)[0];
        assert_eq!(tag_of(&doc, italic), "i");
        assert_eq!(attr(&doc, italic, "data-lumen-vtt-type"), "i");
        assert_eq!(children(&doc, italic).len(), 1);
        assert_eq!(text_of(&doc, children(&doc, italic)[0]), "x");
    }

    /// П.2: повторный вызов заменяет контейнер, а не дублирует.
    #[test]
    fn repeated_call_replaces_container() {
        let (mut doc, video) = video_doc();
        doc.set_vtt_cues(video, &[cue("first"), cue("second")]);
        doc.set_vtt_cues(video, &[cue("only")]);

        let sr = doc.shadow_root_of(video).unwrap();
        assert_eq!(children(&doc, sr).len(), 1, "контейнер ровно один");
        let container = children(&doc, sr)[0];
        let boxes = children(&doc, container);
        assert_eq!(boxes.len(), 1, "старое поддерево заменено новым");
        let bg = child_with_attr(&doc, boxes[0], "data-lumen-vtt", "bg").unwrap();
        assert_eq!(text_of(&doc, children(&doc, bg)[0]), "only");
    }

    /// П.3: пустой список удаляет контейнер.
    #[test]
    fn empty_list_removes_container() {
        let (mut doc, video) = video_doc();
        doc.set_vtt_cues(video, &[cue("hi")]);
        doc.set_vtt_cues(video, &[]);

        let sr = doc.shadow_root_of(video).unwrap();
        assert!(
            child_with_attr(&doc, sr, "data-lumen-vtt", "container").is_none(),
            "контейнера больше нет"
        );
        // Пустой список на уже чистой тени — тоже валиден (no-op по сути).
        doc.set_vtt_cues(video, &[]);
        assert!(child_with_attr(&doc, sr, "data-lumen-vtt", "container").is_none());
    }

    /// П.4: вызов на `<div>` — no-op (без тени и без построения).
    #[test]
    fn call_on_div_is_noop() {
        let mut doc = Document::new();
        let div = doc.create_element(QualName::html("div"));
        doc.append_child(doc.root(), div);
        doc.set_vtt_cues(div, &[cue("hi")]);
        assert!(doc.shadow_root_of(div).is_none(), "у div нет тени");
        assert!(children(&doc, div).is_empty(), "ничего не построено");
    }

    /// П.5: `ua_vtt_role` — `None` на светлом дереве, включая поддельный
    /// `data-lumen-vtt`.
    #[test]
    fn fake_roles_and_light_tree_are_none() {
        let (mut doc, video) = video_doc();
        let plain_div = doc.create_element(QualName::html("div"));
        doc.append_child(doc.root(), plain_div);
        let fake = doc.create_element(QualName::html("span"));
        set_attr(&mut doc, fake, "data-lumen-vtt", "cue");
        doc.append_child(plain_div, fake);
        // Светлый ребёнок <video> — тоже не в тени (NoSlot).
        let light = doc.create_element(QualName::html("span"));
        set_attr(&mut doc, light, "data-lumen-vtt", "container");
        doc.append_child(video, light);

        assert_eq!(doc.ua_vtt_role(plain_div), None);
        assert_eq!(doc.ua_vtt_role(fake), None, "поддельный атрибут роли не даёт");
        assert_eq!(doc.ua_vtt_role(light), None, "светлый ребёнок video вне тени");
    }

    /// П.6: поддерево cue не делает тени авторскими.
    #[test]
    fn vtt_shadow_stays_ua_not_author() {
        let (mut doc, video) = video_doc();
        doc.set_vtt_cues(video, &[cue("hi")]);
        assert!(!doc.has_author_shadow_roots());
    }
}
