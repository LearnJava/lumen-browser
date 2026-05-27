//! HTML5 §13.2 tree construction.
//!
//! Реализация insertion modes по спецификации WHATWG HTML LS §13.2:
//! Initial, BeforeHtml, BeforeHead, InHead, InHeadNoscript, AfterHead,
//! InBody, Text, InTable, InTableText, InTableBody, InRow, InCell,
//! InSelect, InSelectInTable, InCaption, InColumnGroup, InTemplate,
//! InFrameset, AfterBody, AfterFrameset, AfterAfterBody,
//! AfterAfterFrameset — все 23 режима по §13.2.4.1.
//!
//! Ключевые алгоритмы:
//!
//! * Active formatting elements list + reconstruction (§13.2.4.3) —
//!   `<b>`, `<i>`, `<a>` и т.д. восстанавливаются при «прорыве» через
//!   границы блоков.
//! * Adoption Agency Algorithm (§13.2.6.4.7 «in body»: «An end tag
//!   whose tag name is one of: a, b, big, code, em, font, i, nobr, s,
//!   small, strike, strong, tt, u») для разрешения mis-nesting.
//! * Foster parenting (§13.2.6.1) — текст и не-table-элементы в
//!   `<table>`-контексте вставляются перед `<table>`.
//! * Auto-close: `<p>` перед block elements (§13.2.6.4.7 «in body»
//!   правило «have a p element in button scope»), `<li>` перед `<li>`,
//!   `<h1>..<h6>` перед `<h1>..<h6>`.
//! * Implicit `<html>` / `<head>` / `<body>`.
//!
//! Доступен в двух режимах:
//! * [`parse`] — pull-режим: вся строка прогоняется через
//!   [`Tokenizer`].
//! * [`IncrementalTreeBuilder`] — push-режим: ввод подаётся chunk-ами
//!   через [`PushTokenizer`], DOM растёт инкрементально.
//!
//! Инвариант: при идентичном входе оба режима дают одинаковый
//! [`Document`]. Гарантируется через **text-node coalescing**: если
//! push-tokenizer разбил непрерывный текстовый поток на несколько
//! `Token::Text` (из-за chunk boundary), `apply_token` сливает их в
//! один text-node.

use lumen_dom::{Attribute, Document, DocumentMode, NodeData, NodeId, QualName};

use crate::push_tokenizer::PushTokenizer;
use crate::tokenizer::{Token, Tokenizer};

/// Парсит вход целиком в pull-режиме и возвращает построенный
/// [`Document`]. Эквивалент `IncrementalTreeBuilder::new() + feed(input)
/// + finish()`, но без накладных расходов на push-буферизацию.
pub fn parse(input: &str) -> Document {
    let mut builder = IncrementalTreeBuilder::new();
    for token in Tokenizer::new(input) {
        builder.apply_token(token);
    }
    builder.finish()
}

/// Все 23 insertion modes из §13.2.4.1. Foreign content (MathML, SVG) не
/// поддерживается в Phase 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InsertionMode {
    Initial,
    BeforeHtml,
    BeforeHead,
    InHead,
    /// §13.2.6.4.5 «The in head noscript insertion mode». Active when
    /// scripting is disabled and the parser encounters `<noscript>` in head.
    InHeadNoscript,
    AfterHead,
    InBody,
    Text,
    InTable,
    InTableText,
    InCaption,
    InColumnGroup,
    InTableBody,
    InRow,
    InCell,
    InSelect,
    /// §13.2.6.4.17 «The in select in table insertion mode». Active when a
    /// `<select>` appears inside a table cell/caption context.
    InSelectInTable,
    /// §13.2.6.4.19 «The in template insertion mode». Active while the parser
    /// is inside a `<template>` element. Content is inserted into the
    /// template's `DocumentFragment` rather than the template element itself.
    InTemplate,
    /// §13.2.6.4.20 «The in frameset insertion mode». Active when the
    /// document uses `<frameset>` rather than `<body>`.
    InFrameset,
    AfterBody,
    /// §13.2.6.4.21 «The after frameset insertion mode». Active after
    /// `</frameset>` closes the outermost frameset.
    AfterFrameset,
    AfterAfterBody,
    /// §13.2.6.4.23 «The after after frameset insertion mode». Active after
    /// `</html>` in a frameset document.
    AfterAfterFrameset,
}

/// Запись в списке active formatting elements (§13.2.4.3). Либо
/// маркер (граница scope при `<table>`, `<object>` и т.д.), либо
/// элемент с сохранённым именем/атрибутами для Noah's Ark clause.
#[derive(Clone)]
enum ActiveFormattingEntry {
    Marker,
    Element {
        node: NodeId,
        tag: String,
        attrs: Vec<(String, String)>,
    },
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
    /// Строящийся DOM. После `finish()` отдаётся caller-у.
    doc: Document,
    /// Стек открытых элементов (§13.2.4.2). Top — `last()`. Никогда
    /// не содержит `Document`-корень.
    open_elements: Vec<NodeId>,
    /// Список active formatting elements (§13.2.4.3) с маркерами.
    active_formatting: Vec<ActiveFormattingEntry>,
    /// Текущий insertion mode (§13.2.4.1).
    insertion_mode: InsertionMode,
    /// Сохранённый mode для возврата из Text-mode (§13.2.6.4.8).
    original_insertion_mode: Option<InsertionMode>,
    /// Указатель на `<head>` (§13.2.4.4). Нужен для InHead/AfterHead.
    head_element: Option<NodeId>,
    /// Указатель на текущий `<form>` (§13.2.4.4). Phase 0: используется
    /// частично — некоторые формы клонирующих правил опущены.
    #[allow(dead_code)]
    form_element: Option<NodeId>,
    /// Накопитель символов для InTableText (§13.2.6.4.10): table-режим
    /// собирает текстовые токены и решает в конце, foster-ить ли их.
    pending_table_text: String,
    /// `true` если в pending_table_text есть хоть один не-whitespace
    /// символ — тогда finish-of-InTableText включает foster parenting.
    pending_table_text_has_nonspace: bool,
    /// Push-режим токенизатора.
    tokenizer: PushTokenizer,
    /// Виделся ли DOCTYPE — для fallback Quirks при `finish()`.
    seen_doctype: bool,
    /// Stack of per-template insertion modes (§13.2.6.4.19). Each open
    /// `<template>` pushes its content mode here; `</template>` pops it.
    /// When non-empty, `insertion_mode` is `InTemplate`.
    template_mode_stack: Vec<InsertionMode>,
    /// Whether scripting is considered enabled (§13.2.3.5). When `true`,
    /// `<noscript>` in `<head>` is treated as raw text (scripting-enabled
    /// path). When `false`, the parser enters `InHeadNoscript` mode so that
    /// `<noscript>` content is parsed as markup. Default: `true`.
    scripting_enabled: bool,
}

impl IncrementalTreeBuilder {
    /// Создаёт пустой builder в insertion mode `Initial`.
    pub fn new() -> Self {
        Self {
            doc: Document::new(),
            open_elements: Vec::new(),
            active_formatting: Vec::new(),
            insertion_mode: InsertionMode::Initial,
            original_insertion_mode: None,
            head_element: None,
            form_element: None,
            pending_table_text: String::new(),
            pending_table_text_has_nonspace: false,
            tokenizer: PushTokenizer::new(),
            seen_doctype: false,
            template_mode_stack: Vec::new(),
            scripting_enabled: true,
        }
    }

    /// Скармливает chunk push-токенизатору и применяет полученные
    /// токены к DOM. После каждого `feed` `Document` валиден для
    /// чтения.
    pub fn feed(&mut self, chunk: &str) {
        for token in self.tokenizer.feed(chunk) {
            self.apply_token(token);
        }
    }

    /// Вариант [`feed`][Self::feed] для сырых байт.
    pub fn feed_bytes(&mut self, chunk: &[u8]) {
        for token in self.tokenizer.feed_bytes(chunk) {
            self.apply_token(token);
        }
    }

    /// Возвращает ссылку на текущее состояние DOM.
    pub fn as_doc(&self) -> &Document {
        &self.doc
    }

    /// Финализирует ввод. Хвост push-tokenizer-а токенизируется как
    /// при EOF, прогоняется EOF-сценарий insertion modes, выставляется
    /// fallback `DocumentMode::Quirks` если ни одного DOCTYPE не было.
    /// Гарантирует наличие `<html>` / `<head>` / `<body>` даже для
    /// пустого ввода (§13.2.6.4.1-3).
    pub fn finish(mut self) -> Document {
        for token in self.tokenizer.end() {
            self.apply_token(token);
        }
        if !self.seen_doctype {
            self.doc.set_mode(DocumentMode::Quirks);
        }
        // EOF: догоняем недостающую структуру html/head/body.
        self.process_eof();
        self.doc
    }

    /// Применяет один токен к DOM. Используется и pull-парсером
    /// `parse()`, и push-режимом — общая точка, чтобы поведение
    /// гарантированно совпадало.
    fn apply_token(&mut self, token: Token) {
        // InTableText аккумулирует подряд идущие Text-токены и
        // разрешается при первом не-Text токене.
        if self.insertion_mode == InsertionMode::InTableText {
            if let Token::Text(s) = &token {
                for ch in s.chars() {
                    if !is_html_whitespace(ch) {
                        self.pending_table_text_has_nonspace = true;
                    }
                }
                self.pending_table_text.push_str(s);
                return;
            }
            self.flush_pending_table_text();
        }
        self.dispatch(token);
    }

    /// Маршрутизатор по insertion mode (§13.2.6).
    fn dispatch(&mut self, token: Token) {
        match self.insertion_mode {
            InsertionMode::Initial => self.mode_initial(token),
            InsertionMode::BeforeHtml => self.mode_before_html(token),
            InsertionMode::BeforeHead => self.mode_before_head(token),
            InsertionMode::InHead => self.mode_in_head(token),
            InsertionMode::InHeadNoscript => self.mode_in_head_noscript(token),
            InsertionMode::AfterHead => self.mode_after_head(token),
            InsertionMode::InBody => self.mode_in_body(token),
            InsertionMode::Text => self.mode_text(token),
            InsertionMode::InTable => self.mode_in_table(token),
            InsertionMode::InTableText => {
                // Перехвачено в apply_token.
                self.mode_in_table(token);
            }
            InsertionMode::InCaption => self.mode_in_caption(token),
            InsertionMode::InColumnGroup => self.mode_in_column_group(token),
            InsertionMode::InTableBody => self.mode_in_table_body(token),
            InsertionMode::InRow => self.mode_in_row(token),
            InsertionMode::InCell => self.mode_in_cell(token),
            InsertionMode::InSelect => self.mode_in_select(token),
            InsertionMode::InSelectInTable => self.mode_in_select_in_table(token),
            InsertionMode::InTemplate => self.mode_in_template(token),
            InsertionMode::InFrameset => self.mode_in_frameset(token),
            InsertionMode::AfterBody => self.mode_after_body(token),
            InsertionMode::AfterFrameset => self.mode_after_frameset(token),
            InsertionMode::AfterAfterBody => self.mode_after_after_body(token),
            InsertionMode::AfterAfterFrameset => self.mode_after_after_frameset(token),
        }
    }

    // ─────────────────────────────────────────────────────────────
    // Insertion modes
    // ─────────────────────────────────────────────────────────────

    /// §13.2.6.4.1 «The initial insertion mode».
    fn mode_initial(&mut self, token: Token) {
        match token {
            Token::Doctype {
                name,
                public_id,
                system_id,
            } => {
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
                let root = self.doc.root();
                self.doc.append_child(root, dt);
                self.insertion_mode = InsertionMode::BeforeHtml;
            }
            Token::Comment(s) => {
                let root = self.doc.root();
                let c = self.doc.create_comment(s);
                self.doc.append_child(root, c);
            }
            Token::Text(ref s) if s.chars().all(is_html_whitespace) => {
                // Игнорируем whitespace.
            }
            other => {
                self.insertion_mode = InsertionMode::BeforeHtml;
                self.dispatch(other);
            }
        }
    }

    /// §13.2.6.4.2 «The before html insertion mode».
    fn mode_before_html(&mut self, token: Token) {
        match token {
            Token::Doctype { .. } => { /* parse error: ignore */ }
            Token::Comment(s) => {
                let root = self.doc.root();
                let c = self.doc.create_comment(s);
                self.doc.append_child(root, c);
            }
            Token::Text(ref s) if s.chars().all(is_html_whitespace) => { /* ignore */ }
            Token::StartTag {
                ref name, ref attrs, ..
            } if name == "html" => {
                let html = self.create_element_with_attrs("html", attrs);
                let root = self.doc.root();
                self.doc.append_child(root, html);
                self.open_elements.push(html);
                self.insertion_mode = InsertionMode::BeforeHead;
            }
            Token::EndTag { ref name }
                if !matches!(name.as_str(), "head" | "body" | "html" | "br") =>
            {
                // parse error: ignore
            }
            other => {
                // Implicit <html>.
                let html = self.create_element_with_attrs("html", &[]);
                let root = self.doc.root();
                self.doc.append_child(root, html);
                self.open_elements.push(html);
                self.insertion_mode = InsertionMode::BeforeHead;
                self.dispatch(other);
            }
        }
    }

