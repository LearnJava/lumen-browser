//! OBJECT-1: `<object data>`/`<embed src>`, чей ресурс декодировался как
//! картинка, — replaced-бокс изображения; иначе `<object>` показывает
//! fallback-потомков (HTML LS §4.8.6/§4.8.7). Срез 2: ресурс-документ —
//! бокс вложенного документа, как у `<iframe>`.

use lumen_core::geom::Size;
use lumen_dom::{Document, NodeId};

use super::super::{apply_intrinsic_size, collect_image_requests, layout, BoxKind, LayoutBox};

const VP: Size = Size { width: 800.0, height: 600.0 };

fn find_tag(doc: &Document, id: NodeId, tag: &str) -> Option<NodeId> {
    let node = doc.get(id);
    if node.element_name().is_some_and(|n| n.local.as_str() == tag) {
        return Some(id);
    }
    node.children.iter().find_map(|&c| find_tag(doc, c, tag))
}

fn find_image(b: &LayoutBox) -> Option<&LayoutBox> {
    if matches!(b.kind, BoxKind::Image { .. }) {
        return Some(b);
    }
    b.children.iter().find_map(find_image)
}

fn has_text(b: &LayoutBox, needle: &str) -> bool {
    if let BoxKind::InlineRun { segments, .. } = &b.kind
        && segments.iter().any(|s| s.text.contains(needle))
    {
        return true;
    }
    b.children.iter().any(|c| has_text(c, needle))
}

#[test]
fn object_and_embed_request_their_resource_as_embedded_content() {
    let doc = lumen_html_parser::parse(
        r#"<object data="logo.svg">fb</object><embed src="icon.png"><img src="a.png">"#,
    );
    let reqs = collect_image_requests(&doc, VP);
    let urls: Vec<(&str, bool)> = reqs.iter().map(|r| (r.url.as_str(), r.embedded_content)).collect();
    assert_eq!(urls, vec![("logo.svg", true), ("icon.png", true), ("a.png", false)]);
}

#[test]
fn explicit_non_image_type_is_not_requested_as_image() {
    let doc = lumen_html_parser::parse(
        r#"<object data="page.html" type="text/html"></object><object data="x.svg" type="image/svg+xml"></object><embed src="doc.pdf" type="application/pdf"><object>no data</object>"#,
    );
    let urls: Vec<String> = collect_image_requests(&doc, VP).into_iter().map(|r| r.url).collect();
    assert_eq!(urls, vec!["x.svg".to_string()]);
}

