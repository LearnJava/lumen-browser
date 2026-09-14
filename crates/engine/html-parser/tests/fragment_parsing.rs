//! HTML LS §13.4 «Parsing HTML fragments» — вход [`parse_fragment`] (BUG-982).
//!
//! Смысл набора — зафиксировать ровно ту границу, на которой документный и
//! фрагментный разбор обязаны разойтись. Документный обязан игнорировать
//! ведущий whitespace и уносить ведущий комментарий в сам `Document`
//! (§13.2.6.4.1–4); фрагментный обязан сохранить оба, потому что стартует
//! сразу в `in body`. До BUG-982 `innerHTML` ходил документным парсером и
//! забирал детей `<body>`, из-за чего оба узла бесследно пропадали — на этом
//! ломалась гидрация React 18, чьи маркеры Suspense `<!--$-->` стоят как раз
//! в начале фрагмента.

#![allow(clippy::unwrap_used)]

use lumen_dom::{Document, Namespace, NodeData, NodeId};
use lumen_html_parser::FragmentContext;

/// Плоское представление верхнего уровня фрагмента: по одной записи на
/// ребёнка корня, вложенность — в скобках.
fn shape(src: &str) -> Vec<String> {
    let (doc, root) = lumen_html_parser::parse_fragment(src);
    doc.get(root)
        .children
        .iter()
        .map(|&c| describe(&doc, c))
        .collect()
}

/// Тот же [`shape`], но с реальным контекстным элементом (GAP-XMLDOC срез
/// 14, BUG-685) — `namespace`/`local` того элемента, на который вызван
/// `Element.innerHTML=`.
fn shape_with_context(src: &str, namespace: Namespace, local: &str) -> Vec<String> {
    let context = FragmentContext {
        namespace,
        local: local.to_string(),
        attrs: Vec::new(),
    };
    let (doc, root) = lumen_html_parser::parse_fragment_with_context(src, Some(context));
    doc.get(root)
        .children
        .iter()
        .map(|&c| describe(&doc, c))
        .collect()
}

fn describe(doc: &Document, id: NodeId) -> String {
    match &doc.get(id).data {
        NodeData::Element { name, .. } => {
            let kids: Vec<String> = doc
                .get(id)
                .children
                .iter()
                .map(|&c| describe(doc, c))
                .collect();
            if kids.is_empty() {
                format!("<{}>", name.local)
            } else {
                format!("<{}>({})", name.local, kids.join(","))
            }
        }
        NodeData::Text(s) => format!("#text{s:?}"),
        NodeData::Comment(s) => format!("#comment{s:?}"),
        other => format!("{other:?}"),
    }
}

#[test]
fn leading_whitespace_survives() {
    assert_eq!(shape(" abc"), ["#text\" abc\""]);
}

#[test]
fn whitespace_only_fragment_is_a_text_node() {
    // Самый жёсткий случай: документный парсер терял такой фрагмент целиком —
    // whitespace-only токен в `initial` игнорируется, забирать было нечего.
    assert_eq!(shape(" "), ["#text\" \""]);
    assert_eq!(shape("\n  "), ["#text\"\\n  \""]);
}

#[test]
fn leading_whitespace_is_one_node_with_the_text_that_follows() {
    // Регресс-ловушка на «дешёвый» вариант правки (собрать детей `<head>` и
    // `<body>` документного разбора): там пробел оседал в `<head>` отдельным
    // узлом и склейки не происходило — `textContent` совпал бы, а
    // `childNodes.length` был бы 2.
    assert_eq!(shape(" abc"), ["#text\" abc\""]);
    assert_eq!(shape("\n  <div>d</div>"), ["#text\"\\n  \"", "<div>(#text\"d\")"]);
}

