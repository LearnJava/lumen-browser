use super::*;

// ── Раскладка cue-поддерева в UA-тени `<video>` (WebVTT §7.4, срез
// VTT-RENDER-S4). Дети `<video>` в box_tree — поддерево cue из
// `Document::set_vtt_cues` (S3), стили узлов — `apply_ua_vtt` (UA-фаза,
// до author-правил). Бриф — docs/tasks/VTT-RENDER-slices.md §S4. ──

use crate::style::{Color, Display, OverflowWrap, Position, UnicodeBidi, WhiteSpace};
use lumen_dom::vtt_geometry::CueLine;
use lumen_dom::{CueRender, CueSettings, Document, NodeId};

/// Первый `<video>` в дереве фикстуры: парсер сам вешает на него UA-тень
/// (HTML LS §4.8.11, `attach_ua_shadow_root`).
fn find_video_node(doc: &Document) -> NodeId {
    fn walk(doc: &Document, id: NodeId) -> Option<NodeId> {
        if doc.get(id).element_name().is_some_and(|n| n.local.as_str() == "video") {
            return Some(id);
        }
        for &c in &doc.get(id).children {
            if let Some(found) = walk(doc, c) {
                return Some(found);
            }
        }
        None
    }
    walk(doc, doc.root()).expect("no <video> in fixture")
}

fn cue(settings: CueSettings, text: &str) -> CueRender {
    CueRender {
        text: text.to_string(),
        settings,
        id: String::new(),
    }
}

/// Разбор + `set_vtt_cues` (S3) + раскладка, как у [`lay`]: cue кладутся в
/// UA-тень видео до построения боксов, чтобы раскладка их увидела.
/// Возвращает документ — стили inline-узлов тени проверяются через
/// [`collect_computed_styles`] (у inline-элементов собственных боксов нет,
/// BUG-488).
fn lay_vtt(html: &str, cues: &[CueRender]) -> (Document, LayoutBox) {
    let mut doc = lumen_html_parser::parse(html);
    let video = find_video_node(&doc);
    doc.set_vtt_cues(video, cues);
    let sheet = lumen_css_parser::parse("body{margin:0}");
    let root = layout(&doc, &sheet, Size::new(800.0, 600.0));
    (doc, body_layout_box(root))
}

/// Поиск элемента по предикату в поддереве `start` (обход DOM-детей).
fn find_dom_el(
    doc: &Document,
    start: NodeId,
    pred: &dyn Fn(&Document, NodeId) -> bool,
) -> Option<NodeId> {
    if pred(doc, start) {
        return Some(start);
    }
    for &c in &doc.get(start).children {
        if let Some(found) = find_dom_el(doc, c, pred) {
            return Some(found);
        }
    }
    None
}

/// Узел UA-тени видео с атрибутом `attr = value` (`data-lumen-vtt=bg`,
/// `data-lumen-vtt-type=b` — атрибуты ставит срез S3).
fn shadow_el_by_attr(doc: &Document, video: NodeId, attr: &str, value: &str) -> NodeId {
    let sr = doc.shadow_root_of(video).expect("video must carry a UA shadow root");
    find_dom_el(doc, sr, &|doc, id| doc.get(id).get_attr(attr) == Some(value))
        .expect("attribute-marked node missing in the cue shadow tree")
}

/// Рекурсивный поиск первого бокса по предикату — как [`first_video_child`],
/// но с произвольным условием.
fn find_box<'a>(b: &'a LayoutBox, pred: &dyn Fn(&LayoutBox) -> bool) -> Option<&'a LayoutBox> {
    if pred(b) {
        return Some(b);
    }
    for c in &b.children {
        if let Some(found) = find_box(c, pred) {
            return Some(found);
        }
    }
    None
}

fn first_video_child(b: &LayoutBox) -> &LayoutBox {
    find_box(b, &|b| matches!(b.kind, BoxKind::Video { .. })).expect("no <video> box")
}

fn approx(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.1
}