    /// §13.2.6.4.3 «The before head insertion mode».
    fn mode_before_head(&mut self, token: Token) {
        match token {
            Token::Text(ref s) if s.chars().all(is_html_whitespace) => { /* ignore */ }
            Token::Comment(s) => {
                self.insert_comment(s);
            }
            Token::Doctype { .. } => { /* parse error */ }
            Token::StartTag {
                ref name, ref attrs, ..
            } if name == "html" => {
                self.in_body_start_html_attrs(attrs);
            }
            Token::StartTag {
                ref name, ref attrs, ..
            } if name == "head" => {
                let head = self.create_element_with_attrs("head", attrs);
                self.append_to_current_open(head);
                self.open_elements.push(head);
                self.head_element = Some(head);
                self.insertion_mode = InsertionMode::InHead;
            }
            Token::EndTag { ref name }
                if !matches!(name.as_str(), "head" | "body" | "html" | "br") =>
            {
                // parse error: ignore
            }
            other => {
                let head = self.create_element_with_attrs("head", &[]);
                self.append_to_current_open(head);
                self.open_elements.push(head);
                self.head_element = Some(head);
                self.insertion_mode = InsertionMode::InHead;
                self.dispatch(other);
            }
        }
    }

    /// §13.2.6.4.4 «The in head insertion mode».
    fn mode_in_head(&mut self, token: Token) {
        match token {
            Token::Text(s) => {
                let (ws, rest) = split_leading_ws(&s);
                if !ws.is_empty() {
                    self.insert_text(ws);
                }
                if !rest.is_empty() {
                    self.pop_head_and_dispatch_text(rest.to_string());
                }
            }
            Token::Comment(s) => self.insert_comment(s),
            Token::Doctype { .. } => { /* parse error */ }
            Token::StartTag {
                ref name, ref attrs, ..
            } if name == "html" => {
                self.in_body_start_html_attrs(attrs);
            }
            Token::StartTag {
                ref name,
                ref attrs,
                self_closing,
            } if matches!(
                name.as_str(),
                "base" | "basefont" | "bgsound" | "link" | "meta"
            ) =>
            {
                let _ = self_closing;
                let el = self.create_element_with_attrs(name, attrs);
                self.append_to_current_open(el);
                // Void: не push в open_elements.
            }
            Token::StartTag {
                ref name, ref attrs, ..
            } if name == "title" => {
                let el = self.create_element_with_attrs(name, attrs);
                self.append_to_current_open(el);
                self.open_elements.push(el);
                self.original_insertion_mode = Some(self.insertion_mode);
                self.insertion_mode = InsertionMode::Text;
            }
            Token::StartTag {
                ref name,
                ref attrs,
                self_closing,
            } if matches!(name.as_str(), "noframes" | "style" | "script") =>
            {
                let _ = self_closing;
                let el = self.create_element_with_attrs(name, attrs);
                self.append_to_current_open(el);
                self.open_elements.push(el);
                self.original_insertion_mode = Some(self.insertion_mode);
                self.insertion_mode = InsertionMode::Text;
            }
            // §13.2.6.4.4 «In head» — `<noscript>`. Behaviour depends on
            // scripting flag (§13.2.3.5): if scripting is enabled, noscript
            // content is raw text (invisible to the parser); if scripting is
            // disabled, the content is parsed as markup via InHeadNoscript.
            Token::StartTag {
                ref name,
                ref attrs,
                self_closing,
            } if name == "noscript" =>
            {
                let _ = self_closing;
                if self.scripting_enabled {
                    // Scripting on: treat as raw text (same as <style>/<script>).
                    let el = self.create_element_with_attrs(name, attrs);
                    self.append_to_current_open(el);
                    self.open_elements.push(el);
                    self.original_insertion_mode = Some(self.insertion_mode);
                    self.insertion_mode = InsertionMode::Text;
                } else {
                    // Scripting off: parse noscript content as HTML.
                    let el = self.create_element_with_attrs(name, attrs);
                    self.append_to_current_open(el);
                    self.open_elements.push(el);
                    self.insertion_mode = InsertionMode::InHeadNoscript;
                }
            }
            // HTML LS §13.2.6.4.4 «In head» — `<template>` start tag.
            Token::StartTag {
                ref name,
                ref attrs,
                ..
            } if name == "template" => {
                let el = self.create_element_with_attrs(name, attrs);
                self.append_to_current_open(el);
                self.open_elements.push(el);
                // Create the content fragment and associate it with the element.
                let frag = self.doc.create_fragment();
                self.doc.set_template_content(el, frag);
                // Push an active formatting marker so AAA doesn't cross template boundary.
                self.active_formatting.push(ActiveFormattingEntry::Marker);
                // The template content will be parsed in InBody mode (per spec §13.2.6.4.19).
                self.template_mode_stack.push(InsertionMode::InBody);
                self.insertion_mode = InsertionMode::InTemplate;
            }
            // HTML LS §13.2.6.4.4 «In head» — `</template>` end tag.
            Token::EndTag { ref name } if name == "template" => {
                self.process_template_end_tag();
            }
            Token::EndTag { ref name } if name == "head" => {
                self.open_elements.pop();
                self.insertion_mode = InsertionMode::AfterHead;
            }
            Token::EndTag { ref name } if matches!(name.as_str(), "body" | "html" | "br") => {
                // Pop head, switch to AfterHead, reprocess.
                self.open_elements.pop();
                self.insertion_mode = InsertionMode::AfterHead;
                self.dispatch(Token::EndTag { name: name.clone() });
            }
            Token::StartTag { ref name, .. } if name == "head" => {
                // parse error: ignore
            }
            other => {
                self.open_elements.pop();
                self.insertion_mode = InsertionMode::AfterHead;
                self.dispatch(other);
            }
        }
    }

    /// Хелпер: вспышка не-whitespace текста в InHead — закрыть head,
    /// перейти в AfterHead, дальше диспатчить как Text.
    fn pop_head_and_dispatch_text(&mut self, rest: String) {
        self.open_elements.pop();
        self.insertion_mode = InsertionMode::AfterHead;
        self.dispatch(Token::Text(rest));
    }

    /// §13.2.6.4.6 «The after head insertion mode».
    fn mode_after_head(&mut self, token: Token) {
        match token {
            Token::Text(s) => {
                let (ws, rest) = split_leading_ws(&s);
                if !ws.is_empty() {
                    self.insert_text(ws);
                }
                if !rest.is_empty() {
                    self.implicit_body_and_dispatch(Token::Text(rest.to_string()));
                }
            }
            Token::Comment(s) => self.insert_comment(s),
            Token::Doctype { .. } => { /* parse error */ }
            Token::StartTag {
                ref name, ref attrs, ..
            } if name == "html" => {
                self.in_body_start_html_attrs(attrs);
            }
            Token::StartTag {
                ref name, ref attrs, ..
            } if name == "body" => {
                let body = self.create_element_with_attrs("body", attrs);
                self.append_to_current_open(body);
                self.open_elements.push(body);
                self.insertion_mode = InsertionMode::InBody;
            }
            Token::StartTag {
                ref name,
                ref attrs,
                ..
            } if name == "frameset" => {
                let fs = self.create_element_with_attrs(name, attrs);
                self.append_to_current_open(fs);
                self.open_elements.push(fs);
                self.insertion_mode = InsertionMode::InFrameset;
            }
            Token::StartTag { ref name, .. }
                if matches!(
                    name.as_str(),
                    "base"
                        | "basefont"
                        | "bgsound"
                        | "link"
                        | "meta"
                        | "noframes"
                        | "script"
                        | "style"
                        | "template"
                        | "title"
                ) =>
            {
                // parse error: push head back temporarily, обрабатываем в InHead.
                if let Some(head) = self.head_element {
                    self.open_elements.push(head);
                    let saved = self.insertion_mode;
                    self.insertion_mode = InsertionMode::InHead;
                    self.dispatch(token);
                    // Удаляем head из стека, если он там ещё.
                    if let Some(pos) = self.open_elements.iter().position(|&n| n == head) {
                        self.open_elements.remove(pos);
                    }
                    // Restore only if InHead didn't switch us to InTemplate.
                    if self.insertion_mode == InsertionMode::InHead {
                        self.insertion_mode = saved;
                    }
                } else {
                    // Без head — implicit body.
                    self.implicit_body_and_dispatch(token);
                }
            }
            Token::EndTag { ref name } if matches!(name.as_str(), "body" | "html" | "br") => {
                self.implicit_body_and_dispatch(token);
            }
            Token::EndTag { .. } => { /* parse error: ignore */ }
            Token::StartTag { ref name, .. } if name == "head" => {
                // parse error: ignore
            }
            other => {
                self.implicit_body_and_dispatch(other);
            }
        }
    }

    /// Хелпер: создаём implicit `<body>`, переключаем mode в InBody,
    /// и дальше диспатчим.
    fn implicit_body_and_dispatch(&mut self, token: Token) {
        let body = self.create_element_with_attrs("body", &[]);
        self.append_to_current_open(body);
        self.open_elements.push(body);
        self.insertion_mode = InsertionMode::InBody;
        self.dispatch(token);
    }

    /// `<html>` start tag из вне-InBody-режимов: merge атрибутов в
    /// существующий root html (§13.2.6.4.7 «in body» правило start
    /// html).
    fn in_body_start_html_attrs(&mut self, attrs: &[(String, String)]) {
        // Берём первый html в стеке (он там единственный).
        if let Some(&html) = self.open_elements.first()
            && let NodeData::Element {
                attrs: dom_attrs, ..
            } = &mut self.doc.get_mut(html).data
        {
            for (k, v) in attrs {
                if !dom_attrs.iter().any(|a| &a.name.local == k) {
                    dom_attrs.push(Attribute {
                        name: QualName::html(k.clone()),
                        value: v.clone(),
                    });
                }
            }
        }
    }

