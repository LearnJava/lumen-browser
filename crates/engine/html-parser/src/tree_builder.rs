//! Tree builder (Phase 0 — lenient).
//!
//! Простой стековый построитель. Не реализует insertion modes из HTML5
//! spec (in_table / in_select / реструктуризация foster parent и т.д.) —
//! этого достаточно для текстового веба и большинства простых страниц.
//! При несовпадении закрывающего тега молча игнорирует.

use lumen_dom::{Attribute, Document, NodeData, NodeId, QualName};

use crate::tokenizer::{Token, Tokenizer};

pub fn parse(input: &str) -> Document {
    use lumen_dom::DocumentMode;

    let mut doc = Document::new();
    let mut stack: Vec<NodeId> = vec![doc.root()];
    // По §13.2.6.4.1 «The initial insertion mode»: при отсутствии DOCTYPE
    // документ становится Quirks. Document::new() по умолчанию NoQuirks
    // (для программного использования и unit-тестов), поэтому fallback
    // ставим в конце функции, если ни одного DOCTYPE-токена не было.
    let mut seen_doctype = false;

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
            Token::Doctype {
                name,
                public_id,
                system_id,
            } => {
                // По HTML5 spec DOCTYPE до <html> идёт прямо в Document.
                // Без insertion modes мы кладём его туда, где сейчас стек —
                // обычно тоже Document. Этого достаточно для рендеринга.
                //
                // Установить mode документа по §13.2.5.1 («The initial
                // insertion mode»). Только первый DOCTYPE влияет — у нас
                // нет insertion modes, но сторонние DOCTYPE-ы в середине
                // документа — синтаксическая ошибка, и spec их игнорирует.
                if !seen_doctype {
                    doc.set_mode(crate::quirks_mode::detect_document_mode(
                        &name,
                        public_id.as_deref(),
                        system_id.as_deref(),
                    ));
                    seen_doctype = true;
                }
                let dt = doc.create_doctype(
                    name,
                    public_id.unwrap_or_default(),
                    system_id.unwrap_or_default(),
                );
                let parent = *stack.last().expect("stack always non-empty");
                doc.append_child(parent, dt);
            }
        }
    }

    if !seen_doctype {
        doc.set_mode(DocumentMode::Quirks);
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
    fn doctype_creates_node_and_keeps_content() {
        let doc = parse("<!DOCTYPE html><p>x</p>");
        let s = doc.to_string();
        // Doctype node теперь создаётся (раньше токен пропускался).
        assert!(s.contains("<!DOCTYPE html>"), "doctype line missing: {s}");
        assert!(s.contains("<p>"));
        assert!(s.contains("\"x\""));
    }

    #[test]
    fn doctype_node_data_preserved() {
        // Прямая проверка NodeData::Doctype с public/system_id.
        let doc = parse(r#"<!DOCTYPE html PUBLIC "pid" "sid"><p>x</p>"#);
        let root = doc.get(doc.root());
        let dt_id = root.children[0];
        let dt_node = doc.get(dt_id);
        match &dt_node.data {
            NodeData::Doctype { name, public_id, system_id } => {
                assert_eq!(name, "html");
                assert_eq!(public_id, "pid");
                assert_eq!(system_id, "sid");
            }
            other => panic!("expected Doctype, got {other:?}"),
        }
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

    #[test]
    fn script_body_is_single_text_node() {
        // RAWTEXT: тело <script> попадает в DOM одним текстовым узлом
        // с исходными байтами — без интерпретации <b>, <, &amp;.
        let doc = parse("<script>var x = '<b>&amp;</b>'; if (a<b) {}</script>");
        let root = doc.root();
        let script = doc.get(root).children[0];
        match &doc.get(script).data {
            NodeData::Element { name, .. } => assert_eq!(name.local, "script"),
            other => panic!("expected script element, got {other:?}"),
        }
        let kids = &doc.get(script).children;
        assert_eq!(kids.len(), 1, "script must have a single text child, got {kids:?}");
        match &doc.get(kids[0]).data {
            NodeData::Text(s) => {
                assert_eq!(s, "var x = '<b>&amp;</b>'; if (a<b) {}");
            }
            other => panic!("expected text node, got {other:?}"),
        }
    }

    #[test]
    fn style_body_is_single_text_node() {
        let doc = parse("<style>p::before { content: '&'; } /* < */</style>");
        let s = doc.to_string();
        assert!(s.contains("\"p::before { content: '&'; } /* < */\""));
    }

    #[test]
    fn script_then_normal_content() {
        // После </script> токенизатор возвращается в нормальный режим.
        let doc = parse("<script>x<1</script><p>after</p>");
        let s = doc.to_string();
        assert!(s.contains("\"x<1\""));
        assert!(s.contains("<p>"));
        assert!(s.contains("\"after\""));
    }

    #[test]
    fn title_body_is_decoded_text_node() {
        // RCDATA: <title> entities декодируются, угловые скобки буквальны.
        let doc = parse("<title>Foo &amp; <b>Bar</b></title>");
        let root = doc.root();
        let title = doc.get(root).children[0];
        match &doc.get(title).data {
            NodeData::Element { name, .. } => assert_eq!(name.local, "title"),
            other => panic!("expected title element, got {other:?}"),
        }
        let kids = &doc.get(title).children;
        assert_eq!(kids.len(), 1, "title must have a single text child, got {kids:?}");
        match &doc.get(kids[0]).data {
            NodeData::Text(s) => assert_eq!(s, "Foo & <b>Bar</b>"),
            other => panic!("expected text node, got {other:?}"),
        }
    }

    #[test]
    fn textarea_body_is_decoded_text_node() {
        // RCDATA: <textarea> с XSS-подобным содержимым — entities
        // декодируются, но тело не парсится как HTML.
        let doc = parse("<textarea>&lt;script&gt;alert(1)&lt;/script&gt;</textarea>");
        let root = doc.root();
        let ta = doc.get(root).children[0];
        let kids = &doc.get(ta).children;
        assert_eq!(kids.len(), 1);
        match &doc.get(kids[0]).data {
            NodeData::Text(s) => assert_eq!(s, "<script>alert(1)</script>"),
            other => panic!("expected text node, got {other:?}"),
        }
    }

    // ──────── DocumentMode integration ────────

    #[test]
    fn html5_doctype_yields_no_quirks() {
        let doc = parse("<!DOCTYPE html><p>x</p>");
        assert_eq!(doc.mode(), lumen_dom::DocumentMode::NoQuirks);
    }

    #[test]
    fn no_doctype_yields_quirks() {
        // По §13.2.6.4.1 — отсутствие DOCTYPE-токена даёт quirks.
        let doc = parse("<p>x</p>");
        assert_eq!(doc.mode(), lumen_dom::DocumentMode::Quirks);
    }

    #[test]
    fn empty_input_yields_quirks() {
        // Пустой ввод — никаких DOCTYPE-токенов, режим quirks.
        let doc = parse("");
        assert_eq!(doc.mode(), lumen_dom::DocumentMode::Quirks);
    }

    #[test]
    fn html4_strict_with_system_yields_no_quirks() {
        let doc = parse(
            r#"<!DOCTYPE HTML PUBLIC "-//W3C//DTD HTML 4.01//EN" "http://www.w3.org/TR/html4/strict.dtd"><p>x</p>"#,
        );
        assert_eq!(doc.mode(), lumen_dom::DocumentMode::NoQuirks);
    }

    #[test]
    fn html4_transitional_with_system_yields_limited_quirks() {
        let doc = parse(
            r#"<!DOCTYPE HTML PUBLIC "-//W3C//DTD HTML 4.01 Transitional//EN" "http://www.w3.org/TR/html4/loose.dtd"><p>x</p>"#,
        );
        assert_eq!(doc.mode(), lumen_dom::DocumentMode::LimitedQuirks);
    }

    #[test]
    fn html4_transitional_without_system_yields_quirks() {
        let doc = parse(r#"<!DOCTYPE HTML PUBLIC "-//W3C//DTD HTML 4.01 Transitional//EN"><p>x</p>"#);
        assert_eq!(doc.mode(), lumen_dom::DocumentMode::Quirks);
    }

    #[test]
    fn xhtml_transitional_yields_limited_quirks() {
        // XHTML 1.0 Transitional — limited-quirks даже без system_id.
        let doc = parse(
            r#"<!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.0 Transitional//EN" "http://www.w3.org/TR/xhtml1/DTD/xhtml1-transitional.dtd"><p>x</p>"#,
        );
        assert_eq!(doc.mode(), lumen_dom::DocumentMode::LimitedQuirks);
    }

    #[test]
    fn html_3_2_doctype_yields_quirks() {
        let doc = parse(r#"<!DOCTYPE HTML PUBLIC "-//W3C//DTD HTML 3.2 Final//EN"><body>x</body>"#);
        assert_eq!(doc.mode(), lumen_dom::DocumentMode::Quirks);
    }

    #[test]
    fn only_first_doctype_sets_mode() {
        // Второй DOCTYPE-токен — синтаксическая ошибка по spec, игнорится.
        // Первый — `<!DOCTYPE html>` → no-quirks. Второй (HTML 3.2) не
        // переключает на quirks.
        let doc = parse(
            r#"<!DOCTYPE html><p>x</p><!DOCTYPE HTML PUBLIC "-//W3C//DTD HTML 3.2 Final//EN">"#,
        );
        assert_eq!(doc.mode(), lumen_dom::DocumentMode::NoQuirks);
    }
}
