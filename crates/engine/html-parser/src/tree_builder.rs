//! Tree builder (Phase 0 — lenient).
//!
//! Простой стековый построитель. Не реализует insertion modes из HTML5
//! spec (in_table / in_select / реструктуризация foster parent и т.д.) —
//! этого достаточно для текстового веба и большинства простых страниц.
//! При несовпадении закрывающего тега молча игнорирует.
//!
//! Доступен в двух режимах:
//!
//! * [`parse`] — pull-режим: вся строка целиком прогоняется через
//!   [`Tokenizer`] и применяется к DOM.
//! * [`IncrementalTreeBuilder`] — push-режим: ввод подаётся chunk-ами
//!   через [`crate::PushTokenizer`], DOM растёт инкрементально.
//!
//! Инвариант: при идентичном входе оба режима дают одинаковый
//! [`Document`]. Гарантируется через **text-node coalescing**: если
//! push-tokenizer разбил непрерывный текстовый поток на несколько
//! `Token::Text` (из-за chunk boundary), `apply_token` сливает их в
//! один text-node. Pull-режим этим путём проходит no-op (он всегда
//! отдаёт Text-токен единым куском).

use lumen_dom::{Attribute, Document, DocumentMode, NodeData, NodeId, QualName};

use crate::push_tokenizer::PushTokenizer;
use crate::tokenizer::{Token, Tokenizer};

pub fn parse(input: &str) -> Document {
    let mut builder = IncrementalTreeBuilder::new();
    for token in Tokenizer::new(input) {
        builder.apply_token(token);
    }
    builder.finish()
}

/// Push-режим tree builder-а: принимает HTML chunk-ами, держит
/// `Document` и DOM-стек между вызовами `feed`. Финализируется
/// `finish(self) -> Document`.
///
/// Использование:
/// ```ignore
/// let mut b = IncrementalTreeBuilder::new();
/// b.feed("<html><bo");
/// b.feed("dy><p>hi</p></body></html>");
/// let doc = b.finish();
/// ```
pub struct IncrementalTreeBuilder {
    doc: Document,
    stack: Vec<NodeId>,
    tokenizer: PushTokenizer,
    seen_doctype: bool,
}

impl IncrementalTreeBuilder {
    pub fn new() -> Self {
        let doc = Document::new();
        let root = doc.root();
        Self {
            doc,
            stack: vec![root],
            tokenizer: PushTokenizer::new(),
            seen_doctype: false,
        }
    }

    /// Скармливает chunk push-токенизатору и применяет полученные
    /// токены к DOM. После каждого `feed` `Document` валиден для
    /// чтения (consumer может, например, запустить layout на текущем
    /// снапшоте, хотя поддерево может быть незакрытым).
    pub fn feed(&mut self, chunk: &str) {
        for token in self.tokenizer.feed(chunk) {
            self.apply_token(token);
        }
    }

    /// Финализирует ввод. Хвост push-tokenizer-а токенизируется как
    /// при EOF, оставшиеся токены применяются к DOM, выставляется
    /// fallback `DocumentMode::Quirks` если ни одного DOCTYPE не было.
    pub fn finish(mut self) -> Document {
        for token in self.tokenizer.end() {
            self.apply_token(token);
        }
        if !self.seen_doctype {
            self.doc.set_mode(DocumentMode::Quirks);
        }
        self.doc
    }

