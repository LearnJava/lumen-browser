//! Tree builder (Phase 0 — lenient).
//!
//! Простой стековый построитель. Не реализует insertion modes из HTML5
//! spec (in_table / in_select / реструктуризация foster parent и т.д.) —
//! этого достаточно для текстового веба и большинства простых страниц.
//! При несовпадении закрывающего тега молча игнорирует.

use lumen_dom::{Attribute, Document, NodeData, NodeId, QualName};

use crate::tokenizer::{Token, Tokenizer};

pub fn parse(input: &str) -> Document {
    let mut doc = Document::new();
    let mut stack: Vec<NodeId> = vec![doc.root()];

    for token in Tokenizer::new(input) {
        match token {
            Token::StartTag {
                name,
                attrs,
                self_closing,
            } => {
                let elem = doc.create_element(QualName::html(name.clone()));
                if let NodeData::Element {
                    attrs: dom_attrs, ..
                } = &mut doc.get_mut(elem).data
                {
                    for (k, v) in attrs {
                        dom_attrs.push(Attribute {
                            name: QualName::html(k),
                            value: v,
                        });
                    }
                }
                let parent = *stack.last().expect("stack always non-empty");
                doc.append_child(parent, elem);
                if !self_closing && !is_void_element(&name) {
                    stack.push(elem);
                }
            }
            Token::EndTag { name } => {
                let matched = stack.iter().enumerate().rev().find_map(|(idx, &id)| {
                    if let NodeData::Element { name: n, .. } = &doc.get(id).data {
                        (n.local == name).then_some(idx)
                    } else {
                        None
                    }
                });
                if let Some(idx) = matched {
                    stack.truncate(idx);
                }
            }
            Token::Text(s) => {
                if !s.is_empty() {
                    let text = doc.create_text(s);
                    let parent = *stack.last().expect("stack always non-empty");
                    doc.append_child(parent, text);
                }
            }
            Token::Comment(s) => {
                let comment = doc.create_comment(s);
                let parent = *stack.last().expect("stack always non-empty");
                doc.append_child(parent, comment);
            }
        }
    }

    doc
}

fn is_void_element(name: &str) -> bool {
    matches!(
        name,
        "area"
            | "base"
            | "br"
            | "col"
            | "embed"
            | "hr"
            | "img"
            | "input"
            | "link"
            | "meta"
            | "param"
            | "source"
            | "track"
            | "wbr"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input() {
        let doc = parse("");
        assert_eq!(doc.len(), 1); // only root
    }

    #[test]
    fn simple_hello() {
        let doc = parse("<p>hello</p>");
        let s = doc.to_string();
        assert!(s.contains("<p>"));
        assert!(s.contains("\"hello\""));
    }

    #[test]
    fn cyrillic_in_h1() {
        let doc = parse("<html><body><h1>Привет, мир</h1></body></html>");
        let s = doc.to_string();
        assert!(s.contains("<html>"));
        assert!(s.contains("<body>"));
        assert!(s.contains("<h1>"));
        assert!(s.contains("\"Привет, мир\""));
    }

    #[test]
    fn attributes_preserved() {
        let doc = parse(r#"<a href="https://example.com">link</a>"#);
        let s = doc.to_string();
        assert!(s.contains(r#"<a href="https://example.com">"#));
    }

    #[test]
    fn void_element_does_not_consume_parent() {
        let doc = parse("<p>a<br>b</p>");
        let s = doc.to_string();
        // <br> не должен «съесть» <p> — текст 'b' остаётся внутри <p>
        let p_pos = s.find("<p>").unwrap();
        let p_close_pos = s.rfind("\"b\"").unwrap();
        assert!(p_close_pos > p_pos);
        assert!(s.contains("<br>"));
    }

    #[test]
    fn self_closing_tag() {
        let doc = parse("<img src=\"x.png\"/><p>after</p>");
        let s = doc.to_string();
        assert!(s.contains(r#"<img src="x.png">"#));
        assert!(s.contains("<p>"));
        assert!(s.contains("\"after\""));
    }

    #[test]
    fn comment_preserved() {
        let doc = parse("<p><!-- note -->text</p>");
        let s = doc.to_string();
        assert!(s.contains("<!-- note -->"));
        assert!(s.contains("\"text\""));
    }

    #[test]
    fn doctype_ignored_content_kept() {
        let doc = parse("<!DOCTYPE html><p>x</p>");
        let s = doc.to_string();
        assert!(s.contains("<p>"));
        assert!(s.contains("\"x\""));
    }

    #[test]
    fn unclosed_tag_recovered() {
        // <p> без </p>: парсер просто оставляет его открытым
        let doc = parse("<p>hello");
        let s = doc.to_string();
        assert!(s.contains("<p>"));
        assert!(s.contains("\"hello\""));
    }

    #[test]
    fn mismatched_end_tag_ignored() {
        // </div> без открывающего — игнорируем
        let doc = parse("<p>x</div></p>");
        let s = doc.to_string();
        assert!(s.contains("<p>"));
        assert!(s.contains("\"x\""));
    }

    #[test]
    fn entity_in_text() {
        let doc = parse("<p>a &amp; b &lt; c</p>");
        let s = doc.to_string();
        assert!(s.contains("\"a & b < c\""));
    }
}
