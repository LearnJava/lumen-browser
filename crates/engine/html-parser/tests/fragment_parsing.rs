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

use lumen_dom::{Document, NodeData, NodeId};

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