    /// Применяет один токен к DOM. Используется и pull-парсером
    /// `parse()`, и push-режимом — общая точка, чтобы поведение
    /// гарантированно совпадало.
    fn apply_token(&mut self, token: Token) {
        match token {
            Token::StartTag {
                name,
                attrs,
                self_closing,
            } => {
                let elem = self.doc.create_element(QualName::html(name.clone()));
                if let NodeData::Element {
                    attrs: dom_attrs, ..
                } = &mut self.doc.get_mut(elem).data
                {
                    for (k, v) in attrs {
                        dom_attrs.push(Attribute {
                            name: QualName::html(k),
                            value: v,
                        });
                    }
                }
                let parent = *self.stack.last().expect("stack always non-empty");
                self.doc.append_child(parent, elem);
                if !self_closing && !is_void_element(&name) {
                    self.stack.push(elem);
                }
            }
            Token::EndTag { name } => {
                let matched = self.stack.iter().enumerate().rev().find_map(|(idx, &id)| {
                    if let NodeData::Element { name: n, .. } = &self.doc.get(id).data {
                        (n.local == name).then_some(idx)
                    } else {
                        None
                    }
                });
                if let Some(idx) = matched {
                    self.stack.truncate(idx);
                }
            }
            Token::Text(s) => {
                if s.is_empty() {
                    return;
                }
                let parent = *self.stack.last().expect("stack always non-empty");
                // text-node coalescing: если последний ребёнок — text,
                // дописываем к нему вместо создания нового. Это
                // ключевой инвариант для совпадения push/pull DOM:
                // push может разбить непрерывный текст на несколько
                // Token::Text по chunk boundary.
                let last_child = self.doc.get(parent).children.last().copied();
                if let Some(child) = last_child
                    && let NodeData::Text(existing) = &mut self.doc.get_mut(child).data
                {
                    existing.push_str(&s);
                    return;
                }
                let text = self.doc.create_text(s);
                self.doc.append_child(parent, text);
            }
            Token::Comment(s) => {
                let comment = self.doc.create_comment(s);
                let parent = *self.stack.last().expect("stack always non-empty");
                self.doc.append_child(parent, comment);
            }
            Token::Doctype {
                name,
                public_id,
                system_id,
            } => {
                // §13.2.5.1: только первый DOCTYPE влияет на режим.
                if !self.seen_doctype {
                    self.doc.set_mode(crate::quirks_mode::detect_document_mode(
                        &name,
                        public_id.as_deref(),
                        system_id.as_deref(),
                    ));
                    self.seen_doctype = true;
                }
                let dt = self.doc.create_doctype(
                    name,
                    public_id.unwrap_or_default(),
                    system_id.unwrap_or_default(),
                );
                let parent = *self.stack.last().expect("stack always non-empty");
                self.doc.append_child(parent, dt);
            }
        }
    }
}

impl Default for IncrementalTreeBuilder {
    fn default() -> Self {
        Self::new()
    }
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

    // ──────── IncrementalTreeBuilder — корректность независимо от chunk-боундари ────────

    fn parse_incremental_chunks(input: &str, chunk_size: usize) -> Document {
        let mut b = IncrementalTreeBuilder::new();
        let bytes = input.as_bytes();
        let mut start = 0;
        while start < bytes.len() {
            let mut end = (start + chunk_size).min(bytes.len());
            while !input.is_char_boundary(end) {
                end -= 1;
            }
            if end == start {
                // chunk_size попал внутрь multi-byte char — продвинем
                // до следующей границы.
                end = (start + chunk_size + 4).min(bytes.len());
                while !input.is_char_boundary(end) {
                    end -= 1;
                }
            }
            b.feed(&input[start..end]);
            start = end;
        }
        b.finish()
    }

    fn parse_incremental_byte_by_byte(input: &str) -> Document {
        let mut b = IncrementalTreeBuilder::new();
        let mut start = 0;
        for i in 1..=input.len() {
            if !input.is_char_boundary(i) {
                continue;
            }
            b.feed(&input[start..i]);
            start = i;
        }
        b.finish()
    }

    fn assert_incremental_equals_pull(input: &str) {
        let pull = parse(input).to_string();
        let push_whole = {
            let mut b = IncrementalTreeBuilder::new();
            b.feed(input);
            b.finish().to_string()
        };
        let push_byte = parse_incremental_byte_by_byte(input).to_string();
        let push_chunk = parse_incremental_chunks(input, 8).to_string();
        assert_eq!(push_whole, pull, "push(whole) != pull: {input:?}");
        assert_eq!(push_byte, pull, "push(byte) != pull: {input:?}");
        assert_eq!(push_chunk, pull, "push(8) != pull: {input:?}");
    }

    #[test]
    fn incremental_empty() {
        assert_incremental_equals_pull("");
    }

    #[test]
    fn incremental_plain_text() {
        assert_incremental_equals_pull("hello world");
    }