#[test]
fn leading_comment_survives() {
    // Маркер границы Suspense у React 18 — ровно эта форма.
    assert_eq!(shape("<!--$-->x"), ["#comment\"$\"", "#text\"x\""]);
    assert_eq!(shape("<!--$-->"), ["#comment\"$\""]);
    assert_eq!(
        shape("<!--a--><div>d</div><!--b-->"),
        ["#comment\"a\"", "<div>(#text\"d\")", "#comment\"b\""]
    );
}

#[test]
fn trailing_and_interior_whitespace_unchanged() {
    // Эти случаи работали и до правки — тест держит их на месте.
    assert_eq!(shape("abc "), ["#text\"abc \""]);
    assert_eq!(shape("a  b"), ["#text\"a  b\""]);
    assert_eq!(shape("<b> x</b>"), ["<b>(#text\" x\")"]);
}

#[test]
fn no_page_skeleton_leaks_into_the_fragment() {
    // §13.4 не достраивает `<head>`/`<body>`: у фрагмента их нет. Ловит две
    // конкретные утечки — EOF-догон каркаса и «reset the insertion mode»,
    // который на синтетическом корне `<html>` без head-указателя уводил в
    // `before head`.
    assert_eq!(
        shape("<table><tr><td>c</td></tr></table>"),
        ["<table>(<tbody>(<tr>(<td>(#text\"c\"))))"]
    );
    assert_eq!(shape("<template>t</template>"), ["<template>"]);
    assert!(shape("").is_empty());
}

#[test]
fn head_only_elements_stay_in_the_fragment() {
    // Документный парсер прятал их в `<head>`, и фрагмент выходил пустым.
    assert_eq!(shape("<link rel=x>"), ["<link>"]);
    assert_eq!(shape("<meta charset=utf-8>"), ["<meta>"]);
    assert_eq!(shape("<title>t</title>"), ["<title>(#text\"t\")"]);
    assert_eq!(shape("<style>b{}</style>"), ["<style>(#text\"b{}\")"]);
}

#[test]
fn stray_head_tag_is_ignored_but_its_content_is_not() {
    // §13.2.6.4.7 «in body», start tag `head` — parse error, ignore.
    assert_eq!(shape("<head>x</head>"), ["#text\"x\""]);
}

#[test]
fn document_parser_still_swallows_what_the_spec_tells_it_to() {
    // Обратная сторона границы: правка не должна была тронуть `parse`.
    let doc = lumen_html_parser::parse(" abc");
    let body = doc.body().unwrap();
    assert_eq!(
        doc.get(body)
            .children
            .iter()
            .map(|&c| describe(&doc, c))
            .collect::<Vec<_>>(),
        ["#text\"abc\""]
    );
    let doc = lumen_html_parser::parse("<!--$-->x");
    // Комментарий из `initial` — ребёнок самого `Document`, не `<body>`.
    assert!(
        doc.get(doc.root())
            .children
            .iter()
            .any(|&c| matches!(&doc.get(c).data, NodeData::Comment(s) if s == "$"))
    );
}

/// HTML LS §13.2.6.5 "adjusted current node" for the fragment case
/// (GAP-XMLDOC срез 14, BUG-685) — WPT
/// `html/syntax/parsing/cdata-in-integration-point-fragment.html`. A MathML
/// text integration point context (`<mi>`/`<mo>`/`<mn>`/`<ms>`/`<mtext>`)
/// forbids CDATA sections just like plain HTML: `x<![CDATA[y]]>` becomes a
/// text node `"x"` plus a bogus comment, `y` never becomes character data.
#[test]
fn mathml_text_integration_point_context_disallows_cdata() {
    for tag in ["mi", "mo", "mn", "ms", "mtext"] {
        let shape = shape_with_context("x<![CDATA[y]]>", Namespace::MathMl, tag);
        assert_eq!(shape.len(), 2, "context <{tag}>: expected text + comment, got {shape:?}");
        assert_eq!(shape[0], "#text\"x\"", "context <{tag}>");
        assert!(shape[1].starts_with("#comment"), "context <{tag}>: expected a comment, got {}", shape[1]);
    }
}