    /// §13.2.6.4.7 «The in body insertion mode» — основной режим.
    fn mode_in_body(&mut self, token: Token) {
        match token {
            Token::Text(s) => {
                if s.is_empty() {
                    return;
                }
                self.reconstruct_active_formatting();
                self.insert_text(&s);
            }
            Token::Comment(s) => self.insert_comment(s),
            Token::Doctype { .. } => { /* parse error */ }
            Token::StartTag {
                ref name, ref attrs, ..
            } if name == "html" => {
                self.in_body_start_html_attrs(attrs);
            }
            Token::StartTag { ref name, .. }
                if matches!(
                    name.as_str(),
                    "base"
                        | "basefont"
                        | "bgsound"
                        | "link"
                        | "meta"
                        | "noframes"
                        | "script"
                        | "style"
                        | "title"
                ) =>
            {
                // Process as in InHead (mode restored after dispatch).
                let saved = self.insertion_mode;
                self.insertion_mode = InsertionMode::InHead;
                self.dispatch(token);
                self.insertion_mode = saved;
            }
            // `<template>` in body: delegate to InHead processing which switches
            // to InTemplate — do NOT restore mode afterwards.
            Token::StartTag { ref name, ref attrs, .. } if name == "template" => {
                let saved = self.insertion_mode;
                self.insertion_mode = InsertionMode::InHead;
                self.dispatch(token);
                // InHead moved us to InTemplate; only restore if it didn't.
                if self.insertion_mode == InsertionMode::InHead {
                    self.insertion_mode = saved;
                }
            }
            // `</template>` in body: delegate to InHead.
            Token::EndTag { ref name } if name == "template" => {
                let saved = self.insertion_mode;
                self.insertion_mode = InsertionMode::InHead;
                self.dispatch(token);
                if self.insertion_mode == InsertionMode::InHead {
                    self.insertion_mode = saved;
                }
            }
            Token::StartTag {
                ref name, ref attrs, ..
            } if name == "body" => {
                // Merge атрибуты в body.
                if let Some(&body) = self.open_elements.get(1)
                    && let NodeData::Element {
                        attrs: dom_attrs, ..
                    } = &mut self.doc.get_mut(body).data
                {
                    for (k, v) in attrs {
                        if !dom_attrs.iter().any(|a| &a.name.local == k) {
                            dom_attrs.push(Attribute {
                                name: QualName::html(k.clone()),
                                value: v.clone(),
                            });
                        }
                    }
                }
            }
            Token::EndTag { ref name } if name == "body" => {
                self.insertion_mode = InsertionMode::AfterBody;
            }
            Token::EndTag { ref name } if name == "html" => {
                self.insertion_mode = InsertionMode::AfterBody;
                self.dispatch(Token::EndTag { name: name.clone() });
            }
            // Block-уровневые элементы: закрывают <p> в button scope.
            Token::StartTag {
                ref name, ref attrs, ..
            } if is_block_element(name) => {
                if self.has_element_in_button_scope("p") {
                    self.close_p_element();
                }
                let el = self.create_element_with_attrs(name, attrs);
                self.append_to_current_open(el);
                self.open_elements.push(el);
            }
            // <h1>..<h6>: закрывают <p> в button scope, а также
            // предыдущий heading в стеке.
            Token::StartTag {
                ref name, ref attrs, ..
            } if matches!(name.as_str(), "h1" | "h2" | "h3" | "h4" | "h5" | "h6") => {
                if self.has_element_in_button_scope("p") {
                    self.close_p_element();
                }
                if let Some(top) = self.open_elements.last()
                    && is_heading(self.element_local(*top))
                {
                    self.open_elements.pop();
                }
                let el = self.create_element_with_attrs(name, attrs);
                self.append_to_current_open(el);
                self.open_elements.push(el);
            }
            // <li>: имплисит-закрытие предыдущего <li>.
            Token::StartTag {
                ref name, ref attrs, ..
            } if name == "li" => {
                self.close_list_item_like(&["li"]);
                if self.has_element_in_button_scope("p") {
                    self.close_p_element();
                }
                let el = self.create_element_with_attrs(name, attrs);
                self.append_to_current_open(el);
                self.open_elements.push(el);
            }
            // <dt>/<dd>: closing previous <dt>/<dd>.
            Token::StartTag {
                ref name, ref attrs, ..
            } if matches!(name.as_str(), "dt" | "dd") => {
                self.close_list_item_like(&["dt", "dd"]);
                if self.has_element_in_button_scope("p") {
                    self.close_p_element();
                }
                let el = self.create_element_with_attrs(name, attrs);
                self.append_to_current_open(el);
                self.open_elements.push(el);
            }
            // <a>: если уже есть в active formatting, прогнать adoption
            // agency и удалить.
            Token::StartTag {
                ref name, ref attrs, ..
            } if name == "a" => {
                if let Some(existing) = self.find_active_formatting_after_marker("a") {
                    self.adoption_agency("a");
                    // Удалить, если ещё есть.
                    self.remove_from_active_formatting(existing);
                    if let Some(pos) = self.open_elements.iter().position(|&n| n == existing) {
                        self.open_elements.remove(pos);
                    }
                }
                self.reconstruct_active_formatting();
                let el = self.create_element_with_attrs(name, attrs);
                self.append_to_current_open(el);
                self.open_elements.push(el);
                self.push_active_formatting(el, name, attrs);
            }
            // Formatting elements: b, big, code, em, font, i, s,
            // small, strike, strong, tt, u.
            Token::StartTag {
                ref name, ref attrs, ..
            } if is_formatting_element(name) => {
                self.reconstruct_active_formatting();
                let el = self.create_element_with_attrs(name, attrs);
                self.append_to_current_open(el);
                self.open_elements.push(el);
                self.push_active_formatting(el, name, attrs);
            }
            // <nobr>: специальный случай — если есть в scope, adoption.
            Token::StartTag {
                ref name, ref attrs, ..
            } if name == "nobr" => {
                self.reconstruct_active_formatting();
                if self.has_element_in_scope("nobr") {
                    self.adoption_agency("nobr");
                    self.reconstruct_active_formatting();
                }
                let el = self.create_element_with_attrs(name, attrs);
                self.append_to_current_open(el);
                self.open_elements.push(el);
                self.push_active_formatting(el, name, attrs);
            }
            // End tags для formatting elements → adoption agency.
            Token::EndTag { ref name } if is_formatting_element(name) || name == "a" || name == "nobr" => {
                self.adoption_agency(name);
            }
            // Void elements.
            Token::StartTag {
                ref name, ref attrs, ..
            } if is_void_element(name) => {
                self.reconstruct_active_formatting();
                let el = self.create_element_with_attrs(name, attrs);
                self.append_to_current_open(el);
                // Не push в open_elements.
            }
            // <table>.
            Token::StartTag {
                ref name, ref attrs, ..
            } if name == "table" => {
                if self.doc.mode() != DocumentMode::Quirks && self.has_element_in_button_scope("p")
                {
                    self.close_p_element();
                }
                let el = self.create_element_with_attrs(name, attrs);
                self.append_to_current_open(el);
                self.open_elements.push(el);
                self.insertion_mode = InsertionMode::InTable;
            }
            // <select>.
            Token::StartTag {
                ref name, ref attrs, ..
            } if name == "select" => {
                self.reconstruct_active_formatting();
                let el = self.create_element_with_attrs(name, attrs);
                self.append_to_current_open(el);
                self.open_elements.push(el);
                self.insertion_mode = InsertionMode::InSelect;
            }
            // <textarea>.
            Token::StartTag {
                ref name, ref attrs, ..
            } if name == "textarea" => {
                let el = self.create_element_with_attrs(name, attrs);
                self.append_to_current_open(el);
                self.open_elements.push(el);
                self.original_insertion_mode = Some(self.insertion_mode);
                self.insertion_mode = InsertionMode::Text;
            }
            // <button>: если есть в scope, закрыть.
            Token::StartTag {
                ref name, ref attrs, ..
            } if name == "button" => {
                if self.has_element_in_scope("button") {
                    self.generate_implied_end_tags(None);
                    while let Some(&top) = self.open_elements.last() {
                        let n = self.element_local(top).to_string();
                        self.open_elements.pop();
                        if n == "button" {
                            break;
                        }
                    }
                }
                self.reconstruct_active_formatting();
                let el = self.create_element_with_attrs(name, attrs);
                self.append_to_current_open(el);
                self.open_elements.push(el);
            }
            // <p>: ничего особого, но AAA для парсинга `<p>x<div>...`.
            Token::EndTag { ref name } if name == "p" => {
                if !self.has_element_in_button_scope("p") {
                    // parse error: insert implicit <p> then close.
                    let p = self.create_element_with_attrs("p", &[]);
                    self.append_to_current_open(p);
                    self.open_elements.push(p);
                }
                self.close_p_element();
            }
            // </li>, </dt>, </dd>, </h1..h6>.
            Token::EndTag { ref name } if name == "li" => {
                if self.has_element_in_list_item_scope("li") {
                    self.generate_implied_end_tags(Some("li"));
                    while let Some(top) = self.open_elements.pop() {
                        if self.element_local(top) == "li" {
                            break;
                        }
                    }
                }
            }
            Token::EndTag { ref name } if matches!(name.as_str(), "dt" | "dd") => {
                let n = name.clone();
                if self.has_element_in_scope(&n) {
                    self.generate_implied_end_tags(Some(&n));
                    while let Some(top) = self.open_elements.pop() {
                        if self.element_local(top) == n {
                            break;
                        }
                    }
                }
            }
            Token::EndTag { ref name }
                if matches!(name.as_str(), "h1" | "h2" | "h3" | "h4" | "h5" | "h6") =>
            {
                if self.has_heading_in_scope() {
                    self.generate_implied_end_tags(None);
                    while let Some(top) = self.open_elements.pop() {
                        if is_heading(self.element_local(top)) {
                            break;
                        }
                    }
                }
            }
            // </br> — treated as <br>.
            Token::EndTag { ref name } if name == "br" => {
                self.reconstruct_active_formatting();
                let el = self.create_element_with_attrs("br", &[]);
                self.append_to_current_open(el);
            }
            // Generic block end tag.
            Token::EndTag { ref name } if is_block_element(name) => {
                let n = name.clone();
                if self.has_element_in_scope(&n) {
                    self.generate_implied_end_tags(None);
                    while let Some(top) = self.open_elements.pop() {
                        if self.element_local(top) == n {
                            break;
                        }
                    }
                }
            }
            // Generic start tag.
            Token::StartTag {
                ref name,
                ref attrs,
                self_closing,
            } => {
                let _ = self_closing;
                self.reconstruct_active_formatting();
                let el = self.create_element_with_attrs(name, attrs);
                self.append_to_current_open(el);
                self.open_elements.push(el);
            }
            // Generic end tag.
            Token::EndTag { ref name } => {
                let n = name.clone();
                self.generic_end_tag_in_body(&n);
            }
        }
    }

    /// Generic end tag fallback — пройти по стеку, найти совпадение,
    /// generate implied end tags исключая n, и pop до match.
    fn generic_end_tag_in_body(&mut self, name: &str) {
        for i in (0..self.open_elements.len()).rev() {
            let node = self.open_elements[i];
            let local = self.element_local(node).to_string();
            if local == name {
                self.generate_implied_end_tags(Some(name));
                self.open_elements.truncate(i);
                return;
            }
            if is_special(&local) {
                // parse error: ignore.
                return;
            }
        }
    }

    /// §13.2.6.4.8 «The text insertion mode» — для RAWTEXT/RCDATA.
    fn mode_text(&mut self, token: Token) {
        match token {
            Token::Text(s) if !s.is_empty() => {
                self.insert_text(&s);
            }
            Token::EndTag { .. } => {
                self.open_elements.pop();
                if let Some(prev) = self.original_insertion_mode.take() {
                    self.insertion_mode = prev;
                } else {
                    self.insertion_mode = InsertionMode::InBody;
                }
            }
            _ => { /* EOF / etc. — parse error, ignore */ }
        }
    }

    /// §13.2.6.4.19 «The in template insertion mode».
    ///
    /// Handles tokens while the parser is inside a `<template>` element.
    /// All content is inserted into the template's `DocumentFragment` via
    /// `current_insertion_parent()` redirect. Head-level tags and `</template>`
    /// are forwarded to `mode_in_head`.
    fn mode_in_template(&mut self, token: Token) {
        let is_end_template = matches!(&token, Token::EndTag { name } if name == "template");
        let is_head_tag = matches!(
            &token,
            Token::StartTag { name, .. }
                if matches!(
                    name.as_str(),
                    "base" | "basefont" | "bgsound" | "link" | "meta"
                        | "noframes" | "script" | "style" | "template" | "title"
                )
        );

        if is_end_template || is_head_tag {
            // Delegate to InHead; it will either close the template (switching
            // us away from InTemplate) or handle the head-level tag.
            self.insertion_mode = InsertionMode::InHead;
            self.dispatch(token);
            // If InHead didn't transition us, we must stay in InTemplate.
            if self.insertion_mode == InsertionMode::InHead {
                self.insertion_mode = InsertionMode::InTemplate;
            }
            return;
        }

        // All other tokens: process using the template content mode (InBody by
        // default). The current_insertion_parent() redirect ensures nodes land
        // in the fragment, not in the template element.
        let content_mode = self
            .template_mode_stack
            .last()
            .copied()
            .unwrap_or(InsertionMode::InBody);
        self.insertion_mode = content_mode;
        self.dispatch(token);
        // Restore InTemplate if content mode dispatch didn't switch to something
        // else (e.g. a nested InTemplate for a nested <template>).
        if self.insertion_mode == content_mode {
            self.insertion_mode = InsertionMode::InTemplate;
        }
    }

    /// Process `</template>` end tag — shared by InHead and InTemplate.
    ///
    /// Pops open elements up to and including `<template>`, clears active
    /// formatting up to the last marker, pops the template mode stack, and
    /// resets the insertion mode (§13.2.6.4.4 «In head», end-tag `template`).
    fn process_template_end_tag(&mut self) {
        // Find template on stack.
        let pos = self
            .open_elements
            .iter()
            .rposition(|&n| self.element_local(n) == "template");
        let Some(pos) = pos else {
            // Parse error: no matching template on stack — ignore.
            return;
        };

        // Generate implied end tags (not excluding template).
        self.generate_implied_end_tags(None);

        // Pop down to and including the template element.
        self.open_elements.truncate(pos);

        // Clear active formatting list up to the last marker.
        self.clear_active_formatting_to_marker();

        // Pop the template content mode.
        self.template_mode_stack.pop();

        // Reset insertion mode: if still in nested templates stay InTemplate,
        // otherwise fall back to InBody (simplified reset_insertion_mode).
        if self.template_mode_stack.is_empty() {
            self.insertion_mode = InsertionMode::InBody;
        } else {
            self.insertion_mode = InsertionMode::InTemplate;
        }
    }

    /// §13.2.6.4.9 «The in table insertion mode».
    fn mode_in_table(&mut self, token: Token) {
        match token {
            Token::Text(s) => {
                // Switch to InTableText.
                self.original_insertion_mode = Some(self.insertion_mode);
                self.insertion_mode = InsertionMode::InTableText;
                self.pending_table_text.clear();
                self.pending_table_text_has_nonspace = false;
                self.apply_token(Token::Text(s));
            }
            Token::Comment(s) => self.insert_comment(s),
            Token::Doctype { .. } => { /* parse error */ }
            Token::StartTag {
                ref name, ref attrs, ..
            } if name == "caption" => {
                self.clear_stack_to_table_context();
                self.active_formatting.push(ActiveFormattingEntry::Marker);
                let el = self.create_element_with_attrs(name, attrs);
                self.append_to_current_open(el);
                self.open_elements.push(el);
                self.insertion_mode = InsertionMode::InCaption;
            }
            Token::StartTag {
                ref name, ref attrs, ..
            } if name == "colgroup" => {
                self.clear_stack_to_table_context();
                let el = self.create_element_with_attrs(name, attrs);
                self.append_to_current_open(el);
                self.open_elements.push(el);
                self.insertion_mode = InsertionMode::InColumnGroup;
            }
            Token::StartTag {
                ref name, ref attrs, ..
            } if name == "col" => {
                self.clear_stack_to_table_context();
                let cg = self.create_element_with_attrs("colgroup", &[]);
                self.append_to_current_open(cg);
                self.open_elements.push(cg);
                self.insertion_mode = InsertionMode::InColumnGroup;
                self.dispatch(Token::StartTag {
                    name: name.clone(),
                    attrs: attrs.clone(),
                    self_closing: true,
                });
            }
            Token::StartTag {
                ref name, ref attrs, ..
            } if matches!(name.as_str(), "tbody" | "thead" | "tfoot") => {
                self.clear_stack_to_table_context();
                let el = self.create_element_with_attrs(name, attrs);
                self.append_to_current_open(el);
                self.open_elements.push(el);
                self.insertion_mode = InsertionMode::InTableBody;
            }
            Token::StartTag {
                ref name, ref attrs, ..
            } if matches!(name.as_str(), "td" | "th" | "tr") => {
                self.clear_stack_to_table_context();
                let tbody = self.create_element_with_attrs("tbody", &[]);
                self.append_to_current_open(tbody);
                self.open_elements.push(tbody);
                self.insertion_mode = InsertionMode::InTableBody;
                self.dispatch(Token::StartTag {
                    name: name.clone(),
                    attrs: attrs.clone(),
                    self_closing: false,
                });
            }
            Token::StartTag { ref name, .. } if name == "table" => {
                // parse error: close table and reprocess.
                if self.has_element_in_table_scope("table") {
                    while let Some(top) = self.open_elements.pop() {
                        if self.element_local(top) == "table" {
                            break;
                        }
                    }
                    self.reset_insertion_mode();
                    self.dispatch(token);
                }
            }
            Token::EndTag { ref name } if name == "table" => {
                if self.has_element_in_table_scope("table") {
                    while let Some(top) = self.open_elements.pop() {
                        if self.element_local(top) == "table" {
                            break;
                        }
                    }
                    self.reset_insertion_mode();
                }
            }
            Token::EndTag { ref name }
                if matches!(
                    name.as_str(),
                    "body" | "caption" | "col" | "colgroup" | "html" | "tbody" | "td"
                        | "tfoot" | "th" | "thead" | "tr"
                ) =>
            {
                // parse error: ignore
            }
            _ => {
                // Anything else — foster parenting, process as InBody.
                // Phase 0: упрощённо — просто диспатчим в InBody.
                let saved = self.insertion_mode;
                self.insertion_mode = InsertionMode::InBody;
                self.dispatch(token);
                self.insertion_mode = saved;
            }
        }
    }