    #[test]
    fn incremental_simple_tag() {
        assert_incremental_equals_pull("<p>hello</p>");
    }

    #[test]
    fn incremental_nested_tags() {
        assert_incremental_equals_pull("<html><body><h1>Hello</h1></body></html>");
    }

    #[test]
    fn incremental_attributes() {
        assert_incremental_equals_pull(
            r#"<a href="https://example.com" class='x' id=z>link</a>"#,
        );
    }

    #[test]
    fn incremental_void_element() {
        assert_incremental_equals_pull("<p>a<br>b</p>");
    }

    #[test]
    fn incremental_self_closing() {
        assert_incremental_equals_pull("<img src=\"x.png\"/><p>after</p>");
    }

    #[test]
    fn incremental_comment() {
        assert_incremental_equals_pull("<p><!-- note -->text</p>");
    }

    #[test]
    fn incremental_doctype_html5() {
        assert_incremental_equals_pull("<!DOCTYPE html><p>x</p>");
    }

    #[test]
    fn incremental_doctype_html4() {
        assert_incremental_equals_pull(
            r#"<!DOCTYPE HTML PUBLIC "-//W3C//DTD HTML 4.01//EN" "http://www.w3.org/TR/html4/strict.dtd"><p>x</p>"#,
        );
    }

    #[test]
    fn incremental_entity() {
        assert_incremental_equals_pull("<p>a &amp; b &lt; c</p>");
    }

    #[test]
    fn incremental_script_rawtext() {
        assert_incremental_equals_pull("<script>var x = '<b>hi</b>'; if (a<b) f();</script>");
    }

    #[test]
    fn incremental_title_rcdata() {
        assert_incremental_equals_pull("<title>Foo &amp; <b>Bar</b></title>");
    }

    #[test]
    fn incremental_textarea_xss_like() {
        assert_incremental_equals_pull(
            "<textarea>&lt;script&gt;alert(1)&lt;/script&gt;</textarea>",
        );
    }

    #[test]
    fn incremental_cyrillic() {
        assert_incremental_equals_pull("<html><body><h1>Привет, мир</h1></body></html>");
    }

    #[test]
    fn incremental_unclosed_tag() {
        assert_incremental_equals_pull("<p>hello");
    }

    #[test]
    fn incremental_unclosed_script() {
        assert_incremental_equals_pull("<script>x = 1");
    }

    #[test]
    fn incremental_no_doctype_yields_quirks() {
        let mut b = IncrementalTreeBuilder::new();
        b.feed("<p>");
        b.feed("x</p>");
        let doc = b.finish();
        assert_eq!(doc.mode(), DocumentMode::Quirks);
    }

    #[test]
    fn incremental_doctype_split_across_chunks() {
        // DOCTYPE-токен может оказаться разорван по chunk-boundary,
        // например `<!DOC` + `TYPE html>`. Push-tokenizer должен
        // дождаться полного токена и применить mode.
        let mut b = IncrementalTreeBuilder::new();
        b.feed("<!DOC");
        b.feed("TYPE html><p>x</p>");
        let doc = b.finish();
        assert_eq!(doc.mode(), DocumentMode::NoQuirks);
    }

    #[test]
    fn incremental_entity_split_across_chunks() {
        // `&amp;` разрезан посреди: `&am` + `p;`. Финальный DOM
        // должен содержать декодированный `&`.
        let mut b = IncrementalTreeBuilder::new();
        b.feed("<p>a &am");
        b.feed("p; b</p>");
        let doc = b.finish();
        let s = doc.to_string();
        assert!(s.contains("\"a & b\""), "got: {s}");
    }

    #[test]
    fn incremental_rawtext_close_tag_split() {
        // `</script>` разорван — байт-за-байтом push не должен
        // преждевременно закрыть script.
        let mut b = IncrementalTreeBuilder::new();
        b.feed("<script>x = 1; </scr");
        b.feed("ipt><p>after</p>");
        let doc = b.finish();
        let s = doc.to_string();
        assert!(s.contains("\"x = 1; \""), "got: {s}");
        assert!(s.contains("<p>"));
        assert!(s.contains("\"after\""));
    }
}