/// Видео 320×180 с одним cue по умолчанию (§7.1): ширина cue — вся область
/// видео, низ — вплотную к нижнему краю, шрифт контейнера — 9 px = 5 % от
/// 180 px (§7.4), фон-подложка — inline с rgba(0,0,0,0.8).
#[test]
fn vtt_cue_default_box_spans_video_bottom_with_9px_font() {
    let (doc, root) = lay_vtt(
        r#"<video style="width:320px;height:180px"></video>"#,
        &[cue(CueSettings::default(), "Hi")],
    );
    let vid = first_video_child(&root);
    // Единственный ребёнок video — контейнер cue (блок, поверх кадра).
    let container = &vid.children[0];
    assert!(
        matches!(container.kind, BoxKind::Block),
        "container kind = {:?}",
        container.kind
    );
    assert_eq!(container.style.position, Position::Relative);
    assert!(approx(container.rect.width, 320.0), "container w={}", container.rect.width);
    assert!(approx(container.rect.height, 180.0), "container h={}", container.rect.height);
    // §7.4: font-size = 5 % высоты видео (180 × 0.05 = 9), семейство и цвет.
    assert!(approx(container.style.font_size, 9.0), "fs={}", container.style.font_size);
    assert_eq!(container.style.font_family.first().map(String::as_str), Some("sans-serif"));
    assert_eq!(container.style.color, Color::WHITE);
    assert_eq!(container.style.overflow_x, crate::style::Overflow::Hidden);

    // Cue-бокс: absolute-геометрия из cue_box_css (default →
    // left:0%;width:100%;bottom:0lh), ширина — вся область, низ вплотную.
    let cue_box = &container.children[0];
    assert!(approx(cue_box.rect.width, 320.0), "cue w={}", cue_box.rect.width);
    let cue_bottom = cue_box.rect.y + cue_box.rect.height;
    assert!(
        approx(cue_bottom, vid.rect.y + 180.0),
        "cue bottom={} vs video bottom={}",
        cue_bottom,
        vid.rect.y + 180.0
    );
    // §7.4: типографика cue-бокса.
    assert_eq!(cue_box.style.unicode_bidi, UnicodeBidi::Plaintext);
    assert_eq!(cue_box.style.overflow_wrap, OverflowWrap::BreakWord);
    assert_eq!(cue_box.style.white_space, WhiteSpace::PreLine);
    assert_eq!(cue_box.style.display, Display::Block);

    // Фон-подложка — inline-узел с rgba(0,0,0,0.8); у inline-элементов
    // собственных боксов нет, стиль снимается снапшот-коллектором (BUG-488).
    let video_nid = find_video_node(&doc);
    let bg = shadow_el_by_attr(&doc, video_nid, "data-lumen-vtt", "bg");
    let styles = collect_computed_styles(&root, &doc, None, Size::new(800.0, 600.0));
    let bg_style = styles.get(&(bg.index() as u32)).expect("bg span style");
    assert_eq!(bg_style.get("display").map(String::as_str), Some("inline"));
    assert_eq!(
        bg_style.get("background-color").map(String::as_str),
        Some("rgba(0, 0, 0, 0.8)"),
        "bg = {:?}",
        bg_style.get("background-color")
    );
}

/// `size:50%` → ширина 160, центр по строке: x = video.x + 80
/// (cue_box_css: `left:25%;width:50%` при position auto 50 %).
#[test]
fn vtt_cue_size_half_is_half_width_centered() {
    let s = CueSettings {
        size: 50.0,
        ..Default::default()
    };
    let (_, root) = lay_vtt(
        r#"<video style="width:320px;height:180px"></video>"#,
        &[cue(s, "Hi")],
    );
    let vid = first_video_child(&root);
    let container = &vid.children[0];
    let cue_box = &container.children[0];
    assert!(approx(cue_box.rect.width, 160.0), "cue w={}", cue_box.rect.width);
    assert!(
        approx(cue_box.rect.x, vid.rect.x + 80.0),
        "cue x={} vs video x={}",
        cue_box.rect.x,
        vid.rect.x
    );
}

/// `line:0` → cue прижат к верхнему краю области видео (`top:0lh`).
#[test]
fn vtt_cue_line_zero_sits_at_video_top() {
    let s = CueSettings {
        line: CueLine::Number(0.0),
        ..Default::default()
    };
    let (_, root) = lay_vtt(
        r#"<video style="width:320px;height:180px"></video>"#,
        &[cue(s, "Hi")],
    );
    let vid = first_video_child(&root);
    let cue_box = &vid.children[0].children[0];
    assert!(
        approx(cue_box.rect.y, vid.rect.y),
        "cue y={} vs video y={}",
        cue_box.rect.y,
        vid.rect.y
    );
}

/// Видео без cue: детей нет — поведение прежнее (§S4: контейнер cue —
/// единственный источник детей `<video>`).
#[test]
fn vtt_cue_video_without_cues_has_no_children() {
    let root = lay(r#"<video style="width:320px;height:180px"></video>"#, "");
    let vid = first_video_child(&root);
    assert!(
        vid.children.is_empty(),
        "video without cues must stay childless, got {}",
        vid.children.len()
    );
}

/// §7.4: узлы cue text — `b` жирным, `i` курсивом, `u` с подчёркиванием
/// (снимок стилей через [`collect_computed_styles`]: у inline-узлов
/// собственных боксов нет).
#[test]
fn vtt_cue_text_tags_get_bold_italic_underline() {
    let (doc, root) = lay_vtt(
        r#"<video style="width:320px;height:180px"></video>"#,
        &[cue(CueSettings::default(), "<b>bo</b><i>it</i><u>un</u>")],
    );
    let video_nid = find_video_node(&doc);
    let styles = collect_computed_styles(&root, &doc, None, Size::new(800.0, 600.0));
    let style_of = |typ: &str, prop: &str| -> Option<String> {
        let nid = shadow_el_by_attr(&doc, video_nid, "data-lumen-vtt-type", typ);
        styles
            .get(&(nid.index() as u32))
            .unwrap_or_else(|| panic!("no style snapshot for cue <{typ}>"))
            .get(prop)
            .cloned()
    };
    assert_eq!(style_of("b", "font-weight").as_deref(), Some("700"));
    assert_eq!(style_of("i", "font-style").as_deref(), Some("italic"));
    assert_eq!(
        style_of("u", "text-decoration-line").as_deref(),
        Some("underline"),
        "u = {:?}",
        style_of("u", "text-decoration-line")
    );
}