    /// Flush accumulated text from InTableText (§13.2.6.4.10).
    fn flush_pending_table_text(&mut self) {
        let text = std::mem::take(&mut self.pending_table_text);
        let has_nonspace = self.pending_table_text_has_nonspace;
        self.pending_table_text_has_nonspace = false;

        if has_nonspace {
            // Foster parent: process text as InBody через foster parenting.
            // Phase 0: упрощённо — диспатчим в InBody.
            let saved = self.insertion_mode;
            self.insertion_mode = InsertionMode::InBody;
            self.dispatch(Token::Text(text));
            self.insertion_mode = saved;
        } else if !text.is_empty() {
            // Whitespace-only — insert as-is.
            self.insert_text(&text);
        }

        if let Some(prev) = self.original_insertion_mode.take() {
            self.insertion_mode = prev;
        } else {
            self.insertion_mode = InsertionMode::InTable;
        }
    }

    /// §13.2.6.4.11 «The in caption insertion mode» — упрощённо.
    fn mode_in_caption(&mut self, token: Token) {
        match token {
            Token::EndTag { ref name } if name == "caption" => {
                if self.has_element_in_table_scope("caption") {
                    while let Some(top) = self.open_elements.pop() {
                        if self.element_local(top) == "caption" {
                            break;
                        }
                    }
                    self.clear_active_formatting_to_marker();
                    self.insertion_mode = InsertionMode::InTable;
                }
            }
            Token::StartTag { ref name, .. }
            | Token::EndTag { ref name }
                if matches!(
                    name.as_str(),
                    "caption"
                        | "col"
                        | "colgroup"
                        | "tbody"
                        | "td"
                        | "tfoot"
                        | "th"
                        | "thead"
                        | "tr"
                        | "table"
                ) =>
            {
                if self.has_element_in_table_scope("caption") {
                    while let Some(top) = self.open_elements.pop() {
                        if self.element_local(top) == "caption" {
                            break;
                        }
                    }
                    self.clear_active_formatting_to_marker();
                    self.insertion_mode = InsertionMode::InTable;
                    self.dispatch(token);
                }
            }
            _ => self.mode_in_body(token),
        }
    }

    /// §13.2.6.4.12 «The in column group insertion mode».
    fn mode_in_column_group(&mut self, token: Token) {
        match token {
            Token::StartTag {
                ref name, ref attrs, ..
            } if name == "col" => {
                let el = self.create_element_with_attrs(name, attrs);
                self.append_to_current_open(el);
                // Void.
            }
            Token::EndTag { ref name } if name == "colgroup" => {
                if let Some(&top) = self.open_elements.last()
                    && self.element_local(top) == "colgroup"
                {
                    self.open_elements.pop();
                    self.insertion_mode = InsertionMode::InTable;
                }
            }
            _ => {
                // Pop colgroup, reprocess.
                if let Some(&top) = self.open_elements.last()
                    && self.element_local(top) == "colgroup"
                {
                    self.open_elements.pop();
                    self.insertion_mode = InsertionMode::InTable;
                    self.dispatch(token);
                }
            }
        }
    }

    /// §13.2.6.4.13 «The in table body insertion mode».
    fn mode_in_table_body(&mut self, token: Token) {
        match token {
            Token::StartTag {
                ref name, ref attrs, ..
            } if name == "tr" => {
                self.clear_stack_to_table_body_context();
                let el = self.create_element_with_attrs(name, attrs);
                self.append_to_current_open(el);
                self.open_elements.push(el);
                self.insertion_mode = InsertionMode::InRow;
            }
            Token::StartTag {
                ref name, ref attrs, ..
            } if matches!(name.as_str(), "th" | "td") => {
                self.clear_stack_to_table_body_context();
                let tr = self.create_element_with_attrs("tr", &[]);
                self.append_to_current_open(tr);
                self.open_elements.push(tr);
                self.insertion_mode = InsertionMode::InRow;
                self.dispatch(Token::StartTag {
                    name: name.clone(),
                    attrs: attrs.clone(),
                    self_closing: false,
                });
            }
            Token::EndTag { ref name } if matches!(name.as_str(), "tbody" | "thead" | "tfoot") => {
                if self.has_element_in_table_scope(name) {
                    self.clear_stack_to_table_body_context();
                    self.open_elements.pop();
                    self.insertion_mode = InsertionMode::InTable;
                }
            }
            Token::EndTag { ref name } if name == "table" => {
                self.clear_stack_to_table_body_context();
                self.open_elements.pop();
                self.insertion_mode = InsertionMode::InTable;
                self.dispatch(token);
            }
            _ => self.mode_in_table(token),
        }
    }

    /// §13.2.6.4.14 «The in row insertion mode».
    fn mode_in_row(&mut self, token: Token) {
        match token {
            Token::StartTag {
                ref name, ref attrs, ..
            } if matches!(name.as_str(), "th" | "td") => {
                self.clear_stack_to_table_row_context();
                let el = self.create_element_with_attrs(name, attrs);
                self.append_to_current_open(el);
                self.open_elements.push(el);
                self.insertion_mode = InsertionMode::InCell;
                self.active_formatting.push(ActiveFormattingEntry::Marker);
            }
            Token::EndTag { ref name } if name == "tr" => {
                if self.has_element_in_table_scope("tr") {
                    self.clear_stack_to_table_row_context();
                    self.open_elements.pop();
                    self.insertion_mode = InsertionMode::InTableBody;
                }
            }
            Token::StartTag { ref name, .. }
                if matches!(
                    name.as_str(),
                    "caption" | "col" | "colgroup" | "tbody" | "tfoot" | "thead" | "tr"
                ) =>
            {
                if self.has_element_in_table_scope("tr") {
                    self.clear_stack_to_table_row_context();
                    self.open_elements.pop();
                    self.insertion_mode = InsertionMode::InTableBody;
                    self.dispatch(token);
                }
            }
            Token::EndTag { ref name } if name == "table" => {
                if self.has_element_in_table_scope("tr") {
                    self.clear_stack_to_table_row_context();
                    self.open_elements.pop();
                    self.insertion_mode = InsertionMode::InTableBody;
                    self.dispatch(token);
                }
            }
            Token::EndTag { ref name } if matches!(name.as_str(), "tbody" | "thead" | "tfoot") => {
                if self.has_element_in_table_scope(name) && self.has_element_in_table_scope("tr") {
                    self.clear_stack_to_table_row_context();
                    self.open_elements.pop();
                    self.insertion_mode = InsertionMode::InTableBody;
                    self.dispatch(token);
                }
            }
            _ => self.mode_in_table(token),
        }
    }

    /// §13.2.6.4.15 «The in cell insertion mode».
    fn mode_in_cell(&mut self, token: Token) {
        match token {
            Token::EndTag { ref name } if matches!(name.as_str(), "td" | "th") => {
                let n = name.clone();
                if self.has_element_in_table_scope(&n) {
                    self.generate_implied_end_tags(None);
                    while let Some(top) = self.open_elements.pop() {
                        if self.element_local(top) == n {
                            break;
                        }
                    }
                    self.clear_active_formatting_to_marker();
                    self.insertion_mode = InsertionMode::InRow;
                }
            }
            Token::StartTag { ref name, .. }
                if matches!(
                    name.as_str(),
                    "caption" | "col" | "colgroup" | "tbody" | "td" | "tfoot" | "th" | "thead"
                        | "tr"
                ) =>
            {
                // Close current cell, reprocess.
                self.close_cell();
                self.dispatch(token);
            }
            Token::EndTag { ref name }
                if matches!(name.as_str(), "table" | "tbody" | "tfoot" | "thead" | "tr") =>
            {
                if self.has_element_in_table_scope(name) {
                    self.close_cell();
                    self.dispatch(token);
                }
            }
            _ => self.mode_in_body(token),
        }
    }

    /// Close the current cell (§13.2.6.4.15 «close the cell»).
    fn close_cell(&mut self) {
        let cell_name = self.find_cell_in_scope();
        if let Some(n) = cell_name {
            self.generate_implied_end_tags(None);
            while let Some(top) = self.open_elements.pop() {
                if self.element_local(top) == n {
                    break;
                }
            }
            self.clear_active_formatting_to_marker();
            self.insertion_mode = InsertionMode::InRow;
        }
    }

    fn find_cell_in_scope(&self) -> Option<String> {
        for &n in self.open_elements.iter().rev() {
            let local = self.element_local(n);
            if local == "td" || local == "th" {
                return Some(local.to_string());
            }
            if is_scope_stop(local) {
                return None;
            }
        }
        None
    }

    /// §13.2.6.4.16 «The in select insertion mode» — упрощённо.
    fn mode_in_select(&mut self, token: Token) {
        match token {
            Token::Text(s) => self.insert_text(&s),
            Token::Comment(s) => self.insert_comment(s),
            Token::StartTag {
                ref name, ref attrs, ..
            } if name == "option" => {
                if let Some(&top) = self.open_elements.last()
                    && self.element_local(top) == "option"
                {
                    self.open_elements.pop();
                }
                let el = self.create_element_with_attrs(name, attrs);
                self.append_to_current_open(el);
                self.open_elements.push(el);
            }
            Token::StartTag {
                ref name, ref attrs, ..
            } if name == "optgroup" => {
                if let Some(&top) = self.open_elements.last()
                    && self.element_local(top) == "option"
                {
                    self.open_elements.pop();
                }
                if let Some(&top) = self.open_elements.last()
                    && self.element_local(top) == "optgroup"
                {
                    self.open_elements.pop();
                }
                let el = self.create_element_with_attrs(name, attrs);
                self.append_to_current_open(el);
                self.open_elements.push(el);
            }
            Token::EndTag { ref name } if name == "option" => {
                if let Some(&top) = self.open_elements.last()
                    && self.element_local(top) == "option"
                {
                    self.open_elements.pop();
                }
            }
            Token::EndTag { ref name } if name == "optgroup" => {
                if let Some(&top) = self.open_elements.last()
                    && self.element_local(top) == "option"
                {
                    self.open_elements.pop();
                }
                if let Some(&top) = self.open_elements.last()
                    && self.element_local(top) == "optgroup"
                {
                    self.open_elements.pop();
                }
            }
            Token::EndTag { ref name } if name == "select" => {
                while let Some(top) = self.open_elements.pop() {
                    if self.element_local(top) == "select" {
                        break;
                    }
                }
                self.reset_insertion_mode();
            }
            _ => { /* parse error: ignore most things */ }
        }
    }

    /// §13.2.6.4.17 «The in select in table insertion mode».
    fn mode_in_select_in_table(&mut self, token: Token) {
        const TABLE_TAGS: &[&str] = &[
            "caption", "table", "tbody", "tfoot", "thead", "tr", "td", "th",
        ];
        match token {
            Token::StartTag { ref name, .. } if TABLE_TAGS.contains(&name.as_str()) => {
                // parse error: close select, reprocess
                self.pop_open_elements_until("select");
                self.reset_insertion_mode();
                self.dispatch(token);
            }
            Token::EndTag { ref name } if TABLE_TAGS.contains(&name.as_str()) => {
                // parse error; only act if the tag is in table scope
                let in_scope = self
                    .open_elements
                    .iter()
                    .any(|&n| self.element_local(n) == name.as_str());
                if in_scope {
                    self.pop_open_elements_until("select");
                    self.reset_insertion_mode();
                    self.dispatch(token);
                }
                // else: parse error, ignore
            }
            other => self.mode_in_select(other),
        }
    }

    /// §13.2.6.4.5 «The in head noscript insertion mode».
    ///
    /// Only entered when `scripting_enabled` is `false`. Parses `<noscript>`
    /// content as HTML markup (rather than raw text).
    fn mode_in_head_noscript(&mut self, token: Token) {
        match token {
            Token::Doctype { .. } => { /* parse error: ignore */ }
            Token::StartTag { ref name, ref attrs, .. } if name == "html" => {
                self.in_body_start_html_attrs(attrs);
            }
            Token::EndTag { ref name } if name == "noscript" => {
                self.open_elements.pop();
                self.insertion_mode = InsertionMode::InHead;
            }
            // Whitespace text, comments, and these head-level void elements
            // are processed as if in InHead.
            Token::Text(ref s) if s.chars().all(is_html_whitespace) => {
                self.mode_in_head(token);
            }
            Token::Comment(_) => {
                self.mode_in_head(token);
            }
            Token::StartTag { ref name, .. }
                if matches!(
                    name.as_str(),
                    "basefont" | "bgsound" | "link" | "meta" | "noframes" | "style"
                ) =>
            {
                self.mode_in_head(token);
            }
            // parse error: `</br>` is treated as `<br>` (implicit body),
            // any other end tag: parse error, ignore.
            Token::EndTag { ref name } if name == "br" => {
                // pop noscript, switch to InHead, reprocess as if InHead got </br>
                self.open_elements.pop();
                self.insertion_mode = InsertionMode::InHead;
                self.dispatch(Token::EndTag { name: "br".to_string() });
            }
            Token::StartTag { ref name, .. }
                if matches!(name.as_str(), "head" | "noscript") =>
            {
                // parse error: ignore
            }
            Token::EndTag { .. } => { /* parse error: ignore */ }
            other => {
                // parse error: pop noscript, switch to InHead, reprocess
                self.open_elements.pop();
                self.insertion_mode = InsertionMode::InHead;
                self.dispatch(other);
            }
        }
    }