/// Same WPT test, SVG HTML integration points (`<foreignObject>`/`<desc>`/
/// `<title>`) — also disallow CDATA.
#[test]
fn svg_html_integration_point_context_disallows_cdata() {
    for tag in ["foreignObject", "desc", "title"] {
        let shape = shape_with_context("x<![CDATA[y]]>", Namespace::Svg, tag);
        assert_eq!(shape.len(), 2, "context <{tag}>: expected text + comment, got {shape:?}");
        assert_eq!(shape[0], "#text\"x\"", "context <{tag}>");
        assert!(shape[1].starts_with("#comment"), "context <{tag}>: expected a comment, got {}", shape[1]);
    }
}

/// Same WPT test's control case: a non-integration-point SVG context
/// (`<path>`) is genuine foreign content — CDATA IS allowed, `y` becomes
/// character data merged with the preceding `x`.
#[test]
fn non_integration_point_svg_context_allows_cdata() {
    let shape = shape_with_context("x<![CDATA[y]]>", Namespace::Svg, "path");
    assert_eq!(shape, ["#text\"xy\""]);
}

/// WPT `html/syntax/parsing/html_content_in_foreign_context.html` — an HTML
/// LS §13.2.6.5 breakout tag (`<b>`) opened right after a foreign
/// `<svg>`/`<math>` inside `Element.innerHTML=` must exit back to HTML
/// content, landing as the context's own child, not `<svg>`'s. This is the
/// ordinary (non-context) foreign-content breakout srez 3/6 already cover —
/// the regression this guards is specific to the fragment-parsing entry
/// point: `dispatch_foreign_content`'s "pop while foreign" loop must never
/// walk past the synthetic fragment root even though [`current_namespace`]
/// reports the FOREIGN adjusted current node while the stack still holds
/// only that root (GAP-XMLDOC срез 14, BUG-685).
#[test]
fn html_breakout_tag_exits_svg_opened_inside_fragment() {
    for (context_ns, context_local) in [(Namespace::Html, "div"), (Namespace::Svg, "foreignObject")] {
        let shape = shape_with_context("<svg><b>x</svg>", context_ns, context_local);
        assert_eq!(shape.len(), 2, "context <{context_local}>: expected <svg> + <b>, got {shape:?}");
        assert_eq!(shape[0], "<svg>", "context <{context_local}>");
        assert_eq!(shape[1], "<b>(#text\"x\")", "context <{context_local}>");
    }
}

/// Same WPT test, the two bare-end-tag element ids (`"/p"`/`"/br"`) — a
/// `</p>`/`</br>` with nothing open to close it still exits `<svg>` and
/// produces a genuine element as the wrapper's own child, same as its
/// start-tag counterpart. Regression for the fix's first (too broad) shape,
/// which made ANY unmatched end tag reaching an HTML-namespace boundary
/// fall through and pop — that silently destroyed still-open, unrelated
/// foreign elements too (see `svg_script_with_bogus_end_tag_inside_stays_executable`
/// in `foreign_script_execution.rs`).
#[test]
fn breakout_end_tag_exits_svg_with_no_matching_open_element() {
    for (tag, expected) in [("p", "<p>"), ("br", "<br>")] {
        let shape = shape_with_context(&format!("<svg></{tag}></svg"), Namespace::Html, "div");
        assert_eq!(shape, ["<svg>", expected], "</{tag}> inside <svg>");
    }
}

/// A bare, genuinely unrecognized end tag (`</g>`) inside foreign content
/// must NOT exit anything — it's simply ignored (HTML LS §13.2.6.5 "any
/// other end tag", no match anywhere on the stack).
#[test]
fn bogus_end_tag_inside_svg_is_ignored() {
    let shape = shape_with_context("<svg><g></g></svg>x", Namespace::Html, "div");
    assert_eq!(shape, ["<svg>(<g>)", "#text\"x\""]);
}