#[test]
fn object_without_decoded_image_renders_fallback_children() {
    let doc = lumen_html_parser::parse(r#"<object data="logo.svg">fallback text</object>"#);
    let root = layout(&doc, &lumen_css_parser::parse(""), VP);
    assert!(find_image(&root).is_none(), "no decoded content yet → no image box");
    assert!(has_text(&root, "fallback text"), "fallback children must be laid out");
}

#[test]
fn object_with_decoded_image_is_an_image_box_at_intrinsic_size() {
    let mut doc = lumen_html_parser::parse(r#"<object data="logo.svg">fallback text</object>"#);
    let obj = find_tag(&doc, doc.root(), "object").unwrap();
    assert!(apply_intrinsic_size(&mut doc, obj, 120, 40));
    // The size goes to the side table, never into reflected attributes.
    assert_eq!(doc.get(obj).get_attr("width"), None);
    assert_eq!(doc.get(obj).get_attr("height"), None);
    assert!(!apply_intrinsic_size(&mut doc, obj, 120, 40), "second report is a no-op");

    let root = layout(&doc, &lumen_css_parser::parse(""), VP);
    let img = find_image(&root).expect("object with decoded image must be an Image box");
    match &img.kind {
        BoxKind::Image { src, .. } => assert_eq!(src, "logo.svg"),
        _ => unreachable!(),
    }
    assert_eq!((img.rect.width, img.rect.height), (120.0, 40.0));
    assert!(!has_text(&root, "fallback text"), "fallback must not render next to the content");
}

#[test]
fn object_width_attribute_scales_by_intrinsic_ratio() {
    let mut doc = lumen_html_parser::parse(r#"<object data="logo.svg" width="60"></object>"#);
    let obj = find_tag(&doc, doc.root(), "object").unwrap();
    apply_intrinsic_size(&mut doc, obj, 120, 40);
    let root = layout(&doc, &lumen_css_parser::parse(""), VP);
    let img = find_image(&root).unwrap();
    assert_eq!((img.rect.width, img.rect.height), (60.0, 20.0));
}

#[test]
fn changed_data_url_drops_stale_image_until_new_resource_reports() {
    let mut doc = lumen_html_parser::parse(r#"<object data="old.svg">fallback text</object>"#);
    let obj = find_tag(&doc, doc.root(), "object").unwrap();
    apply_intrinsic_size(&mut doc, obj, 10, 10);
    if let lumen_dom::NodeData::Element { attrs, .. } = &mut doc.get_mut(obj).data {
        for a in attrs.iter_mut().filter(|a| a.name.local.as_str() == "data") {
            a.value = "new.svg".into();
        }
    }
    let root = layout(&doc, &lumen_css_parser::parse(""), VP);
    assert!(find_image(&root).is_none());
    assert!(has_text(&root, "fallback text"));
}

#[test]
fn embed_with_decoded_image_is_an_image_box() {
    let mut doc = lumen_html_parser::parse(r#"<embed src="icon.png">"#);
    let emb = find_tag(&doc, doc.root(), "embed").unwrap();
    apply_intrinsic_size(&mut doc, emb, 32, 16);
    let root = layout(&doc, &lumen_css_parser::parse(""), VP);
    let img = find_image(&root).expect("embed with decoded image must be an Image box");
    assert_eq!((img.rect.width, img.rect.height), (32.0, 16.0));
}

fn find_iframe(b: &LayoutBox) -> Option<&LayoutBox> {
    if matches!(b.kind, BoxKind::Iframe { .. }) {
        return Some(b);
    }
    b.children.iter().find_map(find_iframe)
}

/// OBJECT-1 срез 2: ресурс, который шелл признал документом, делает
/// `<object>` боксом вложенного документа — тем же `BoxKind::Iframe`, что у
/// `<iframe>`, с UA-размером 300×150 и адресом `data` (ключ заглушки, на место
/// которой шелл вклеивает содержимое фрейма). Fallback не раскладывается.
#[test]
fn object_with_embedded_document_is_iframe_box_without_fallback() {
    let mut doc = lumen_html_parser::parse(r#"<object data="page.html">fallback text</object>"#);
    let obj = find_tag(&doc, doc.root(), "object").expect("object");
    assert!(doc.set_embedded_document(obj, "page.html", true));
    let root = layout(&doc, &lumen_css_parser::parse(""), VP);
    let frame = find_iframe(&root).expect("<object> с документом — бокс фрейма");
    assert!(matches!(&frame.kind, BoxKind::Iframe { src, .. } if src == "page.html"));
    assert!((frame.rect.width - 300.0).abs() < 0.5 && (frame.rect.height - 150.0).abs() < 0.5, "{:?}", frame.rect);
    assert!(!has_text(&root, "fallback text"), "fallback не раскладывается");
}

/// Вердикт «не документ» и вердикт для прежнего `data` оставляют fallback.
#[test]
fn object_not_document_or_stale_verdict_keeps_fallback() {
    let mut doc = lumen_html_parser::parse(
        r#"<object data="a.pdf">fallback one</object><embed src="new.html">"#,
    );
    let obj = find_tag(&doc, doc.root(), "object").expect("object");
    let embed = find_tag(&doc, doc.root(), "embed").expect("embed");
    doc.set_embedded_document(obj, "a.pdf", false);
    doc.set_embedded_document(embed, "old.html", true);
    let root = layout(&doc, &lumen_css_parser::parse(""), VP);
    assert!(find_iframe(&root).is_none());
    assert!(has_text(&root, "fallback one"));
}

/// `width`/`height` у `<embed>` с документом — размер бокса фрейма.
#[test]
fn embed_with_embedded_document_honours_dimension_attributes() {
    let mut doc = lumen_html_parser::parse(r#"<embed src="p.html" width="120" height="40">"#);
    let embed = find_tag(&doc, doc.root(), "embed").expect("embed");
    doc.set_embedded_document(embed, "p.html", true);
    let root = layout(&doc, &lumen_css_parser::parse(""), VP);
    let frame = find_iframe(&root).expect("бокс фрейма");
    assert!((frame.rect.width - 120.0).abs() < 0.5 && (frame.rect.height - 40.0).abs() < 0.5, "{:?}", frame.rect);
}