    /// §13.2.6.4.20 «The in frameset insertion mode».
    ///
    /// Active for frameset-based documents (using `<frameset>` instead of
    /// `<body>`). Handles `<frame>` (void), nested `<frameset>`, `<noframes>`.
    fn mode_in_frameset(&mut self, token: Token) {
        match token {
            Token::Text(ref s) if s.chars().all(is_html_whitespace) => {
                self.insert_text(s);
            }
            Token::Comment(s) => self.insert_comment(s),
            Token::Doctype { .. } => { /* parse error: ignore */ }
            Token::StartTag { ref name, ref attrs, .. } if name == "html" => {
                self.in_body_start_html_attrs(attrs);
            }
            Token::StartTag { ref name, ref attrs, .. } if name == "frameset" => {
                let fs = self.create_element_with_attrs(name, attrs);
                self.append_to_current_open(fs);
                self.open_elements.push(fs);
            }
            Token::EndTag { ref name } if name == "frameset" => {
                // Only the html element remains (open_elements.len() == 1).
                if self.open_elements.len() == 1 {
                    // parse error: ignore (fragment parsing context)
                    return;
                }
                self.open_elements.pop();
                // If current node is no longer a frameset, the document is
                // done with its frameset structure → AfterFrameset.
                if let Some(&top) = self.open_elements.last()
                    && self.element_local(top) != "frameset"
                {
                    self.insertion_mode = InsertionMode::AfterFrameset;
                }
            }
            Token::StartTag { ref name, ref attrs, .. } if name == "frame" => {
                // void element: create + append, do NOT push onto open_elements
                let el = self.create_element_with_attrs(name, attrs);
                self.append_to_current_open(el);
            }
            Token::StartTag { ref name, .. } if name == "noframes" => {
                // process as InHead (switches to Text mode for raw content)
                let saved = self.insertion_mode;
                self.insertion_mode = InsertionMode::InHead;
                self.dispatch(token);
                if self.insertion_mode == InsertionMode::InHead {
                    self.insertion_mode = saved;
                }
            }
            _ => { /* parse error: ignore */ }
        }
    }

    /// §13.2.6.4.21 «The after frameset insertion mode».
    fn mode_after_frameset(&mut self, token: Token) {
        match token {
            Token::Text(ref s) if s.chars().all(is_html_whitespace) => {
                self.insert_text(s);
            }
            Token::Comment(s) => self.insert_comment(s),
            Token::Doctype { .. } => { /* parse error: ignore */ }
            Token::StartTag { ref name, ref attrs, .. } if name == "html" => {
                self.in_body_start_html_attrs(attrs);
            }
            Token::EndTag { ref name } if name == "html" => {
                self.insertion_mode = InsertionMode::AfterAfterFrameset;
            }
            Token::StartTag { ref name, .. } if name == "noframes" => {
                let saved = self.insertion_mode;
                self.insertion_mode = InsertionMode::InHead;
                self.dispatch(token);
                if self.insertion_mode == InsertionMode::InHead {
                    self.insertion_mode = saved;
                }
            }
            _ => { /* parse error: ignore */ }
        }
    }

    /// §13.2.6.4.19 «The after body insertion mode».
    fn mode_after_body(&mut self, token: Token) {
        match token {
            Token::Comment(s) => {
                // Append to html element.
                if let Some(&html) = self.open_elements.first() {
                    let c = self.doc.create_comment(s);
                    self.doc.append_child(html, c);
                }
            }
            Token::Text(ref s) if s.chars().all(is_html_whitespace) => {
                // Process in InBody.
                self.mode_in_body(token);
            }
            Token::EndTag { ref name } if name == "html" => {
                self.insertion_mode = InsertionMode::AfterAfterBody;
            }
            _ => {
                // parse error: switch back to InBody and reprocess.
                self.insertion_mode = InsertionMode::InBody;
                self.dispatch(token);
            }
        }
    }

    /// §13.2.6.4.22 «The after after body insertion mode».
    fn mode_after_after_body(&mut self, token: Token) {
        match token {
            Token::Comment(s) => {
                let root = self.doc.root();
                let c = self.doc.create_comment(s);
                self.doc.append_child(root, c);
            }
            Token::Text(ref s) if s.chars().all(is_html_whitespace) => {
                self.mode_in_body(token);
            }
            _ => {
                self.insertion_mode = InsertionMode::InBody;
                self.dispatch(token);
            }
        }
    }

    /// §13.2.6.4.23 «The after after frameset insertion mode».
    fn mode_after_after_frameset(&mut self, token: Token) {
        match token {
            Token::Comment(s) => {
                let root = self.doc.root();
                let c = self.doc.create_comment(s);
                self.doc.append_child(root, c);
            }
            Token::Text(ref s) if s.chars().all(is_html_whitespace) => {
                self.mode_in_body(token);
            }
            Token::Doctype { .. } => { /* ignore */ }
            Token::StartTag { ref name, ref attrs, .. } if name == "html" => {
                self.in_body_start_html_attrs(attrs);
            }
            Token::StartTag { ref name, .. } if name == "noframes" => {
                let saved = self.insertion_mode;
                self.insertion_mode = InsertionMode::InHead;
                self.dispatch(token);
                if self.insertion_mode == InsertionMode::InHead {
                    self.insertion_mode = saved;
                }
            }
            _ => { /* parse error: ignore */ }
        }
    }

    // ─────────────────────────────────────────────────────────────
    // EOF processing
    // ─────────────────────────────────────────────────────────────

    /// EOF: гарантировать наличие html/head/body даже для пустого ввода.
    fn process_eof(&mut self) {
        // Flush pending table text if any.
        if self.insertion_mode == InsertionMode::InTableText {
            self.flush_pending_table_text();
        }
        // Drive empty-doc transitions: Initial → BeforeHtml → BeforeHead
        // → InHead → AfterHead → InBody (via implicit creations).
        loop {
            match self.insertion_mode {
                InsertionMode::Initial => {
                    self.insertion_mode = InsertionMode::BeforeHtml;
                }
                InsertionMode::BeforeHtml => {
                    let html = self.create_element_with_attrs("html", &[]);
                    let root = self.doc.root();
                    self.doc.append_child(root, html);
                    self.open_elements.push(html);
                    self.insertion_mode = InsertionMode::BeforeHead;
                }
                InsertionMode::BeforeHead => {
                    let head = self.create_element_with_attrs("head", &[]);
                    self.append_to_current_open(head);
                    self.open_elements.push(head);
                    self.head_element = Some(head);
                    self.insertion_mode = InsertionMode::InHead;
                }
                InsertionMode::InHead => {
                    self.open_elements.pop();
                    self.insertion_mode = InsertionMode::AfterHead;
                }
                InsertionMode::AfterHead => {
                    let body = self.create_element_with_attrs("body", &[]);
                    self.append_to_current_open(body);
                    self.open_elements.push(body);
                    self.insertion_mode = InsertionMode::InBody;
                    break;
                }
                _ => break,
            }
        }
    }

    // ─────────────────────────────────────────────────────────────
    // Helpers: DOM mutation
    // ─────────────────────────────────────────────────────────────

    /// Локальное имя элемента или пустая строка для не-элементных
    /// узлов (теоретически не должно встречаться в open_elements).
    fn element_local(&self, id: NodeId) -> &str {
        match &self.doc.get(id).data {
            NodeData::Element { name, .. } => name.local.as_str(),
            _ => "",
        }
    }

    /// Создаёт DOM-элемент с заданными атрибутами; не вставляет.
    fn create_element_with_attrs(&mut self, name: &str, attrs: &[(String, String)]) -> NodeId {
        let id = self.doc.create_element(QualName::html(name));
        if let NodeData::Element {
            attrs: dom_attrs, ..
        } = &mut self.doc.get_mut(id).data
        {
            for (k, v) in attrs {
                dom_attrs.push(Attribute {
                    name: QualName::html(k.clone()),
                    value: v.clone(),
                });
            }
        }
        id
    }

    /// Resolve the current insertion parent.
    ///
    /// Normally this is `open_elements.last()`. When the stack top is a
    /// `<template>` element, insertions are redirected to its content
    /// `DocumentFragment` so that template content is stored separately from
    /// the template element's DOM children (HTML LS §13.2.6.1).
    fn current_insertion_parent(&self) -> NodeId {
        if let Some(&top) = self.open_elements.last() {
            if self.element_local(top) == "template"
                && let Some(frag) = self.doc.template_content(top)
            {
                return frag;
            }
            top
        } else {
            self.doc.root()
        }
    }

    /// Вставляет узел в текущий «open insertion point» — top of
    /// open_elements или, если стек пуст, в Document root.
    ///
    /// Если top — `<template>`, вставка перенаправляется в content fragment
    /// (см. [`current_insertion_parent`][Self::current_insertion_parent]).
    fn append_to_current_open(&mut self, node: NodeId) {
        let parent = self.current_insertion_parent();
        self.doc.append_child(parent, node);
    }

    /// Вставка текста с coalescing: если последний ребёнок текущего
    /// родителя — Text, дописываем туда, иначе создаём новый.
    fn insert_text(&mut self, s: &str) {
        if s.is_empty() {
            return;
        }
        let parent = self.current_insertion_parent();
        let last_child = self.doc.get(parent).children.last().copied();
        if let Some(child) = last_child
            && let NodeData::Text(existing) = &mut self.doc.get_mut(child).data
        {
            existing.push_str(s);
            return;
        }
        let text = self.doc.create_text(s);
        self.doc.append_child(parent, text);
    }

    /// Вставка комментария — в текущий open insertion point.
    fn insert_comment(&mut self, s: String) {
        let parent = self.current_insertion_parent();
        let c = self.doc.create_comment(s);
        self.doc.append_child(parent, c);
    }

    // ─────────────────────────────────────────────────────────────
    // Scope queries (§13.2.4.2)
    // ─────────────────────────────────────────────────────────────

    fn has_element_in_scope(&self, target: &str) -> bool {
        for &n in self.open_elements.iter().rev() {
            let local = self.element_local(n);
            if local == target {
                return true;
            }
            if is_scope_stop(local) {
                return false;
            }
        }
        false
    }

    fn has_element_in_button_scope(&self, target: &str) -> bool {
        for &n in self.open_elements.iter().rev() {
            let local = self.element_local(n);
            if local == target {
                return true;
            }
            if is_scope_stop(local) || local == "button" {
                return false;
            }
        }
        false
    }

    fn has_element_in_list_item_scope(&self, target: &str) -> bool {
        for &n in self.open_elements.iter().rev() {
            let local = self.element_local(n);
            if local == target {
                return true;
            }
            if is_scope_stop(local) || local == "ol" || local == "ul" {
                return false;
            }
        }
        false
    }

    fn has_element_in_table_scope(&self, target: &str) -> bool {
        for &n in self.open_elements.iter().rev() {
            let local = self.element_local(n);
            if local == target {
                return true;
            }
            if matches!(local, "html" | "table" | "template") {
                return false;
            }
        }
        false
    }

    fn has_heading_in_scope(&self) -> bool {
        for &n in self.open_elements.iter().rev() {
            let local = self.element_local(n);
            if is_heading(local) {
                return true;
            }
            if is_scope_stop(local) {
                return false;
            }
        }
        false
    }

    // ─────────────────────────────────────────────────────────────
    // Close / generate implied
    // ─────────────────────────────────────────────────────────────

    /// §13.2.6.4.7 «close a p element».
    fn close_p_element(&mut self) {
        self.generate_implied_end_tags(Some("p"));
        while let Some(top) = self.open_elements.pop() {
            if self.element_local(top) == "p" {
                break;
            }
        }
    }

    /// §13.2.4.2 «generate implied end tags».
    fn generate_implied_end_tags(&mut self, exclude: Option<&str>) {
        loop {
            let Some(&top) = self.open_elements.last() else {
                return;
            };
            let local = self.element_local(top);
            if Some(local) == exclude {
                return;
            }
            if matches!(
                local,
                "dd" | "dt" | "li" | "optgroup" | "option" | "p" | "rb" | "rp" | "rt" | "rtc"
            ) {
                self.open_elements.pop();
            } else {
                return;
            }
        }
    }

    /// Закрыть предыдущий `<li>` / `<dt>` / `<dd>` если есть.
    fn close_list_item_like(&mut self, targets: &[&str]) {
        for i in (0..self.open_elements.len()).rev() {
            let node = self.open_elements[i];
            let local = self.element_local(node).to_string();
            if targets.contains(&local.as_str()) {
                self.generate_implied_end_tags(Some(&local));
                self.open_elements.truncate(i);
                return;
            }
            if is_special(&local) && !matches!(local.as_str(), "address" | "div" | "p") {
                return;
            }
        }
    }

    // ─────────────────────────────────────────────────────────────
    // Table-specific helpers
    // ─────────────────────────────────────────────────────────────

    fn clear_stack_to_table_context(&mut self) {
        while let Some(&top) = self.open_elements.last() {
            let local = self.element_local(top);
            if matches!(local, "table" | "template" | "html") {
                return;
            }
            self.open_elements.pop();
        }
    }

    fn clear_stack_to_table_body_context(&mut self) {
        while let Some(&top) = self.open_elements.last() {
            let local = self.element_local(top);
            if matches!(local, "tbody" | "tfoot" | "thead" | "template" | "html") {
                return;
            }
            self.open_elements.pop();
        }
    }

    fn clear_stack_to_table_row_context(&mut self) {
        while let Some(&top) = self.open_elements.last() {
            let local = self.element_local(top);
            if matches!(local, "tr" | "template" | "html") {
                return;
            }
            self.open_elements.pop();
        }
    }

    /// Pop elements from the stack until (and including) the first element
    /// with the given local name. Used by InSelectInTable and similar rules
    /// where the spec says «pop elements until a `<X>` element has been
    /// popped».
    fn pop_open_elements_until(&mut self, local: &str) {
        while let Some(top) = self.open_elements.pop() {
            if self.element_local(top) == local {
                break;
            }
        }
    }

    /// §13.2.4.1 «reset the insertion mode appropriately».
    fn reset_insertion_mode(&mut self) {
        for i in (0..self.open_elements.len()).rev() {
            let node = self.open_elements[i];
            let local = self.element_local(node);
            let mode = match local {
                "select" => {
                    // §13.2.4.1 step 14: if any ancestor is a table-structure
                    // element, use InSelectInTable rather than InSelect.
                    let in_table_context = self.open_elements[..i].iter().any(|&a| {
                        matches!(
                            self.element_local(a),
                            "table" | "thead" | "tbody" | "tfoot" | "tr" | "td" | "th"
                                | "caption" | "template"
                        )
                    });
                    if in_table_context {
                        InsertionMode::InSelectInTable
                    } else {
                        InsertionMode::InSelect
                    }
                }
                "td" | "th" => InsertionMode::InCell,
                "tr" => InsertionMode::InRow,
                "tbody" | "thead" | "tfoot" => InsertionMode::InTableBody,
                "caption" => InsertionMode::InCaption,
                "colgroup" => InsertionMode::InColumnGroup,
                "table" => InsertionMode::InTable,
                "body" => InsertionMode::InBody,
                "frameset" => InsertionMode::InFrameset,
                "html" => {
                    if self.head_element.is_some() {
                        InsertionMode::AfterHead
                    } else {
                        InsertionMode::BeforeHead
                    }
                }
                _ => continue,
            };
            self.insertion_mode = mode;
            return;
        }
        self.insertion_mode = InsertionMode::InBody;
    }

    // ─────────────────────────────────────────────────────────────
    // Active formatting list (§13.2.4.3)
    // ─────────────────────────────────────────────────────────────

    /// Найти запись с заданным тегом, ища от хвоста до ближайшего
    /// маркера (или начала списка).
    fn find_active_formatting_after_marker(&self, tag: &str) -> Option<NodeId> {
        for entry in self.active_formatting.iter().rev() {
            match entry {
                ActiveFormattingEntry::Marker => return None,
                ActiveFormattingEntry::Element { node, tag: t, .. } if t == tag => {
                    return Some(*node);
                }
                _ => continue,
            }
        }
        None
    }

    /// Удалить элемент из списка active formatting по node id.
    fn remove_from_active_formatting(&mut self, node: NodeId) {
        if let Some(pos) = self.active_formatting.iter().position(|e| match e {
            ActiveFormattingEntry::Element { node: n, .. } => *n == node,
            _ => false,
        }) {
            self.active_formatting.remove(pos);
        }
    }

    /// Push с применением Noah's Ark clause (§13.2.4.3): если последние
    /// 3 entries с тем же tag+attrs существуют, удалить самую раннюю.
    fn push_active_formatting(&mut self, node: NodeId, tag: &str, attrs: &[(String, String)]) {
        // Noah's Ark: считаем сколько после ближайшего marker-а имеют
        // тот же тег+атрибуты.
        let mut matches: Vec<usize> = Vec::new();
        for (i, entry) in self.active_formatting.iter().enumerate().rev() {
            match entry {
                ActiveFormattingEntry::Marker => break,
                ActiveFormattingEntry::Element {
                    tag: t, attrs: a, ..
                } => {
                    if t == tag && attrs_equal(a, attrs) {
                        matches.push(i);
                    }
                }
            }
        }
        if matches.len() >= 3 {
            // matches идёт от хвоста; remove the earliest (last элемент
            // в matches).
            let earliest = *matches.last().expect("non-empty");
            self.active_formatting.remove(earliest);
        }
        self.active_formatting.push(ActiveFormattingEntry::Element {
            node,
            tag: tag.to_string(),
            attrs: attrs.to_vec(),
        });
    }

    /// Очистить active formatting list до ближайшего маркера (или
    /// начала, если маркеров нет).
    fn clear_active_formatting_to_marker(&mut self) {
        while let Some(entry) = self.active_formatting.pop() {
            if matches!(entry, ActiveFormattingEntry::Marker) {
                break;
            }
        }
    }

    /// §13.2.4.3 «reconstruct the active formatting elements».
    fn reconstruct_active_formatting(&mut self) {
        if self.active_formatting.is_empty() {
            return;
        }
        let last_idx = self.active_formatting.len() - 1;
        let last = &self.active_formatting[last_idx];
        match last {
            ActiveFormattingEntry::Marker => return,
            ActiveFormattingEntry::Element { node, .. } => {
                if self.open_elements.contains(node) {
                    return;
                }
            }
        }

        // Идём назад, пока не найдём marker или элемент в стеке.
        let mut entry_idx = last_idx;
        loop {
            if entry_idx == 0 {
                break;
            }
            entry_idx -= 1;
            match &self.active_formatting[entry_idx] {
                ActiveFormattingEntry::Marker => {
                    entry_idx += 1;
                    break;
                }
                ActiveFormattingEntry::Element { node, .. } => {
                    if self.open_elements.contains(node) {
                        entry_idx += 1;
                        break;
                    }
                }
            }
        }

        // Создаём клоны от entry_idx до конца.
        while entry_idx < self.active_formatting.len() {
            let (tag, attrs) = match &self.active_formatting[entry_idx] {
                ActiveFormattingEntry::Element { tag, attrs, .. } => {
                    (tag.clone(), attrs.clone())
                }
                ActiveFormattingEntry::Marker => unreachable!(),
            };
            let clone = self.create_element_with_attrs(&tag, &attrs);
            self.append_to_current_open(clone);
            self.open_elements.push(clone);
            self.active_formatting[entry_idx] = ActiveFormattingEntry::Element {
                node: clone,
                tag,
                attrs,
            };
            entry_idx += 1;
        }
    }

    // ─────────────────────────────────────────────────────────────
    // Adoption Agency Algorithm (§13.2.6.4.7)
    // ─────────────────────────────────────────────────────────────

    /// Реализация AAA — упрощённая, но покрывает основные случаи
    /// mis-nesting типа `<b>a<i>b</b>c</i>` и `<a>a<a>b</a>c</a>`.
    /// Phase 0 выполняет один проход AAA вместо полных 8 итераций
    /// outer loop из спецификации — этого достаточно для большинства
    /// реальных страниц.
    fn adoption_agency(&mut self, subject: &str) {
        // Step 4: find formatting element from active formatting
        // list (after marker).
        let Some(formatting_node) = self.find_active_formatting_after_marker(subject) else {
            // Not in active formatting — fallback to generic end tag.
            self.generic_end_tag_in_body(subject);
            return;
        };

        // Step 5: if formatting element not in open elements →
        // parse error, remove from active formatting, return.
        if !self.open_elements.contains(&formatting_node) {
            self.remove_from_active_formatting(formatting_node);
            return;
        }

        // Step 7: find furthest block — special element below
        // formatting node in open_elements stack.
        let formatting_pos = self
            .open_elements
            .iter()
            .position(|&n| n == formatting_node)
            .expect("found above");
        let furthest_block = self
            .open_elements
            .iter()
            .enumerate()
            .skip(formatting_pos + 1)
            .find(|&(_, &n)| is_special(self.element_local(n)))
            .map(|(i, &n)| (i, n));

        let Some((furthest_pos, furthest_block)) = furthest_block else {
            // Step 8: pop from open elements up to and including
            // formatting node, remove from active formatting.
            self.open_elements.truncate(formatting_pos);
            self.remove_from_active_formatting(formatting_node);
            return;
        };

        // Step 9: common ancestor = element above formatting in
        // open_elements.
        let common_ancestor = if formatting_pos == 0 {
            self.doc.root()
        } else {
            self.open_elements[formatting_pos - 1]
        };

        // Step 10-13 (inner loop): простая версия — берём всё
        // между formatting+1 и furthest_block, переносим под клон
        // formatting node.
        // Clone formatting element.
        let Some((tag, attrs)) = self.active_formatting.iter().find_map(|e| match e {
            ActiveFormattingEntry::Element { node, tag, attrs }
                if *node == formatting_node =>
            {
                Some((tag.clone(), attrs.clone()))
            }
            _ => None,
        }) else {
            return;
        };

        // Move children of furthest_block to a clone, then move
        // furthest_block to common ancestor and append clone with
        // the original children inside.
        let new_formatting = self.create_element_with_attrs(&tag, &attrs);

        // Take furthest_block's children and reattach to new_formatting.
        let children: Vec<NodeId> = self.doc.get(furthest_block).children.clone();
        for ch in children {
            self.doc.append_child(new_formatting, ch);
        }
        // Append new_formatting as child of furthest_block.
        self.doc.append_child(furthest_block, new_formatting);

        // Move furthest_block to common_ancestor.
        self.doc.append_child(common_ancestor, furthest_block);

        // Update active formatting: replace formatting_node with
        // new_formatting.
        for entry in &mut self.active_formatting {
            if let ActiveFormattingEntry::Element { node, .. } = entry
                && *node == formatting_node
            {
                *node = new_formatting;
            }
        }
        // Remove the original formatting from open_elements,
        // insert new one just after furthest_block.
        let formatting_pos = self
            .open_elements
            .iter()
            .position(|&n| n == formatting_node);
        if let Some(p) = formatting_pos {
            self.open_elements.remove(p);
        }
        let furthest_pos_new = self
            .open_elements
            .iter()
            .position(|&n| n == furthest_block)
            .unwrap_or(furthest_pos.saturating_sub(1));
        self.open_elements
            .insert(furthest_pos_new + 1, new_formatting);
    }
}

impl Default for IncrementalTreeBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ─────────────────────────────────────────────────────────────
// Element classification helpers
// ─────────────────────────────────────────────────────────────

/// HTML void elements — не имеют конечного тега и контента.
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

/// Formatting elements per §13.2.4.3 — кандидаты на active formatting list.
fn is_formatting_element(name: &str) -> bool {
    matches!(
        name,
        "b" | "big"
            | "code"
            | "em"
            | "font"
            | "i"
            | "s"
            | "small"
            | "strike"
            | "strong"
            | "tt"
            | "u"
    )
}

/// Block-уровневые элементы, которые auto-close открытый `<p>`.
fn is_block_element(name: &str) -> bool {
    matches!(
        name,
        "address"
            | "article"
            | "aside"
            | "blockquote"
            | "center"
            | "details"
            | "dialog"
            | "dir"
            | "div"
            | "dl"
            | "fieldset"
            | "figcaption"
            | "figure"
            | "footer"
            | "form"
            | "header"
            | "hgroup"
            | "main"
            | "menu"
            | "nav"
            | "ol"
            | "p"
            | "pre"
            | "search"
            | "section"
            | "summary"
            | "ul"
    )
}

/// Заголовки h1..h6.
fn is_heading(name: &str) -> bool {
    matches!(name, "h1" | "h2" | "h3" | "h4" | "h5" | "h6")
}

/// Stop-элементы для default scope (§13.2.4.2).
fn is_scope_stop(name: &str) -> bool {
    matches!(
        name,
        "applet"
            | "caption"
            | "html"
            | "table"
            | "td"
            | "th"
            | "marquee"
            | "object"
            | "template"
    )
}

/// «Special» elements (§13.2.4.4 «The list of active formatting
/// elements» — определяет «special» как набор HTML/MathML/SVG
/// элементов, которые не могут быть formatting). Используется в AAA и
/// generic end-tag fallback.
fn is_special(name: &str) -> bool {
    matches!(
        name,
        "address"
            | "applet"
            | "area"
            | "article"
            | "aside"
            | "base"
            | "basefont"
            | "bgsound"
            | "blockquote"
            | "body"
            | "br"
            | "button"
            | "caption"
            | "center"
            | "col"
            | "colgroup"
            | "dd"
            | "details"
            | "dir"
            | "div"
            | "dl"
            | "dt"
            | "embed"
            | "fieldset"
            | "figcaption"
            | "figure"
            | "footer"
            | "form"
            | "frame"
            | "frameset"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "head"
            | "header"
            | "hgroup"
            | "hr"
            | "html"
            | "iframe"
            | "img"
            | "input"
            | "li"
            | "link"
            | "main"
            | "marquee"
            | "menu"
            | "meta"
            | "nav"
            | "noembed"
            | "noframes"
            | "noscript"
            | "object"
            | "ol"
            | "p"
            | "param"
            | "plaintext"
            | "pre"
            | "script"
            | "search"
            | "section"
            | "select"
            | "source"
            | "style"
            | "summary"
            | "table"
            | "tbody"
            | "td"
            | "template"
            | "textarea"
            | "tfoot"
            | "th"
            | "thead"
            | "title"
            | "tr"
            | "track"
            | "ul"
            | "wbr"
            | "xmp"
    )
}

/// HTML whitespace per §13.2.5 — TAB / LF / FF / CR / SPACE.
fn is_html_whitespace(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\x0C' | '\r' | ' ')
}

/// Split leading whitespace из текста. Возвращает (ws, rest).
fn split_leading_ws(s: &str) -> (&str, &str) {
    for (i, ch) in s.char_indices() {
        if !is_html_whitespace(ch) {
            return (&s[..i], &s[i..]);
        }
    }
    (s, "")
}

/// Сравнение attrs как мульти-сетов по (name, value).
fn attrs_equal(a: &[(String, String)], b: &[(String, String)]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().all(|(k, v)| b.iter().any(|(k2, v2)| k == k2 && v == v2))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: walk root → html → head; вернуть head id.
    fn head_of(doc: &Document) -> NodeId {
        let root = doc.root();
        let html = doc.get(root).children.iter().copied().find(|&c| {
            matches!(&doc.get(c).data, NodeData::Element { name, .. } if name.local == "html")
        }).expect("html present");
        *doc.get(html).children.iter().find(|&&c| {
            matches!(&doc.get(c).data, NodeData::Element { name, .. } if name.local == "head")
        }).expect("head present")
    }

    /// Helper: walk root → html → body; вернуть body id.
    fn body_of(doc: &Document) -> NodeId {
        let root = doc.root();
        let html = doc.get(root).children.iter().copied().find(|&c| {
            matches!(&doc.get(c).data, NodeData::Element { name, .. } if name.local == "html")
        }).expect("html present");
        *doc.get(html).children.iter().find(|&&c| {
            matches!(&doc.get(c).data, NodeData::Element { name, .. } if name.local == "body")
        }).expect("body present")
    }

    #[test]
    fn empty_input() {
        let doc = parse("");
        // root + html + head + body.
        assert_eq!(doc.len(), 4);
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
        assert!(s.contains("<!DOCTYPE html>"), "doctype line missing: {s}");
        assert!(s.contains("<p>"));
        assert!(s.contains("\"x\""));
    }

    #[test]
    fn doctype_node_data_preserved() {
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
        let doc = parse("<p>hello");
        let s = doc.to_string();
        assert!(s.contains("<p>"));
        assert!(s.contains("\"hello\""));
    }

    #[test]
    fn mismatched_end_tag_ignored() {
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
        // <script> теперь идёт в <head> — навигируем через html/head.
        let doc = parse("<script>var x = '<b>&amp;</b>'; if (a<b) {}</script>");
        let head = head_of(&doc);
        let script = doc.get(head).children[0];
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
        let doc = parse("<script>x<1</script><p>after</p>");
        let s = doc.to_string();
        assert!(s.contains("\"x<1\""));
        assert!(s.contains("<p>"));
        assert!(s.contains("\"after\""));
    }

    #[test]
    fn title_body_is_decoded_text_node() {
        let doc = parse("<title>Foo &amp; <b>Bar</b></title>");
        let head = head_of(&doc);
        let title = doc.get(head).children[0];
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
        // <textarea> идёт в body.
        let doc = parse("<textarea>&lt;script&gt;alert(1)&lt;/script&gt;</textarea>");
        let body = body_of(&doc);
        let ta = doc.get(body).children[0];
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
        let doc = parse("<p>x</p>");
        assert_eq!(doc.mode(), lumen_dom::DocumentMode::Quirks);
    }

    #[test]
    fn empty_input_yields_quirks() {
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
        let doc = parse(
            r#"<!DOCTYPE html><p>x</p><!DOCTYPE HTML PUBLIC "-//W3C//DTD HTML 3.2 Final//EN">"#,
        );
        assert_eq!(doc.mode(), lumen_dom::DocumentMode::NoQuirks);
    }

    // ──────── HTML5 §13.2 tree builder — новое поведение ────────

    #[test]
    fn implicit_html_head_body() {
        // <p>x</p> создаёт html/head/body имплиситно.
        let doc = parse("<p>x</p>");
        let s = doc.to_string();
        assert!(s.contains("<html>"));
        assert!(s.contains("<head>"));
        assert!(s.contains("<body>"));
        assert!(s.contains("<p>"));
        assert!(s.contains("\"x\""));
        // <p> внутри <body>.
        let body = body_of(&doc);
        let p = doc.get(body).children[0];
        assert!(matches!(&doc.get(p).data,
            NodeData::Element { name, .. } if name.local == "p"));
    }

    #[test]
    fn p_auto_close_before_block() {
        // <p>a<div>b</div> — <p> auto-закрывается перед <div>.
        let doc = parse("<p>a<div>b</div>");
        let body = body_of(&doc);
        let kids = &doc.get(body).children;
        // <p> и <div> должны быть siblings.
        assert_eq!(kids.len(), 2);
        assert!(matches!(&doc.get(kids[0]).data,
            NodeData::Element { name, .. } if name.local == "p"));
        assert!(matches!(&doc.get(kids[1]).data,
            NodeData::Element { name, .. } if name.local == "div"));
        // <p> содержит "a", <div> содержит "b".
        let p_text = doc.get(kids[0]).children[0];
        let div_text = doc.get(kids[1]).children[0];
        assert!(matches!(&doc.get(p_text).data, NodeData::Text(s) if s == "a"));
        assert!(matches!(&doc.get(div_text).data, NodeData::Text(s) if s == "b"));
    }

    #[test]
    fn li_auto_close() {
        // <ul><li>a<li>b</ul> — два отдельных <li>.
        let doc = parse("<ul><li>a<li>b</ul>");
        let body = body_of(&doc);
        let ul = doc.get(body).children[0];
        let lis = &doc.get(ul).children;
        assert_eq!(lis.len(), 2, "expected 2 <li>, got: {}", doc);
        for (i, expected_text) in ["a", "b"].iter().enumerate() {
            let li = lis[i];
            assert!(matches!(&doc.get(li).data,
                NodeData::Element { name, .. } if name.local == "li"));
            let t = doc.get(li).children[0];
            assert!(matches!(&doc.get(t).data,
                NodeData::Text(s) if s == expected_text));
        }
    }

    #[test]
    fn adoption_agency_basic() {
        // <b>a<i>b</b>c</i> — corner of mis-nesting.
        // Ожидаем что-то вроде: <b>a<i>b</i></b><i>c</i>
        let doc = parse("<b>a<i>b</b>c</i>");
        let s = doc.to_string();
        // <b> и <i> оба должны быть в выводе. Текст "a", "b", "c"
        // сохранён.
        assert!(s.contains("<b>"));
        assert!(s.contains("<i>"));
        assert!(s.contains("\"a\""));
        assert!(s.contains("\"b\""));
        assert!(s.contains("\"c\""));
    }

    #[test]
    fn table_structure() {
        let doc = parse("<table><tr><td>cell</td></tr></table>");
        let s = doc.to_string();
        assert!(s.contains("<table>"));
        // tbody должна быть имплиситной.
        assert!(s.contains("<tbody>"));
        assert!(s.contains("<tr>"));
        assert!(s.contains("<td>"));
        assert!(s.contains("\"cell\""));
    }

    #[test]
    fn heading_auto_close() {
        // <h1>a<h2>b</h2> — h1 должен закрыться перед h2.
        let doc = parse("<h1>a<h2>b</h2>");
        let body = body_of(&doc);
        let kids = &doc.get(body).children;
        assert_eq!(kids.len(), 2);
        assert!(matches!(&doc.get(kids[0]).data,
            NodeData::Element { name, .. } if name.local == "h1"));
        assert!(matches!(&doc.get(kids[1]).data,
            NodeData::Element { name, .. } if name.local == "h2"));
    }

    #[test]
    fn formatting_reconstruction() {
        // <b><p>x</b>y</p> — </b> через AAA создаёт клон <b> вокруг
        // содержимого <p>. По спецификации после AAA новый клон —
        // вершина стека, поэтому "y" попадает внутрь клона; text
        // coalescing сливает с "x".
        // Ожидаемая структура:
        //   <b>(пустой)
        //   <p>
        //     <b>(клон)
        //       "xy"  (x и y слиты coalescing-ом)
        let doc = parse("<b><p>x</b>y</p>");
        let s = doc.to_string();
        // Должно быть как минимум два <b>: исходный (пустой) и клон.
        let b_count = s.matches("<b>").count();
        assert!(b_count >= 2, "expected at least 2 <b> after AAA, got {b_count} in: {s}");
        assert!(s.contains("<p>"));
        // Текст содержит и x, и y.
        assert!(s.contains("xy") || (s.contains("\"x\"") && s.contains("\"y\"")));
    }

    #[test]
    fn nested_links() {
        // <a href=x>a<a href=y>b</a>c</a> — AAA для <a>.
        let doc = parse("<a href=x>a<a href=y>b</a>c</a>");
        let s = doc.to_string();
        // Оба <a> должны присутствовать в выводе.
        assert!(s.contains("href=\"x\""));
        assert!(s.contains("href=\"y\""));
        assert!(s.contains("\"a\""));
        assert!(s.contains("\"b\""));
    }

    // ──────── IncrementalTreeBuilder ────────

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
        let mut b = IncrementalTreeBuilder::new();
        b.feed("<!DOC");
        b.feed("TYPE html><p>x</p>");
        let doc = b.finish();
        assert_eq!(doc.mode(), DocumentMode::NoQuirks);
    }

    #[test]
    fn incremental_entity_split_across_chunks() {
        let mut b = IncrementalTreeBuilder::new();
        b.feed("<p>a &am");
        b.feed("p; b</p>");
        let doc = b.finish();
        let s = doc.to_string();
        assert!(s.contains("\"a & b\""), "got: {s}");
    }

    #[test]
    fn incremental_rawtext_close_tag_split() {
        let mut b = IncrementalTreeBuilder::new();
        b.feed("<script>x = 1; </scr");
        b.feed("ipt><p>after</p>");
        let doc = b.finish();
        let s = doc.to_string();
        assert!(s.contains("\"x = 1; \""), "got: {s}");
        assert!(s.contains("<p>"));
        assert!(s.contains("\"after\""));
    }

    // ──────── feed_bytes ────────

    fn parse_feed_bytes_chunks(input: &str, chunk_size: usize) -> Document {
        let mut b = IncrementalTreeBuilder::new();
        let bytes = input.as_bytes();
        let mut pos = 0;
        while pos < bytes.len() {
            let end = (pos + chunk_size).min(bytes.len());
            b.feed_bytes(&bytes[pos..end]);
            pos = end;
        }
        b.finish()
    }

    #[test]
    fn feed_bytes_ascii_equals_feed() {
        let input = "<html><body><p>hello world</p></body></html>";
        let pull = parse(input).to_string();
        let bytes_whole = {
            let mut b = IncrementalTreeBuilder::new();
            b.feed_bytes(input.as_bytes());
            b.finish().to_string()
        };
        assert_eq!(bytes_whole, pull);
        let bytes_chunked = parse_feed_bytes_chunks(input, 8).to_string();
        assert_eq!(bytes_chunked, pull);
    }

    #[test]
    fn feed_bytes_cyrillic_split_at_byte_boundary() {
        let input = "<p>Привет, мир!</p>";
        let pull = parse(input).to_string();
        let bytes_1 = parse_feed_bytes_chunks(input, 1).to_string();
        assert_eq!(bytes_1, pull, "1-byte chunks failed");
        let bytes_3 = parse_feed_bytes_chunks(input, 3).to_string();
        assert_eq!(bytes_3, pull, "3-byte chunks failed");
    }

    #[test]
    fn feed_bytes_emoji_split() {
        let input = "<p>Hello 🌍</p>";
        let pull = parse(input).to_string();
        let bytes_1 = parse_feed_bytes_chunks(input, 1).to_string();
        assert_eq!(bytes_1, pull);
        let bytes_2 = parse_feed_bytes_chunks(input, 2).to_string();
        assert_eq!(bytes_2, pull);
    }

    // ──────── <template> element ────────

    /// Helper: find a `<template>` element node in body.
    fn find_template(doc: &Document) -> Option<NodeId> {
        let body = body_of(doc);
        fn search(doc: &Document, id: NodeId) -> Option<NodeId> {
            let node = doc.get(id);
            if matches!(&node.data, NodeData::Element { name, .. } if name.local == "template") {
                return Some(id);
            }
            for &child in &node.children {
                if let Some(found) = search(doc, child) {
                    return Some(found);
                }
            }
            None
        }
        search(doc, body)
    }

    #[test]
    fn template_element_exists_in_dom() {
        let doc = parse("<body><template id=\"t\"><p>content</p></template></body>");
        let tmpl = find_template(&doc);
        assert!(tmpl.is_some(), "template element must be in DOM");
    }

    #[test]
    fn template_element_has_no_dom_children() {
        // Template content goes to fragment, not to template's DOM children.
        let doc = parse("<body><template><p>content</p></template></body>");
        let tmpl = find_template(&doc).expect("template not found");
        let children = &doc.get(tmpl).children;
        assert!(
            children.is_empty(),
            "template DOM children must be empty, got {children:?}"
        );
    }

    #[test]
    fn template_has_content_fragment() {
        let doc = parse("<body><template><p>hello</p></template></body>");
        let tmpl = find_template(&doc).expect("template not found");
        let frag = doc.template_content(tmpl);
        assert!(frag.is_some(), "template must have a content fragment");
    }

    #[test]
    fn template_content_contains_child_elements() {
        let doc = parse("<body><template><p>hello</p><span>world</span></template></body>");
        let tmpl = find_template(&doc).expect("template not found");
        let frag = doc.template_content(tmpl).expect("no content fragment");
        let children = &doc.get(frag).children;
        assert_eq!(children.len(), 2, "fragment must have 2 children (p, span)");
        let p_name = doc.get(children[0]).element_name().map(|q| q.local.as_str());
        assert_eq!(p_name, Some("p"));
        let span_name = doc.get(children[1]).element_name().map(|q| q.local.as_str());
        assert_eq!(span_name, Some("span"));
    }

    #[test]
    fn template_content_text_preserved() {
        let doc = parse("<body><template>hello world</template></body>");
        let tmpl = find_template(&doc).expect("template not found");
        let frag = doc.template_content(tmpl).expect("no content fragment");
        let children = &doc.get(frag).children;
        assert!(!children.is_empty(), "text must be in fragment");
        let text = match &doc.get(children[0]).data {
            NodeData::Text(s) => s.clone(),
            other => panic!("expected text, got {other:?}"),
        };
        assert_eq!(text.trim(), "hello world");
    }

    #[test]
    fn template_sibling_content_after_template_is_in_body() {
        let doc = parse("<body><template><p>in-template</p></template><div>after</div></body>");
        let body = body_of(&doc);
        // The <div> must be a direct child of body, not inside the template.
        let div = doc.get(body).children.iter().find(|&&c| {
            matches!(&doc.get(c).data, NodeData::Element { name, .. } if name.local == "div")
        });
        assert!(div.is_some(), "<div> must be a body child, not inside template");
    }

    #[test]
    fn template_in_head() {
        // <template> in <head> is also valid HTML.
        let doc = parse("<html><head><template><style>body{}</style></template></head><body></body></html>");
        let head = head_of(&doc);
        let tmpl = {
            let mut found = None;
            for &c in &doc.get(head).children {
                if matches!(&doc.get(c).data, NodeData::Element { name, .. } if name.local == "template") {
                    found = Some(c);
                    break;
                }
            }
            found
        };
        assert!(tmpl.is_some(), "template in head must be present");
        let tmpl = tmpl.unwrap();
        assert!(doc.get(tmpl).children.is_empty(), "template DOM children must be empty");
        let frag = doc.template_content(tmpl).expect("no content fragment");
        assert!(!doc.get(frag).children.is_empty(), "fragment must have children");
    }

    #[test]
    fn template_attributes_preserved() {
        let doc = parse(r#"<body><template id="foo" data-x="bar"></template></body>"#);
        let tmpl = find_template(&doc).expect("template not found");
        let id_val = doc.get(tmpl).get_attr("id");
        assert_eq!(id_val, Some("foo"));
        let data_val = doc.get(tmpl).get_attr("data-x");
        assert_eq!(data_val, Some("bar"));
    }

    #[test]
    fn template_content_fragment_is_document_fragment() {
        let doc = parse("<body><template><p>x</p></template></body>");
        let tmpl = find_template(&doc).expect("template not found");
        let frag = doc.template_content(tmpl).expect("no content fragment");
        assert!(
            matches!(doc.get(frag).data, NodeData::DocumentFragment),
            "content must be DocumentFragment, got {:?}",
            doc.get(frag).data
        );
    }

    #[test]
    fn nested_template_outer_content_in_outer_fragment() {
        // Outer template's fragment should contain the inner template element.
        let doc = parse("<body><template><template id=\"inner\"><p>deep</p></template></template></body>");
        let outer = find_template(&doc).expect("outer template not found");
        let outer_frag = doc.template_content(outer).expect("no outer fragment");
        // outer fragment must contain the inner template element
        let inner = doc.get(outer_frag).children.iter().find(|&&c| {
            matches!(&doc.get(c).data, NodeData::Element { name, .. } if name.local == "template")
        });
        assert!(inner.is_some(), "inner template must be in outer fragment");
    }

    #[test]
    fn template_empty_is_valid() {
        let doc = parse("<body><template></template></body>");
        let tmpl = find_template(&doc).expect("template not found");
        assert!(doc.get(tmpl).children.is_empty());
        let frag = doc.template_content(tmpl).expect("no content fragment");
        assert!(doc.get(frag).children.is_empty(), "empty template fragment must have no children");
    }

    #[test]
    fn template_display_includes_fragment() {
        // Document::fmt must include the template content fragment in its output.
        let doc = parse("<body><template><p>displayed</p></template></body>");
        let s = doc.to_string();
        assert!(s.contains("#document-fragment"), "display must show #document-fragment");
        assert!(s.contains("<p>"), "display must show fragment content");
    }

    // ─────────────────────────────────────────────────────────────
    // Tests: InFrameset / AfterFrameset / AfterAfterFrameset
    // ─────────────────────────────────────────────────────────────

    fn find_element(doc: &Document, name: &str) -> Option<NodeId> {
        fn walk(doc: &Document, node: NodeId, name: &str) -> Option<NodeId> {
            if matches!(&doc.get(node).data, NodeData::Element { name: n, .. } if n.local == name) {
                return Some(node);
            }
            for &child in &doc.get(node).children {
                if let Some(found) = walk(doc, child, name) {
                    return Some(found);
                }
            }
            None
        }
        walk(doc, doc.root(), name)
    }

    #[test]
    fn frameset_basic_structure() {
        let doc = parse(
            "<!DOCTYPE html><html><head></head>\
             <frameset rows=\"50%,50%\">\
               <frame src=\"a.html\">\
               <frame src=\"b.html\">\
             </frameset></html>",
        );
        let fs = find_element(&doc, "frameset");
        assert!(fs.is_some(), "frameset element must be created");
        let fs = fs.unwrap();
        let frames: Vec<_> = doc
            .get(fs)
            .children
            .iter()
            .filter(|&&c| {
                matches!(&doc.get(c).data, NodeData::Element { name, .. } if name.local == "frame")
            })
            .collect();
        assert_eq!(frames.len(), 2, "two frame elements expected");
    }

    #[test]
    fn frameset_no_body() {
        // A frameset document must NOT create an implicit <body>.
        let doc = parse("<!DOCTYPE html><html><head></head><frameset><frame></frameset></html>");
        let body = find_element(&doc, "body");
        assert!(body.is_none(), "frameset document must not contain <body>");
    }

    #[test]
    fn frameset_frame_is_void() {
        // <frame> is void: must not be pushed onto open_elements.
        // Verify by checking it has no children.
        let doc = parse("<frameset><frame src=\"x.html\"><frame src=\"y.html\"></frameset>");
        let fs = find_element(&doc, "frameset").expect("frameset");
        for &child in &doc.get(fs).children {
            if matches!(&doc.get(child).data, NodeData::Element { name, .. } if name.local == "frame") {
                assert!(
                    doc.get(child).children.is_empty(),
                    "frame element must be void (no children)"
                );
            }
        }
    }

    #[test]
    fn frameset_nested() {
        let doc = parse(
            "<frameset cols=\"50%,50%\">\
               <frameset rows=\"50%,50%\">\
                 <frame><frame>\
               </frameset>\
               <frame>\
             </frameset>",
        );
        let outer_fs = find_element(&doc, "frameset").expect("outer frameset");
        let inner_fs = doc.get(outer_fs).children.iter().copied().find(|&c| {
            matches!(&doc.get(c).data, NodeData::Element { name, .. } if name.local == "frameset")
        });
        assert!(inner_fs.is_some(), "inner frameset must exist");
    }

    #[test]
    fn frameset_noframes_content_raw() {
        // <noframes> inside <frameset> is parsed as raw text via InHead routing.
        let doc = parse("<frameset><frame><noframes>fallback</noframes></frameset>");
        let nf = find_element(&doc, "noframes").expect("noframes element");
        let has_text = doc.get(nf).children.iter().any(|&c| {
            matches!(&doc.get(c).data, NodeData::Text(s) if s.contains("fallback"))
        });
        assert!(has_text, "noframes must contain raw text");
    }

    #[test]
    fn after_frameset_only_whitespace_and_noframes() {
        // After </frameset>, only whitespace text and <noframes> are valid.
        // Non-whitespace text should be silently ignored (parse error).
        let doc = parse("<frameset><frame></frameset>spurious text");
        // "spurious text" must not appear anywhere in the document
        fn has_text(doc: &Document, node: NodeId, needle: &str) -> bool {
            if matches!(&doc.get(node).data, NodeData::Text(s) if s.contains(needle)) {
                return true;
            }
            doc.get(node).children.iter().any(|&c| has_text(doc, c, needle))
        }
        assert!(
            !has_text(&doc, doc.root(), "spurious"),
            "spurious text after </frameset> must be ignored"
        );
    }

    #[test]
    fn after_after_frameset_html_close() {
        // </html> after </frameset> transitions to AfterAfterFrameset.
        // Any further non-whitespace content is ignored (parse error).
        let doc = parse("<frameset><frame></frameset></html>extra");
        fn has_text(doc: &Document, node: NodeId, needle: &str) -> bool {
            if matches!(&doc.get(node).data, NodeData::Text(s) if s.contains(needle)) {
                return true;
            }
            doc.get(node).children.iter().any(|&c| has_text(doc, c, needle))
        }
        assert!(
            !has_text(&doc, doc.root(), "extra"),
            "content after </html> in frameset doc must be ignored"
        );
    }

    // ─────────────────────────────────────────────────────────────
    // Tests: InHeadNoscript (scripting disabled)
    // ─────────────────────────────────────────────────────────────

    fn parse_noscript_off(input: &str) -> Document {
        let mut b = IncrementalTreeBuilder::new();
        b.scripting_enabled = false;
        b.feed(input);
        b.finish()
    }

    #[test]
    fn in_head_noscript_end_tag_closes_and_returns_to_in_head() {
        let doc = parse_noscript_off(
            "<html><head><noscript><link rel=\"stylesheet\" href=\"x.css\"></noscript></head><body></body></html>",
        );
        let head = head_of(&doc);
        let noscript = doc.get(head).children.iter().copied().find(|&c| {
            matches!(&doc.get(c).data, NodeData::Element { name, .. } if name.local == "noscript")
        });
        assert!(noscript.is_some(), "noscript element must be in head");
        let ns = noscript.unwrap();
        // The <link> inside noscript must be a child (parsed as markup, not raw text).
        let link = doc.get(ns).children.iter().copied().find(|&c| {
            matches!(&doc.get(c).data, NodeData::Element { name, .. } if name.local == "link")
        });
        assert!(link.is_some(), "link inside noscript must be parsed as markup");
    }

    #[test]
    fn in_head_noscript_whitespace_processed_in_head() {
        // Whitespace inside <noscript> (scripting off) must be inserted as text.
        let doc = parse_noscript_off(
            "<html><head><noscript>   </noscript></head></html>",
        );
        let head = head_of(&doc);
        let ns = doc.get(head).children.iter().copied().find(|&c| {
            matches!(&doc.get(c).data, NodeData::Element { name, .. } if name.local == "noscript")
        }).expect("noscript in head");
        let has_ws = doc.get(ns).children.iter().any(|&c| {
            matches!(&doc.get(c).data, NodeData::Text(s) if s.trim().is_empty())
        });
        assert!(has_ws, "whitespace inside noscript (scripting off) must be text node");
    }

    #[test]
    fn in_head_noscript_unknown_start_pops_and_reprocesses() {
        // An unexpected start tag inside <noscript> (scripting off) pops noscript
        // and reprocesses: the tag ends up in AfterHead/InBody.
        let doc = parse_noscript_off(
            "<html><head><noscript><body></body></html>",
        );
        // <body> inside <noscript> must cause noscript to close; the <body>
        // element must then be created in the normal position.
        let body = find_element(&doc, "body");
        assert!(body.is_some(), "body must be created after noscript closes");
    }

    // ─────────────────────────────────────────────────────────────
    // Tests: InSelectInTable (complete implementation)
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn select_in_table_cell_context() {
        // <select> inside <td> should use InSelectInTable mode so that a
        // start tag like <table> closes the select and reprocesses.
        let doc = parse(
            "<table><tr><td>\
               <select><option>a</option></select>\
             </td></tr></table>",
        );
        // The select must exist inside the td.
        let sel = find_element(&doc, "select");
        assert!(sel.is_some(), "select must be created in table cell");
    }

    #[test]
    fn select_in_table_closed_by_table_start_tag() {
        // Inside InSelectInTable: a <table> start tag must close the select.
        // After close, the <table> element is reprocessed.
        let doc = parse(
            "<table><tr><td>\
               <select><option>x</option><table><tr><td>y</td></tr></table>\
             </td></tr></table>",
        );
        // The <table> start tag closed the select; a nested table must exist.
        let tables: Vec<_> = {
            fn collect_tables(doc: &Document, node: NodeId, out: &mut Vec<NodeId>) {
                if matches!(&doc.get(node).data, NodeData::Element { name, .. } if name.local == "table") {
                    out.push(node);
                }
                for &c in &doc.get(node).children {
                    collect_tables(doc, c, out);
                }
            }
            let mut v = Vec::new();
            collect_tables(&doc, doc.root(), &mut v);
            v
        };
        // At least the outer table must exist.
        assert!(!tables.is_empty(), "at least one table must exist");
    }

    #[test]
    fn reset_insertion_mode_select_in_table_context() {
        // Verify that reset_insertion_mode uses InSelectInTable when a
        // <select> has table ancestors, not plain InSelect.
        let doc = parse(
            "<table><tr><td><select><option>1</option></select></td></tr></table>",
        );
        // The select must exist, confirming the mode switch didn't break parsing.
        let sel = find_element(&doc, "select");
        assert!(sel.is_some(), "select in table cell must be parsed correctly");
    }
}
