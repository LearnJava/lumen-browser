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

use std::collections::HashSet;

use lumen_dom::{
    Attribute, Document, DocumentMode, MetaRefresh, Namespace, NodeData, NodeId, QualName,
    ShadowRootMode, ViewportMeta, ViewportWidth,
};

use crate::foreign_content;
use crate::push_tokenizer::PushTokenizer;
use crate::tokenizer::{Token, Tokenizer};

/// Whether `ns` is a foreign (non-HTML) namespace for the purposes of HTML
/// LS §13.2.6.5 — SVG and MathML share every foreign-content rule this
/// crate implements (breakout list, unconditional self-closing; GAP-XMLDOC
/// срезы 3 и 6, BUG-685).
fn is_foreign_namespace(ns: Namespace) -> bool {
    matches!(ns, Namespace::Svg | Namespace::MathMl)
}

/// Same `encoding` check as [`IncrementalTreeBuilder::has_html_encoding`],
/// against a fragment context element's raw `(name, value)` attribute list
/// instead of a real DOM node's [`lumen_dom::Attribute`]s — GAP-XMLDOC срез
/// 14 (BUG-685) needs both shapes for the same table
/// ([`IncrementalTreeBuilder::resolve_content_namespace`]).
fn attrs_have_html_encoding(attrs: &[(String, String)]) -> bool {
    attrs.iter().any(|(k, v)| {
        k.eq_ignore_ascii_case("encoding")
            && matches!(v.to_ascii_lowercase().as_str(), "text/html" | "application/xhtml+xml")
    })
}

/// Прогоняет `input` через `tokenizer` и `builder`, token за token, отменяя
/// RAWTEXT/RCDATA-переключение токенизатора для `<script>`/`<style>`/
/// `<title>`/`<textarea>`, только что открытых в foreign-неймспейсе
/// (GAP-XMLDOC срез 12, BUG-685): токенизатор сам не знает про namespace
/// (см. `is_raw_text_element`/`is_rcdata_element` в `tokenizer.rs`), поэтому
/// решение — здесь, сразу после того, как `apply_token` создал элемент и
/// его настоящий неймспейс стал известен через `current_namespace()`. Общий
/// для `parse`/`parse_xml_flavoured`/`parse_fragment` — все три раньше молча
/// разошлись бы, реализуй каждая свою копию цикла.
fn run_pull(builder: &mut IncrementalTreeBuilder, input: &str) {
    let mut tokenizer = Tokenizer::new(input);
    tokenizer.set_cdata_allowed(builder.cdata_sections_allowed());
    while let Some(token) = tokenizer.next() {
        let is_open_start_tag = matches!(&token, Token::StartTag { self_closing: false, .. });
        builder.apply_token(token);
        if is_open_start_tag && builder.current_context_forbids_text_only() {
            tokenizer.cancel_text_only();
        }
        // GAP-XMLDOC срез 14 (BUG-685): re-derived after every token, same
        // pattern as the RAWTEXT/RCDATA cancel above — the adjusted current
        // node the CDATA decision depends on changes as elements open/close.
        tokenizer.set_cdata_allowed(builder.cdata_sections_allowed());
    }
}

/// Парсит вход целиком в pull-режиме и возвращает построенный
/// [`Document`]. Эквивалент `IncrementalTreeBuilder::new() + feed(input)
/// + finish()`, но без накладных расходов на push-буферизацию.
pub fn parse(input: &str) -> Document {
    let mut builder = IncrementalTreeBuilder::new();
    run_pull(&mut builder, input);
    builder.finish()
}

/// Same as [`parse`], but for documents the caller has already identified as
/// XML-flavoured (`.xhtml`/`.xht`/`.svg`, `application/xhtml+xml`, …):
/// `<style>`/`<script>` content wrapped in `<![CDATA[ ... ]]>` has that
/// wrapper stripped before it reaches the CSS/JS parser, instead of being
/// left as literal RAWTEXT (BUG-786). GAP-XMLDOC tracks the rest of proper
/// XML document handling (foreign content, self-closing non-void tags, …) —
/// this covers only the CDATA slice, still driven by the HTML5 tree builder.
pub fn parse_xml_flavoured(input: &str) -> Document {
    let mut builder = IncrementalTreeBuilder::new();
    builder.xml_mode = true;
    run_pull(&mut builder, input);
    builder.finish()
}

/// Парсит `input` как **фрагмент** (HTML LS §13.4 «Parsing HTML fragments»)
/// и возвращает временный [`Document`] вместе с корневым `<html>`-узлом
/// фрагмента: дети этого узла — результат разбора (шаг 14 алгоритма).
///
/// Отличается от [`parse`] точкой входа: не `initial`, а сразу `in body`
/// поверх уже открытого синтетического `<html>` (шаги 4 и 6). Именно здесь
/// проходит граница между документом и фрагментом — §13.2.6.4.1–4 обязаны
/// *игнорировать* ведущий whitespace и уносить comment-токены в сам
/// `Document` (мимо `<body>`), а фрагмент обязан сохранить и то и другое
/// (BUG-982: `d.innerHTML=' abc'` терял пробел, `'<!--$-->x'` — комментарий).
///
/// Не реализовано из §13.4: выбор insertion mode по контекстному элементу
/// (шаг 4 всё ещё безусловно «in body» — верно для всех измеренных
/// вызывающих: `innerHTML`/`outerHTML`/`insertAdjacentHTML` никогда не дают
/// контекст вроде `<select>`/`<template>`, требующий другого стартового
/// режима) и form pointer (шаг 7). [`parse_fragment_with_context`] закрывает
/// оставшуюся часть, нужную GAP-XMLDOC (BUG-685): adjusted current node
/// (шаг 3) для CDATA-флага и foreign-content routing, когда контекст —
/// SVG/MathML элемент.
pub fn parse_fragment(input: &str) -> (Document, NodeId) {
    parse_fragment_with_context(input, None)
}

/// Namespace/имя/атрибуты контекстного элемента для HTML LS §13.4 fragment
/// parsing (GAP-XMLDOC срез 14, BUG-685) — то, на что вызывается
/// `Element.innerHTML=`. Используется только чтобы вычислить adjusted
/// current node (§13.2.6.5 "adjusted current node"), пока стек открытых
/// элементов держит один-единственный синтетический `<html>`-корень
/// (`parse_fragment` его всегда создаёт в HTML-неймспейсе независимо от
/// контекста — сам контекстный элемент в дерево фрагмента не попадает,
/// ровно как того требует спека).
pub struct FragmentContext {
    /// Неймспейс контекстного элемента.
    pub namespace: Namespace,
    /// Локальное имя контекстного элемента (lower-case).
    pub local: String,
    /// Атрибуты контекстного элемента — нужны для MathML
    /// `<annotation-xml encoding="text/html">`, единственного integration
    /// point, чья принадлежность зависит не только от имени тега.
    pub attrs: Vec<(String, String)>,
}

/// Same as [`parse_fragment`], but with a real HTML LS §13.4 context
/// element (GAP-XMLDOC срез 14, BUG-685) — `context: None` reproduces
/// `parse_fragment`'s old body-like default exactly.
pub fn parse_fragment_with_context(input: &str, context: Option<FragmentContext>) -> (Document, NodeId) {
    let (mut builder, root) = IncrementalTreeBuilder::new_fragment(context);
    run_pull(&mut builder, input);
    (builder.finish(), root)
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
    /// NodeIds of `<template>` elements that are Declarative Shadow DOM roots
    /// (WHATWG HTML §14.5 — `shadowrootmode` attribute present and valid).
    /// Content is parsed into the shadow root instead of a `DocumentFragment`.
    /// On `</template>`, the template element itself is detached from the DOM.
    declarative_shadow_templates: HashSet<NodeId>,
    /// Построен ли builder алгоритмом §13.4 fragment parsing
    /// ([`new_fragment`][Self::new_fragment]). Меняет ровно два места, где
    /// документный разбор обязан достроить каркас страницы, а фрагментный —
    /// обязан этого не делать: «reset the insertion mode appropriately»
    /// (§13.2.4.1 шаг 4, контекстный элемент вместо корня) и EOF-догон
    /// html/head/body.
    is_fragment: bool,
    /// `true` for [`parse_xml_flavoured`]. Two effects, both scoped to
    /// documents already identified as XML-flavoured — HTML5 semantics for
    /// ordinary documents are untouched:
    /// * strips a `<![CDATA[ ... ]]>` wrapper off `<style>`/`<script>`
    ///   RAWTEXT content on `</style>`/`</script>` (BUG-786) — see
    ///   [`crate::xml_cdata`].
    /// * honours the self-closing flag (`/>`) on non-void elements the way
    ///   XML does, instead of the HTML5 rule that ignores it outside void
    ///   elements — see [`Self::push_open_element`] (GAP-XMLDOC срез 2).
    xml_mode: bool,
    /// HTML LS §13.4 fragment context element (GAP-XMLDOC срез 14,
    /// BUG-685) — `Some` only for [`new_fragment`][Self::new_fragment]
    /// with a real [`FragmentContext`]. Consulted only while
    /// `open_elements` holds exactly the synthetic root (`len() == 1`):
    /// that is precisely the spec's "fragment case" for "adjusted current
    /// node", the one place namespace/CDATA decisions must see the context
    /// element instead of the always-HTML synthetic root.
    fragment_context: Option<FragmentContext>,
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
            declarative_shadow_templates: HashSet::new(),
            is_fragment: false,
            xml_mode: false,
            fragment_context: None,
        }
    }

    /// Builder для §13.4 fragment parsing: синтетический `<html>` уже создан и
    /// открыт (шаг 4), insertion mode — сразу `in body` (шаг 6 для контекста
    /// уровня `<body>`). Возвращает builder и id того самого `<html>`: его
    /// дети и есть разобранный фрагмент (шаг 14).
    ///
    /// Стек open elements непустой намеренно — весь `in body` (scope-запросы,
    /// adoption agency, `append_to_current_open`) написан в расчёте на корень
    /// под ногами; пустой стек дал бы вставку в `#document` и молча иное
    /// поведение у `</div>`, `<body>` и adoption agency. `context` (GAP-XMLDOC
    /// срез 14, BUG-685) не вставляется в дерево — сам `html`-корень всегда
    /// HTML-неймспейса, ровно как требует спека; `context` лишь подменяет
    /// adjusted current node, пока стек держит один этот корень.
    fn new_fragment(context: Option<FragmentContext>) -> (Self, NodeId) {
        let mut builder = Self::new();
        builder.is_fragment = true;
        builder.fragment_context = context;
        let html = builder.create_element_with_attrs("html", &[]);
        let doc_root = builder.doc.root();
        builder.doc.append_child(doc_root, html);
        builder.open_elements.push(html);
        builder.insertion_mode = InsertionMode::InBody;
        (builder, html)
    }

    /// Скармливает chunk push-токенизатору и применяет полученные
    /// токены к DOM. После каждого `feed` `Document` валиден для
    /// чтения.
    pub fn feed(&mut self, chunk: &str) {
        // GAP-XMLDOC срез 12 (BUG-685): `feed_with_context` вместо
        // collect-then-apply — `on_token` применяет каждый токен к дереву и
        // тут же (пока `PushTokenizer`'s внутренний `Tokenizer` ещё жив)
        // решает, годится ли только что выставленный RAWTEXT/RCDATA —
        // `self.tokenizer` временно вынут через `mem::take`, чтобы замыкание
        // могло держать `&mut self` без конфликта заимствований.
        let mut tokenizer = std::mem::take(&mut self.tokenizer);
        tokenizer.feed_with_context(chunk, |token| self.apply_token_for_stream(token));
        self.tokenizer = tokenizer;
    }

    /// Вариант [`feed`][Self::feed] для сырых байт.
    pub fn feed_bytes(&mut self, chunk: &[u8]) {
        let mut tokenizer = std::mem::take(&mut self.tokenizer);
        tokenizer.feed_bytes_with_context(chunk, |token| self.apply_token_for_stream(token));
        self.tokenizer = tokenizer;
    }

    /// `on_token` для [`feed`][Self::feed]/[`feed_bytes`][Self::feed_bytes]/
    /// [`finish`][Self::finish]: применяет токен к дереву, затем сообщает
    /// [`PushTokenizer`] через возврат, форбидит ли *текущий* (только что
    /// открытый этим токеном, если это был `StartTag`) контекст
    /// RAWTEXT/RCDATA — см. [`current_context_forbids_text_only`]
    /// [Self::current_context_forbids_text_only].
    fn apply_token_for_stream(&mut self, token: Token) -> bool {
        self.apply_token(token);
        self.current_context_forbids_text_only()
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
        let mut tokenizer = std::mem::take(&mut self.tokenizer);
        tokenizer.end_with_context(|token| self.apply_token_for_stream(token));
        self.tokenizer = tokenizer;
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
    fn apply_token(&mut self, mut token: Token) {
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
        // GAP-XMLDOC срез 5 (BUG-685, «Третья грань, случай 1»): `html:`/
        // `h:` — the two XHTML-bound namespace prefixes measured in the
        // vendored WPT corpus. Stripped unconditionally under `xml_mode`
        // (not gated on `current_namespace`, since by the time a closing
        // tag like `</html:div>` arrives the element it closes has already
        // moved to the HTML namespace — see `strip_known_html_prefix` doc).
        // `had_html_prefix` remembers the strip happened so
        // `dispatch_foreign_content` can force a breakout even for names
        // (`script`, `link`, ...) absent from the ordinary §13.2.6.5
        // breakout list.
        let mut had_html_prefix = false;
        if self.xml_mode {
            match &mut token {
                Token::StartTag { name, .. } | Token::EndTag { name } => {
                    if let Some(stripped) = foreign_content::strip_known_html_prefix(name) {
                        *name = stripped.to_string();
                        had_html_prefix = true;
                    }
                }
                _ => {}
            }
        }
        // GAP-XMLDOC срез 8 (BUG-685): a start tag routes through the
        // integration-point-aware `start_tag_namespace` (the current node
        // may be an SVG/MathML "integration point" that wants this tag
        // processed as HTML content, not foreign) — an end tag stays on the
        // raw, un-overridden `current_namespace`, since HTML LS §13.2.6.5
        // has no integration-point exception for end tags: closing a tag
        // while inside an integration point still runs the generic
        // by-name stack search in `dispatch_foreign_content`.
        let route_foreign = match &token {
            Token::StartTag { name, .. } => is_foreign_namespace(self.start_tag_namespace(name)),
            Token::EndTag { .. } => is_foreign_namespace(self.current_namespace()),
            _ => false,
        };
        if route_foreign {
            self.dispatch_foreign_content(token, had_html_prefix);
            return;
        }
        self.dispatch(token);
    }

    /// §13.2.6.5 "the rules for parsing tokens in foreign content", reduced
    /// to SVG (GAP-XMLDOC срез 3) and MathML (GAP-XMLDOC срез 6), both
    /// BUG-685 — routed here from [`apply_token`][Self::apply_token]
    /// whenever [`current_namespace`][Self::current_namespace] is foreign.
    /// See `crate::foreign_content` for what this deliberately leaves out
    /// (integration points, foreign-attribute namespacing).
    ///
    /// `forced_breakout` is `true` when [`apply_token`][Self::apply_token]
    /// already stripped an `html:`/`h:` prefix off this token (GAP-XMLDOC
    /// срез 5) — such a tag always breaks out, even for names like
    /// `script`/`link` that are not on the ordinary breakout list.
    fn dispatch_foreign_content(&mut self, token: Token, forced_breakout: bool) {
        match token {
            Token::StartTag {
                ref name,
                ref attrs,
                ..
            } if forced_breakout || foreign_content::breaks_out_of_foreign_content(name, attrs) => {
                // GAP-XMLDOC срез 14 (BUG-685): `real_current_namespace`,
                // not `current_namespace` — this loop pops *real* stack
                // entries, and in the fragment case the synthetic root is
                // never itself foreign even when the context element is
                // (`current_namespace` would report the context's foreign
                // namespace here and pop the root right off the stack).
                while is_foreign_namespace(self.real_current_namespace()) {
                    if let Some(&node) = self.open_elements.last() {
                        self.mark_if_foreign_script_not_executable(node);
                    }
                    self.open_elements.pop();
                }
                self.dispatch(token);
            }
            Token::StartTag {
                name,
                attrs,
                self_closing,
            } => {
                let el = self.create_element_with_attrs(&name, &attrs);
                self.append_to_current_open(el);
                self.push_open_element(el, self_closing);
            }
            Token::EndTag { name } => {
                let lname = name.to_ascii_lowercase();
                // GAP-XMLDOC срез 14 (BUG-685): an end tag whose name is on
                // the same §13.2.6.5 breakout list as the start-tag branch
                // above (`b`, `br`, `p`, `table`, …) exits foreign content
                // exactly like its start-tag counterpart would, REGARDLESS
                // of whether anything on the stack actually matches it —
                // WPT-measured as a bare `</p>`/`</br>` right inside
                // `<svg>`/`<math>` with no open `<p>`/`<br>` to close: real
                // browsers still produce a genuine (self-closing) `<br>`, or
                // an empty `<p>`, as the nearest HTML ancestor's child, via
                // "in body"'s own special-cased end-tag rules for those two
                // names. A name NOT on this list (e.g. a bogus `</g>`,
                // srez 12's regression test) must stay on the plain
                // search-the-stack-or-ignore path below — it must never pop
                // a genuinely still-open element like a foreign `<script>`.
                if foreign_content::breaks_out_of_foreign_content(&lname, &[]) {
                    while is_foreign_namespace(self.real_current_namespace()) {
                        if let Some(&node) = self.open_elements.last() {
                            self.mark_if_foreign_script_not_executable(node);
                        }
                        self.open_elements.pop();
                    }
                    self.dispatch(Token::EndTag { name });
                    return;
                }
                let mut boundary = None;
                for i in (0..self.open_elements.len()).rev() {
                    let node = self.open_elements[i];
                    if self.element_local(node).eq_ignore_ascii_case(&lname) {
                        boundary = Some(i);
                        break;
                    }
                    if self.node_namespace(node) == Namespace::Html {
                        break;
                    }
                }
                if let Some(i) = boundary {
                    // GAP-XMLDOC срез 13 (BUG-685): HTML LS §13.2.6.5 only
                    // treats an SVG/MathML script as executed when the
                    // `</script>` end tag arrives while that script is
                    // still the *current* node (`i` is the top of the
                    // stack) — closing it as a side effect of some
                    // ancestor's end tag (`i` below the top) never sets
                    // the "already started" flag on a real engine, even
                    // though the script's already-parsed text stays in the
                    // tree. `top_is_direct_script_close` is exactly that
                    // one exempted shape; everything else in the truncated
                    // range gets marked.
                    let top_is_direct_script_close =
                        i + 1 == self.open_elements.len() && lname == "script";
                    if !top_is_direct_script_close {
                        let nodes: Vec<NodeId> = self.open_elements[i..].to_vec();
                        for node in nodes {
                            self.mark_if_foreign_script_not_executable(node);
                        }
                    }
                    self.open_elements.truncate(i);
                }
            }
            _ => {}
        }
    }

    /// If `node` is a foreign (SVG/MathML) `<script>` element, record it as
    /// not executable (GAP-XMLDOC срез 13, BUG-685) — see
    /// [`Document::mark_foreign_script_not_executable`].
    fn mark_if_foreign_script_not_executable(&mut self, node: NodeId) {
        if is_foreign_namespace(self.node_namespace(node))
            && self.element_local(node).eq_ignore_ascii_case("script")
        {
            self.doc.mark_foreign_script_not_executable(node);
        }
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
                if name == "meta" && let Some(meta) = parse_viewport_meta(attrs) {
                    self.doc.set_viewport_meta(meta);
                }
                if name == "meta" && let Some(refresh) = parse_meta_refresh(attrs) {
                    self.doc.set_meta_refresh(refresh);
                }
            }
            Token::StartTag {
                ref name,
                ref attrs,
                self_closing,
            } if name == "title" => {
                let el = self.create_element_with_attrs(name, attrs);
                self.append_to_current_open(el);
                if !(self.xml_mode && self_closing) {
                    self.open_elements.push(el);
                    self.original_insertion_mode = Some(self.insertion_mode);
                    self.insertion_mode = InsertionMode::Text;
                }
            }
            Token::StartTag {
                ref name,
                ref attrs,
                self_closing,
            } if matches!(name.as_str(), "noframes" | "style" | "script") =>
            {
                let el = self.create_element_with_attrs(name, attrs);
                self.append_to_current_open(el);
                if !(self.xml_mode && self_closing) {
                    self.open_elements.push(el);
                    self.original_insertion_mode = Some(self.insertion_mode);
                    self.insertion_mode = InsertionMode::Text;
                }
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
                // WHATWG HTML §14.5 — Declarative Shadow DOM.
                // If `shadowrootmode="open"` or `"closed"` is present and the current
                // node can host a shadow root, redirect content into the shadow root.
                let shadow_mode = attrs.iter().find_map(|(name, value)| {
                    if name.eq_ignore_ascii_case("shadowrootmode") {
                        match value.as_str() {
                            "open" => Some(ShadowRootMode::Open),
                            "closed" => Some(ShadowRootMode::Closed),
                            _ => None,
                        }
                    } else {
                        None
                    }
                });

                let el = self.create_element_with_attrs(name, attrs);
                self.append_to_current_open(el);
                self.open_elements.push(el);
                self.active_formatting.push(ActiveFormattingEntry::Marker);
                self.template_mode_stack.push(InsertionMode::InBody);
                self.insertion_mode = InsertionMode::InTemplate;

                if let Some(mode) = shadow_mode {
                    // The shadow host is the parent of the <template> element.
                    // It is the element at open_elements[-2] (we just pushed el at [-1]).
                    let host = self.open_elements
                        .len()
                        .checked_sub(2)
                        .and_then(|i| self.open_elements.get(i))
                        .copied();
                    if let Some(host) = host {
                        let shadow_root = self.doc.attach_shadow(host, mode);
                        // Use the shadow root as the template's "content" so that
                        // current_insertion_parent() redirects content there.
                        self.doc.set_template_content(el, shadow_root);
                        self.declarative_shadow_templates.insert(el);
                    } else {
                        // Fallback: no valid host — treat as regular template.
                        let frag = self.doc.create_fragment();
                        self.doc.set_template_content(el, frag);
                    }
                } else {
                    // Regular <template>: create a DocumentFragment as content.
                    let frag = self.doc.create_fragment();
                    self.doc.set_template_content(el, frag);
                }
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
                // §13.2.6.4.7 "in body": "process the token using the
                // rules for the 'in head' insertion mode" is a plain
                // delegation, not a mode switch — call `mode_in_head`
                // directly instead of mutating `self.insertion_mode` first.
                // `script`/`style`/`noframes`/`title` capture
                // `self.insertion_mode` as `original_insertion_mode` to
                // restore once their RAWTEXT body's closing tag arrives
                // (§13.2.6.2 "generic raw text element parsing algorithm");
                // if this call first forced `insertion_mode` to `InHead`,
                // that capture recorded `InHead` instead of the real
                // current mode (`InBody`), and restoring it later routed
                // the tag *after* the script/style through InHead's
                // "anything else" fallback — which treats stray content as
                // still being in `<head>` and reopens a second `<body>`
                // (GAP-XMLDOC срез 5 test regression, BUG-685).
                self.mode_in_head(token);
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
            // §13.2.6.4.7 «in body» — start tag `head`: parse error, ignore.
            // Document parsing reached this arm only for a *stray* second
            // `<head>` and made a bogus element out of it; fragment parsing
            // (BUG-982) reaches it for the very first one, where dropping the
            // tag and keeping its content is what every browser does. The rest
            // of the spec's ignore list (`caption`/`col`/`td`/`tr`/…) stays
            // unimplemented on purpose — those already build elements here and
            // in `innerHTML`, and changing that is not this fix.
            Token::StartTag { ref name, .. } if name == "head" => {}
            Token::EndTag { ref name } if name == "body" => {
                self.insertion_mode = InsertionMode::AfterBody;
            }
            Token::EndTag { ref name } if name == "html" => {
                self.insertion_mode = InsertionMode::AfterBody;
                self.dispatch(Token::EndTag { name: name.clone() });
            }
            // Block-уровневые элементы: закрывают <p> в button scope.
            Token::StartTag {
                ref name,
                ref attrs,
                self_closing,
            } if is_block_element(name) => {
                if self.has_element_in_button_scope("p") {
                    self.close_p_element();
                }
                let el = self.create_element_with_attrs(name, attrs);
                self.append_to_current_open(el);
                self.push_open_element(el, self_closing);
            }
            // <h1>..<h6>: закрывают <p> в button scope, а также
            // предыдущий heading в стеке.
            Token::StartTag {
                ref name,
                ref attrs,
                self_closing,
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
                self.push_open_element(el, self_closing);
            }
            // <li>: имплисит-закрытие предыдущего <li>.
            Token::StartTag {
                ref name,
                ref attrs,
                self_closing,
            } if name == "li" => {
                self.close_list_item_like(&["li"]);
                if self.has_element_in_button_scope("p") {
                    self.close_p_element();
                }
                let el = self.create_element_with_attrs(name, attrs);
                self.append_to_current_open(el);
                self.push_open_element(el, self_closing);
            }
            // <dt>/<dd>: closing previous <dt>/<dd>.
            Token::StartTag {
                ref name,
                ref attrs,
                self_closing,
            } if matches!(name.as_str(), "dt" | "dd") => {
                self.close_list_item_like(&["dt", "dd"]);
                if self.has_element_in_button_scope("p") {
                    self.close_p_element();
                }
                let el = self.create_element_with_attrs(name, attrs);
                self.append_to_current_open(el);
                self.push_open_element(el, self_closing);
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
                ref name,
                ref attrs,
                self_closing,
            } if name == "textarea" => {
                let el = self.create_element_with_attrs(name, attrs);
                self.append_to_current_open(el);
                if !(self.xml_mode && self_closing) {
                    self.open_elements.push(el);
                    self.original_insertion_mode = Some(self.insertion_mode);
                    self.insertion_mode = InsertionMode::Text;
                }
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
                self.reconstruct_active_formatting();
                let el = self.create_element_with_attrs(name, attrs);
                self.append_to_current_open(el);
                self.push_open_element(el, self_closing);
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
                if let Some(el) = self.open_elements.pop()
                    && self.xml_mode
                {
                    self.strip_cdata_wrapper_from(el);
                }
                if let Some(prev) = self.original_insertion_mode.take() {
                    self.insertion_mode = prev;
                } else {
                    self.insertion_mode = InsertionMode::InBody;
                }
            }
            _ => { /* EOF / etc. — parse error, ignore */ }
        }
    }

    /// [`xml_mode`][Self::xml_mode] support: `el` just closed
    /// (its RAWTEXT content is complete) — if that content is a single text
    /// child wrapped in `<![CDATA[ ... ]]>`, rewrite it to the unwrapped
    /// inner text (BUG-786). RAWTEXT elements coalesce into one text node
    /// via [`Self::insert_text`], so «single child» covers every case the
    /// corpus this targets actually produces.
    fn strip_cdata_wrapper_from(&mut self, el: NodeId) {
        let Some(&child) = self.doc.get(el).children.first() else {
            return;
        };
        if self.doc.get(el).children.len() != 1 {
            return;
        }
        if let NodeData::Text(s) = &self.doc.get(child).data {
            let stripped = crate::xml_cdata::strip_cdata_wrapper(s);
            if stripped.len() != s.len() {
                let owned = stripped.to_string();
                if let NodeData::Text(s) = &mut self.doc.get_mut(child).data {
                    *s = owned;
                }
            }
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
            if is_end_template {
                // `process_template_end_tag` has already reset the insertion mode
                // appropriately (§13.2.4.1). The restore below assumes «InHead means
                // InHead did not transition us», which is false here: after a
                // `<template>` closed while still in head, InHead *is* the answer
                // (BUG-417) — forcing InTemplate back sent everything after
                // `</template>` into the head element.
                return;
            }
            // A rawtext head element (<style>/<script>/<title>) switched us to Text
            // mode and captured `original_insertion_mode = InHead` (the mode we set
            // above). Correct it to InTemplate so the element's end tag returns us to
            // template-content mode — otherwise tokens after it (e.g. a `<slot>` in a
            // declarative shadow `<template>`) are processed in InHead and never land
            // in the template fragment / shadow root (BUG-142).
            if self.insertion_mode == InsertionMode::Text
                && self.original_insertion_mode == Some(InsertionMode::InHead)
            {
                self.original_insertion_mode = Some(InsertionMode::InTemplate);
            } else if self.insertion_mode == InsertionMode::InHead {
                // Non-rawtext head tag (<meta>/<link>/<base>): InHead didn't
                // transition us, so restore InTemplate.
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

        let template_node = self.open_elements[pos];

        // Generate implied end tags (not excluding template).
        self.generate_implied_end_tags(None);

        // Pop down to and including the template element.
        self.open_elements.truncate(pos);

        // Clear active formatting list up to the last marker.
        self.clear_active_formatting_to_marker();

        // Pop the template content mode.
        self.template_mode_stack.pop();

        // §13.2.6.4.4 «In head», `</template>`: reset the insertion mode
        // appropriately. Jumping straight to InBody (as this did before BUG-417)
        // loses the «after head» transition when the template was seen while still
        // in head — the mode says «in body» while no `<body>` exists, so everything
        // after `</template>` lands in `<head>`/`<html>` and `document.body` stays
        // null forever.
        self.reset_insertion_mode();

        // WHATWG HTML §14.5 — Declarative Shadow DOM cleanup.
        // The <template shadowrootmode="..."> element is a syntactic marker only;
        // its content was parsed directly into the shadow root. Detach it so it
        // is invisible to the final DOM (the shadow root remains attached to the host).
        if self.declarative_shadow_templates.remove(&template_node) {
            self.doc.detach(template_node);
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
        // §13.4 fragment parsing has no page skeleton to complete: the root
        // `<html>` is synthetic and `<head>`/`<body>` are not part of the
        // fragment. Without this guard `'<table>…</table>'` ends in a mode that
        // walks the chain below and appends a stray `<head>`/`<body>` pair to
        // the fragment's children.
        if self.is_fragment {
            return;
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
        let qname = self.resolve_element_name(name);
        let namespace = qname.namespace;
        let id = self.doc.create_element(qname);
        if let NodeData::Element {
            attrs: dom_attrs, ..
        } = &mut self.doc.get_mut(id).data
        {
            for (k, v) in attrs {
                // §13.2.6.5 "adjust foreign attributes" runs for both foreign
                // namespaces before the per-namespace case-restoration table
                // below — `xlink:href` on a MathML element is just as much a
                // foreign attribute as on an SVG one (GAP-XMLDOC срез 10).
                let foreign_ns = if matches!(namespace, Namespace::Svg | Namespace::MathMl) {
                    foreign_content::adjust_foreign_attribute(k)
                } else {
                    None
                };
                let attr_name = match foreign_ns {
                    Some(ns) => QualName {
                        namespace: ns,
                        local: k.clone(),
                    },
                    None => {
                        let local = match namespace {
                            Namespace::Svg => {
                                foreign_content::adjust_svg_attribute_name(k).to_string()
                            }
                            Namespace::MathMl => {
                                foreign_content::adjust_mathml_attribute_name(k).to_string()
                            }
                            _ => k.clone(),
                        };
                        QualName::html(local)
                    }
                };
                dom_attrs.push(Attribute {
                    name: attr_name,
                    value: v.clone(),
                });
            }
        }
        id
    }

    /// Namespace-aware qualified name for a newly created element — HTML by
    /// default, SVG/MathML while [`start_tag_namespace`][Self::start_tag_namespace]
    /// says so (already that namespace, the tag being opened is `<svg>`/
    /// `<math>` itself, or neither and an integration point overrides back
    /// to HTML) (HTML LS §13.2.6.5 "insert a foreign element", GAP-XMLDOC
    /// срезы 3, 6 и 8, BUG-685).
    fn resolve_element_name(&self, name: &str) -> QualName {
        match self.start_tag_namespace(name) {
            Namespace::Svg => QualName {
                namespace: Namespace::Svg,
                local: foreign_content::adjust_svg_tag_name(name).to_string(),
            },
            Namespace::MathMl => QualName {
                namespace: Namespace::MathMl,
                local: name.to_string(),
            },
            _ if name == "svg" => QualName {
                namespace: Namespace::Svg,
                local: "svg".to_string(),
            },
            _ if name == "math" => QualName {
                namespace: Namespace::MathMl,
                local: "math".to_string(),
            },
            _ => QualName::html(name),
        }
    }

    /// Namespace of the current node (`open_elements` top), or `Html` for
    /// an empty stack (document root). Overridden by the fragment context
    /// element while the stack holds only the synthetic root — HTML LS
    /// §13.2.6.5 "adjusted current node" for the fragment case (GAP-XMLDOC
    /// срез 14, BUG-685); see [`fragment_context`][Self::fragment_context].
    fn current_namespace(&self) -> Namespace {
        if self.open_elements.len() == 1
            && let Some(ctx) = &self.fragment_context
        {
            return ctx.namespace;
        }
        self.real_current_namespace()
    }

    /// Namespace of the REAL current node (`open_elements` top, never the
    /// fragment-context override) — see the warning on
    /// [`current_namespace`][Self::current_namespace]. Needed anywhere that
    /// actually walks or mutates the open-elements stack, since the context
    /// element never sits on it for real.
    fn real_current_namespace(&self) -> Namespace {
        self.open_elements
            .last()
            .map(|&id| self.node_namespace(id))
            .unwrap_or(Namespace::Html)
    }

    /// GAP-XMLDOC срез 12 (BUG-685): true right after a `<script>`/`<style>`/
    /// `<title>`/`<textarea>` start tag opened an element whose *resolved*
    /// namespace ([`current_namespace`][Self::current_namespace], which
    /// already accounts for `html:`/`h:` breakout and integration-point
    /// overrides) is foreign. HTML LS §13.2.6.5's "generic raw text/RCDATA
    /// element parsing algorithm" is invoked only by the HTML insertion-mode
    /// rules ("in head"/"in body"), never by "the rules for parsing tokens
    /// in foreign content" — so a genuinely-foreign `<script>`/`<title>`
    /// must not leave the tokenizer in RAWTEXT/RCDATA state, even though the
    /// tokenizer's own [`is_raw_text_element`][crate::tokenizer]-style check
    /// (namespace-blind, keyed on bare tag name) already switched it there.
    fn current_context_forbids_text_only(&self) -> bool {
        is_foreign_namespace(self.current_namespace())
    }

    /// Namespace a start tag named `name` should be processed under, given
    /// the current open-elements stack — [`current_namespace`]
    /// [Self::current_namespace], except for two HTML LS §13.2.6.5
    /// "integration point" overrides measured for GAP-XMLDOC срез 8
    /// (BUG-685):
    ///
    /// * the current node is an SVG/MathML "integration point"
    ///   (`is_integration_point_host`) — new elements default to HTML
    ///   instead of inheriting the foreign namespace, since markup nested
    ///   there (`<foreignObject><p>...`, `<annotation-xml
    ///   encoding="text/html"><div>...`, `<mtext><b>...`) is genuine HTML
    ///   content per spec, not merely SVG/MathML that happens to render
    ///   like it;
    /// * `<svg>` as a direct child of MathML `annotation-xml` always
    ///   becomes an SVG element regardless of `encoding` — the one
    ///   exception that goes the other way.
    fn start_tag_namespace(&self, name: &str) -> Namespace {
        if self.open_elements.len() == 1
            && let Some(ctx) = &self.fragment_context
        {
            let has_html_encoding = attrs_have_html_encoding(&ctx.attrs);
            return Self::resolve_content_namespace(ctx.namespace, &ctx.local, has_html_encoding, name);
        }
        let Some(&top) = self.open_elements.last() else {
            return Namespace::Html;
        };
        let top_ns = self.node_namespace(top);
        Self::resolve_content_namespace(top_ns, self.element_local(top), self.has_html_encoding(top), name)
    }

    /// HTML LS §13.2.6.5 namespace/integration-point table shared by
    /// [`start_tag_namespace`][Self::start_tag_namespace] and
    /// [`cdata_sections_allowed`][Self::cdata_sections_allowed] —
    /// parameterized over the adjusted current node's identity
    /// (`node_ns`/`node_local`/`node_has_html_encoding`) instead of a
    /// concrete [`NodeId`], since the fragment-case override (GAP-XMLDOC
    /// срез 14, BUG-685) has no real node to look up: the context element
    /// never enters the tree.
    ///
    /// `tag_name` only matters for the MathML text integration points'
    /// `mglyph`/`malignmark` exception (those children stay MathML); pass
    /// `""` (never a real tag name) when the caller isn't routing a
    /// specific start tag.
    fn resolve_content_namespace(
        node_ns: Namespace,
        node_local: &str,
        node_has_html_encoding: bool,
        tag_name: &str,
    ) -> Namespace {
        if node_ns == Namespace::MathMl && node_local == "annotation-xml" && tag_name == "svg" {
            return Namespace::Svg;
        }
        let is_integration_point = match node_ns {
            Namespace::Svg => foreign_content::is_svg_html_integration_point(node_local),
            Namespace::MathMl => {
                (node_local == "annotation-xml" && node_has_html_encoding)
                    || (foreign_content::is_mathml_text_integration_point(node_local)
                        && !matches!(tag_name, "mglyph" | "malignmark"))
            }
            _ => false,
        };
        if is_integration_point { Namespace::Html } else { node_ns }
    }

    /// Whether `<![CDATA[` right now would start a real CDATA section
    /// (GAP-XMLDOC срез 14, BUG-685) — true iff the adjusted current node
    /// (same fragment-case override as [`current_namespace`]
    /// [Self::current_namespace]) is foreign (SVG/MathML) and not an
    /// integration point. Uses [`resolve_content_namespace`]
    /// [Self::resolve_content_namespace] with `tag_name = ""` — CDATA isn't
    /// a start tag, so the MathML `mglyph`/`malignmark` exception (which
    /// only ever concerns an incoming tag name) never applies here.
    fn cdata_sections_allowed(&self) -> bool {
        let content_ns = if self.open_elements.len() == 1 {
            match &self.fragment_context {
                Some(ctx) => {
                    let has_html_encoding = attrs_have_html_encoding(&ctx.attrs);
                    Self::resolve_content_namespace(ctx.namespace, &ctx.local, has_html_encoding, "")
                }
                None => Namespace::Html,
            }
        } else {
            match self.open_elements.last() {
                Some(&top) => Self::resolve_content_namespace(
                    self.node_namespace(top),
                    self.element_local(top),
                    self.has_html_encoding(top),
                    "",
                ),
                None => Namespace::Html,
            }
        };
        is_foreign_namespace(content_ns)
    }

    /// Whether `node` (a MathML `annotation-xml` element) carries an
    /// `encoding` attribute of `text/html` or `application/xhtml+xml`
    /// (case-insensitive per HTML LS §13.2.6.5) — the condition that makes
    /// it an HTML integration point.
    fn has_html_encoding(&self, node: NodeId) -> bool {
        match &self.doc.get(node).data {
            NodeData::Element { attrs, .. } => attrs.iter().any(|a| {
                a.name.local.eq_ignore_ascii_case("encoding")
                    && matches!(
                        a.value.to_ascii_lowercase().as_str(),
                        "text/html" | "application/xhtml+xml"
                    )
            }),
            _ => false,
        }
    }

    /// Namespace stored on `id`'s `QualName` — `Html` if `id` isn't an
    /// element (shouldn't happen for a stack entry, but this stays a total
    /// function rather than one more `unwrap()`).
    fn node_namespace(&self, id: NodeId) -> Namespace {
        match &self.doc.get(id).data {
            NodeData::Element { name, .. } => name.namespace,
            _ => Namespace::Html,
        }
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

    /// Pushes a just-created non-void element onto the open-elements stack,
    /// then immediately pops it back off if the start tag was self-closing
    /// and either [`xml_mode`][Self::xml_mode] is on (GAP-XMLDOC срез 2) or
    /// `el` is a foreign (SVG/MathML) element (GAP-XMLDOC срезы 3 и 6,
    /// BUG-685).
    ///
    /// HTML5 (§13.2.5.32 "before attribute value state" note) defines the
    /// self-closing flag but the tree builder ignores it outside void/foreign
    /// elements — real markup does this too (`<br/>text` closes `br`, a void
    /// element, either way) and Lumen follows that for ordinary HTML
    /// elements. Two exceptions honour the flag instead:
    /// * XML-flavoured documents (`.xhtml`/`.xht`/`.svg`) — `<div class="a"/>`
    ///   is XML well-formed and self-closes. Without this, a self-closing
    ///   non-void tag opens an element it never closes, so N sibling
    ///   self-closing tags become N nested elements — measured on the
    ///   corpus (BUG-786 "Вторая грань") to turn `flex-direction: column`
    ///   layouts with O(2^depth) cost into TIMEOUTs.
    /// * Foreign content (§13.2.6.5 step 4) honours the self-closing flag
    ///   unconditionally, in *any* document — `<rect/>`/`<circle/>` inside an
    ///   inline `<svg>` are the common case and are never void elements per
    ///   HTML's fixed list, so without this every sibling shape nests inside
    ///   the previous one the same way BUG-786 did for XML documents.
    fn push_open_element(&mut self, el: NodeId, self_closing: bool) {
        self.open_elements.push(el);
        let is_foreign = is_foreign_namespace(self.node_namespace(el));
        if self_closing && (self.xml_mode || is_foreign) {
            self.open_elements.pop();
        }
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
            // §13.2.4.1 step 3: `last` is true for the first node of the stack.
            let last = i == 0;
            // §13.2.4.1 step 4: in fragment parsing the last node is replaced by
            // the *context element*. `new_fragment` fixes that context at body
            // level (§13.4 step 6 with a real context element is BUG-685), so
            // the answer is `in body` — and, crucially, not the `"html"` arm
            // below: with `head_element == None` that arm returns BeforeHead,
            // and the EOF walk would then materialise a `<head>`/`<body>` pair
            // inside the fragment (visible on `'<table>…</table>'`).
            if last && self.is_fragment {
                self.insertion_mode = InsertionMode::InBody;
                return;
            }
            let local = self.element_local(node);
            let mode = match local {
                "select" => {
                    // §13.2.4.1 steps 4.1–4.8: walk ancestors outwards and stop at
                    // the first one that decides the mode — a `<template>` means
                    // plain InSelect, a `<table>` means InSelectInTable.
                    let mut select_mode = InsertionMode::InSelect;
                    if !last {
                        for &ancestor in self.open_elements[..i].iter().rev() {
                            match self.element_local(ancestor) {
                                "template" => break,
                                "table" => {
                                    select_mode = InsertionMode::InSelectInTable;
                                    break;
                                }
                                _ => {}
                            }
                        }
                    }
                    select_mode
                }
                "td" | "th" if !last => InsertionMode::InCell,
                "tr" => InsertionMode::InRow,
                "tbody" | "thead" | "tfoot" => InsertionMode::InTableBody,
                "caption" => InsertionMode::InCaption,
                "colgroup" => InsertionMode::InColumnGroup,
                "table" => InsertionMode::InTable,
                // §13.2.4.1 step 11: inside a `<template>` the mode is the current
                // template insertion mode. `template_mode_stack` holds the *content*
                // mode here (InBody), so InTemplate is what dispatches through it.
                "template" => InsertionMode::InTemplate,
                // §13.2.4.1 step 12: `<head>` still open means we are back in head —
                // the next flow content then closes head and creates `<body>`.
                "head" if !last => InsertionMode::InHead,
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
    #[allow(clippy::expect_used)]  // унаследовано, docs/lint-policy.md §10
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
    #[allow(clippy::expect_used)]  // унаследовано, docs/lint-policy.md §10
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

/// Parse `<meta name="viewport" content="…">` attributes into a `ViewportMeta`.
///
/// Returns `None` if the tag is not a viewport meta (missing `name=viewport`
/// or missing `content`). Silently ignores unrecognised directives.
fn parse_viewport_meta(attrs: &[(String, String)]) -> Option<ViewportMeta> {
    let name_val = attrs.iter().find(|(k, _)| k.eq_ignore_ascii_case("name"))?.1.as_str();
    if !name_val.eq_ignore_ascii_case("viewport") {
        return None;
    }
    let content = &attrs.iter().find(|(k, _)| k.eq_ignore_ascii_case("content"))?.1;
    let mut meta = ViewportMeta::default();
    for part in content.split(',') {
        let part = part.trim();
        let mut kv = part.splitn(2, '=');
        let key = kv.next()?.trim();
        let val = kv.next()?.trim();
        match key.to_ascii_lowercase().as_str() {
            "width" => {
                meta.width = Some(if val.eq_ignore_ascii_case("device-width") {
                    ViewportWidth::DeviceWidth
                } else {
                    ViewportWidth::Pixels(val.parse::<f32>().unwrap_or(980.0))
                });
            }
            "initial-scale" => {
                if let Ok(v) = val.parse::<f32>() && v > 0.0 {
                    meta.initial_scale = v;
                }
            }
            _ => {}
        }
    }
    Some(meta)
}

/// Parse `<meta http-equiv="refresh" content="…">` attributes into a
/// [`MetaRefresh`] (BUG-566, HTML LS §4.2.5.3 "shared declarative refresh
/// steps").
///
/// Returns `None` if the tag is not a refresh meta (`http-equiv` mismatch,
/// missing `content`, or a `content` value with no parseable leading time).
fn parse_meta_refresh(attrs: &[(String, String)]) -> Option<MetaRefresh> {
    let http_equiv = attrs.iter().find(|(k, _)| k.eq_ignore_ascii_case("http-equiv"))?.1.as_str();
    if !http_equiv.eq_ignore_ascii_case("refresh") {
        return None;
    }
    let content = &attrs.iter().find(|(k, _)| k.eq_ignore_ascii_case("content"))?.1;
    let (_, s) = split_leading_ws(content);
    let digits_end = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    let (digits, mut rest) = s.split_at(digits_end);
    if digits.is_empty() && !rest.starts_with('.') {
        return None;
    }
    let delay_seconds: u64 = digits.parse().unwrap_or(0);
    // Discard the fractional part (spec keeps only whole seconds).
    if let Some(after_dot) = rest.strip_prefix('.') {
        let frac_end = after_dot.find(|c: char| !c.is_ascii_digit()).unwrap_or(after_dot.len());
        rest = &after_dot[frac_end..];
    }
    let (_, rest) = split_leading_ws(rest);
    let rest = rest.strip_prefix(';').or_else(|| rest.strip_prefix(',')).unwrap_or(rest);
    let (_, rest) = split_leading_ws(rest);
    if rest.is_empty() {
        return Some(MetaRefresh { delay_seconds, url: None });
    }
    let rest = if rest.len() >= 3 && rest[..3].eq_ignore_ascii_case("url") {
        let (_, after_url) = split_leading_ws(&rest[3..]);
        let after_eq = after_url.strip_prefix('=').unwrap_or(after_url);
        split_leading_ws(after_eq).1
    } else {
        rest
    };
    let url = match rest.chars().next() {
        Some(q @ ('\'' | '"')) => {
            let body = &rest[1..];
            body.find(q).map_or(body, |end| &body[..end])
        }
        _ => rest,
    };
    Some(MetaRefresh { delay_seconds, url: Some(url.trim_end().to_string()) })
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
        // BUG-417: the template must not cost us the <body> — the assertions above
        // only describe the template itself, which stayed correct all along.
        let _ = body_of(&doc);
    }

    /// BUG-417: a `<template>` seen while the parser is still «in head» must not
    /// swallow `<body>`. `</template>` resets the insertion mode appropriately
    /// (§13.2.4.1) — back to InHead — so the next flow content closes head and
    /// creates the body; jumping straight to InBody skipped that transition and
    /// left `document.body` null with the rest of the page inside `<head>`.
    #[test]
    fn template_before_body_still_creates_body() {
        let doc = parse(
            "<!DOCTYPE html><template id=t><div>inside</div></template><p id=b>AFTER-TEXT</p>",
        );
        // <body> exists at all (body_of panics otherwise)…
        let body = body_of(&doc);
        // …and everything after </template> lands in it, not in <head>.
        let p = find_element(&doc, "p").expect("<p> after </template> must be parsed");
        assert!(
            doc.get(body).children.contains(&p),
            "<p> after </template> must be a child of <body>, not <head>"
        );
        // The template itself is unharmed: still in <head>, content holds the DIV.
        let head = head_of(&doc);
        let tmpl = *doc
            .get(head)
            .children
            .iter()
            .find(|&&c| {
                matches!(&doc.get(c).data, NodeData::Element { name, .. } if name.local == "template")
            })
            .expect("<template> must stay in <head>");
        let frag = doc.template_content(tmpl).expect("no content fragment");
        assert_eq!(
            doc.get(frag).children.len(),
            1,
            "template content must still hold exactly the <div>"
        );
    }

    /// BUG-417, second case of the report: explicit `<head>`/`<body>` around the
    /// template. Here the page rendered, but `document.body` was still null and the
    /// content landed under `<html>`.
    #[test]
    fn template_in_explicit_head_keeps_body() {
        let doc = parse(
            "<html><head><template><style>i{}</style></template></head><body><p>x</p></body></html>",
        );
        let body = body_of(&doc);
        let p = find_element(&doc, "p").expect("<p> must be parsed");
        assert!(
            doc.get(body).children.contains(&p),
            "<p> must be a child of <body>, not of <html>"
        );
    }

    /// Regression guard for the nested case the old two-way branch handled: after
    /// the inner `</template>` the parser must go back to the *outer* template's
    /// content, not to `<body>`.
    #[test]
    fn nested_template_end_returns_to_outer_template() {
        let doc = parse(
            "<body><template id=outer><template id=inner></template><p>after</p></template></body>",
        );
        let outer = find_template(&doc).expect("outer template not found");
        assert_eq!(doc.get(outer).get_attr("id"), Some("outer"));
        let frag = doc.template_content(outer).expect("no content fragment");
        let names: Vec<&str> = doc
            .get(frag)
            .children
            .iter()
            .map(|&c| match &doc.get(c).data {
                NodeData::Element { name, .. } => name.local.as_str(),
                _ => "",
            })
            .collect();
        assert!(
            names.contains(&"p"),
            "content after the inner </template> must stay in the outer template, got {names:?}"
        );
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

    #[test]
    fn viewport_meta_device_width_scale() {
        let doc = parse(
            r#"<html><head><meta name="viewport" content="width=device-width, initial-scale=1.0"></head><body></body></html>"#,
        );
        let vm = doc.viewport_meta().expect("viewport meta must be set");
        assert!((vm.initial_scale - 1.0).abs() < 0.001);
        assert_eq!(vm.width, Some(ViewportWidth::DeviceWidth));
    }

    #[test]
    fn viewport_meta_custom_scale() {
        let doc = parse(
            r#"<html><head><meta name="viewport" content="width=device-width, initial-scale=2.0"></head><body></body></html>"#,
        );
        let vm = doc.viewport_meta().expect("viewport meta must be set");
        assert!((vm.initial_scale - 2.0).abs() < 0.001);
    }

    #[test]
    fn viewport_meta_fixed_width() {
        let doc = parse(
            r#"<html><head><meta name="viewport" content="width=375, initial-scale=1"></head><body></body></html>"#,
        );
        let vm = doc.viewport_meta().expect("viewport meta must be set");
        assert_eq!(vm.width, Some(ViewportWidth::Pixels(375.0)));
        assert!((vm.initial_scale - 1.0).abs() < 0.001);
    }

    #[test]
    fn viewport_meta_absent_returns_none() {
        let doc = parse("<html><head></head><body></body></html>");
        assert!(doc.viewport_meta().is_none(), "no meta → no viewport_meta");
    }

    #[test]
    fn viewport_meta_non_viewport_meta_ignored() {
        let doc = parse(
            r#"<html><head><meta name="description" content="hello"></head><body></body></html>"#,
        );
        assert!(doc.viewport_meta().is_none(), "description meta must not set viewport_meta");
    }

    // ─── `<meta http-equiv="refresh">` (BUG-566) ───────────────────────────────

    #[test]
    fn meta_refresh_with_url() {
        let doc = parse(
            r#"<html><head><meta http-equiv="refresh" content="5; url=https://example.com/next"></head><body></body></html>"#,
        );
        let mr = doc.meta_refresh().expect("refresh meta must be set");
        assert_eq!(mr.delay_seconds, 5);
        assert_eq!(mr.url.as_deref(), Some("https://example.com/next"));
    }

    #[test]
    fn meta_refresh_no_url_reloads_self() {
        let doc = parse(r#"<html><head><meta http-equiv="refresh" content="3"></head><body></body></html>"#);
        let mr = doc.meta_refresh().expect("refresh meta must be set");
        assert_eq!(mr.delay_seconds, 3);
        assert_eq!(mr.url, None);
    }

    #[test]
    fn meta_refresh_quoted_url() {
        let doc = parse(
            r#"<html><head><meta http-equiv="refresh" content='0;url="https://example.com/a b"'></head><body></body></html>"#,
        );
        let mr = doc.meta_refresh().expect("refresh meta must be set");
        assert_eq!(mr.url.as_deref(), Some("https://example.com/a b"));
    }

    #[test]
    fn meta_refresh_comma_separator_and_no_url_equals() {
        let doc = parse(
            r#"<html><head><meta http-equiv="refresh" content="2, https://example.com/c"></head><body></body></html>"#,
        );
        let mr = doc.meta_refresh().expect("refresh meta must be set");
        assert_eq!(mr.delay_seconds, 2);
        assert_eq!(mr.url.as_deref(), Some("https://example.com/c"));
    }

    #[test]
    fn meta_refresh_wrong_http_equiv_ignored() {
        let doc = parse(r#"<html><head><meta http-equiv="content-type" content="5"></head><body></body></html>"#);
        assert!(doc.meta_refresh().is_none());
    }

    #[test]
    fn meta_refresh_no_leading_digits_ignored() {
        let doc = parse(
            r#"<html><head><meta http-equiv="refresh" content="url=https://example.com"></head><body></body></html>"#,
        );
        assert!(doc.meta_refresh().is_none(), "no leading time -> invalid, ignored");
    }

    #[test]
    fn meta_refresh_first_occurrence_wins() {
        let doc = parse(
            r#"<html><head><meta http-equiv="refresh" content="1;url=https://first.example"><meta http-equiv="refresh" content="2;url=https://second.example"></head><body></body></html>"#,
        );
        let mr = doc.meta_refresh().expect("refresh meta must be set");
        assert_eq!(mr.delay_seconds, 1);
        assert_eq!(mr.url.as_deref(), Some("https://first.example"));
    }

    // ─── Declarative Shadow DOM (WHATWG HTML §14.5) ───────────────────────────

    #[test]
    fn declarative_shadow_dom_open_mode_creates_shadow_root() {
        use lumen_dom::ShadowRootMode;
        let doc = parse(r#"<div id="host"><template shadowrootmode="open"><p>shadow</p></template></div>"#);
        // Find the host <div>
        let body = doc.body().expect("body exists");
        let host = doc.get(body).children.first().copied().expect("host div");
        let sr = doc.shadow_root_of(host);
        assert!(sr.is_some(), "shadow root must be attached to host");
        // The shadow root should be open
        if let Some(sr_id) = sr {
            let node = doc.get(sr_id);
            if let lumen_dom::NodeData::ShadowRoot { mode } = &node.data {
                assert_eq!(*mode, ShadowRootMode::Open);
            } else {
                panic!("expected ShadowRoot node");
            }
        }
    }

    #[test]
    fn declarative_shadow_dom_closed_mode() {
        use lumen_dom::ShadowRootMode;
        let doc = parse(r#"<section><template shadowrootmode="closed"><span>inner</span></template></section>"#);
        let body = doc.body().expect("body");
        let section = doc.get(body).children.first().copied().expect("section");
        let sr = doc.shadow_root_of(section).expect("shadow root on section");
        let node = doc.get(sr);
        if let lumen_dom::NodeData::ShadowRoot { mode } = &node.data {
            assert_eq!(*mode, ShadowRootMode::Closed);
        } else {
            panic!("expected ShadowRoot node");
        }
    }

    #[test]
    fn declarative_shadow_dom_template_element_removed_from_host() {
        let doc = parse(r#"<div><template shadowrootmode="open"><p>in shadow</p></template></div>"#);
        let body = doc.body().expect("body");
        let host = doc.get(body).children.first().copied().expect("div");
        // The <template> element must be detached — host's direct children should not include it.
        for &child in &doc.get(host).children {
            if let lumen_dom::NodeData::Element { name, .. } = &doc.get(child).data {
                assert_ne!(name.local.as_str(), "template", "template element must be detached");
            }
        }
    }

    #[test]
    fn declarative_shadow_dom_content_inside_shadow_root() {
        use lumen_dom::NodeData;
        let doc = parse(r#"<div><template shadowrootmode="open"><h1>Hello</h1><p>World</p></template></div>"#);
        let body = doc.body().expect("body");
        let host = doc.get(body).children.first().copied().expect("host");
        let sr = doc.shadow_root_of(host).expect("shadow root");
        let sr_children: Vec<_> = doc.get(sr).children.iter()
            .filter_map(|&n| {
                if let NodeData::Element { name, .. } = &doc.get(n).data {
                    Some(name.local.as_str().to_owned())
                } else {
                    None
                }
            })
            .collect();
        assert!(sr_children.contains(&"h1".to_owned()), "h1 in shadow root");
        assert!(sr_children.contains(&"p".to_owned()), "p in shadow root");
    }

    #[test]
    fn declarative_shadow_dom_slot_after_style_preserved() {
        // BUG-142: a `<style>` (rawtext) before a `<slot>` inside a declarative
        // shadow template must not leave the parser in InHead mode — otherwise the
        // `<slot>` is misplaced outside the shadow root and slotted content never
        // renders. Both `<style>` and `<slot>` must end up in the shadow root.
        use lumen_dom::NodeData;
        let doc = parse(
            r#"<div><template shadowrootmode="open"><style>:host{color:red}</style><slot></slot></template></div>"#,
        );
        let body = doc.body().expect("body");
        let host = doc.get(body).children.first().copied().expect("host");
        let sr = doc.shadow_root_of(host).expect("shadow root");
        let sr_children: Vec<_> = doc.get(sr).children.iter()
            .filter_map(|&n| {
                if let NodeData::Element { name, .. } = &doc.get(n).data {
                    Some(name.local.as_str().to_owned())
                } else {
                    None
                }
            })
            .collect();
        assert!(sr_children.contains(&"style".to_owned()), "style in shadow root: {sr_children:?}");
        assert!(sr_children.contains(&"slot".to_owned()), "slot must follow style into shadow root: {sr_children:?}");
    }

    #[test]
    fn regular_template_unaffected_by_declarative_shadow_dom() {
        let doc = parse(r#"<div><template><p>regular</p></template></div>"#);
        let body = doc.body().expect("body");
        let host = doc.get(body).children.first().copied().expect("div");
        // No shadow root on the host for a regular template.
        assert!(doc.shadow_root_of(host).is_none(), "no shadow root for regular template");
        // The <template> element remains as a child.
        let has_template = doc.get(host).children.iter().any(|&n| {
            matches!(&doc.get(n).data, lumen_dom::NodeData::Element { name, .. } if name.local == "template")
        });
        assert!(has_template, "regular template element must remain in DOM");
    }

    #[test]
    fn declarative_shadow_dom_invalid_mode_falls_back_to_regular_template() {
        let doc = parse(r#"<div><template shadowrootmode="invalid"><p>inside</p></template></div>"#);
        let body = doc.body().expect("body");
        let host = doc.get(body).children.first().copied().expect("div");
        // Invalid mode → treated as regular template, no shadow root attached.
        assert!(doc.shadow_root_of(host).is_none(), "invalid shadowrootmode must not attach shadow root");
    }

    #[test]
    fn declarative_shadow_dom_in_body() {
        use lumen_dom::ShadowRootMode;
        // <template shadowrootmode> in body (not head) should also work.
        let doc = parse(r#"<html><body><article><template shadowrootmode="open"><nav>menu</nav></template></article></body></html>"#);
        let body = doc.body().expect("body");
        let article = doc.get(body).children.first().copied().expect("article");
        let sr = doc.shadow_root_of(article).expect("shadow root on article");
        if let lumen_dom::NodeData::ShadowRoot { mode } = &doc.get(sr).data {
            assert_eq!(*mode, ShadowRootMode::Open);
        } else {
            panic!("expected ShadowRoot");
        }
    }

    // --- parse_xml_flavoured / BUG-786 CDATA slice ---

    #[test]
    fn xml_flavoured_strips_cdata_from_style() {
        let doc = parse_xml_flavoured(
            "<style type=\"text/css\"><![CDATA[\ndiv { color: red; }\n]]></style>",
        );
        let s = doc.to_string();
        assert!(s.contains("div { color: red; }"), "CDATA markers not stripped: {s}");
        assert!(!s.contains("CDATA"), "CDATA marker leaked into text node: {s}");
    }

    #[test]
    fn xml_flavoured_strips_cdata_from_inline_script() {
        let doc = parse_xml_flavoured("<script><![CDATA[\nvar x = 1 < 2;\n]]></script>");
        let s = doc.to_string();
        assert!(s.contains("var x = 1 < 2;"), "CDATA markers not stripped: {s}");
        assert!(!s.contains("CDATA"), "CDATA marker leaked into text node: {s}");
    }

    #[test]
    fn xml_flavoured_first_rule_survives_multi_rule_style() {
        // BUG-786 срез 13: без починки терялось именно ПЕРВОЕ правило блока —
        // не весь блок целиком.
        let doc = parse_xml_flavoured(
            "<style><![CDATA[\n#a { width: 100px; }\n#b { width: 200px; }\n]]></style>",
        );
        let s = doc.to_string();
        assert!(s.contains("#a { width: 100px; }"), "first rule lost: {s}");
        assert!(s.contains("#b { width: 200px; }"));
    }

    #[test]
    fn plain_parse_does_not_strip_cdata() {
        // HTML5 semantics (default `parse`) leave CDATA markers as literal
        // RAWTEXT — only `parse_xml_flavoured` opts into XML behaviour.
        let doc = parse("<style><![CDATA[div{color:red}]]></style>");
        let s = doc.to_string();
        assert!(s.contains("CDATA"), "plain HTML parse must not strip CDATA: {s}");
    }

    #[test]
    fn xml_flavoured_leaves_plain_style_untouched() {
        let doc = parse_xml_flavoured("<style>div { color: red; }</style>");
        let s = doc.to_string();
        assert!(s.contains("div { color: red; }"));
    }

    // --- parse_xml_flavoured / GAP-XMLDOC срез 2: self-closing non-void tags ---

    #[test]
    fn xml_flavoured_self_closing_div_does_not_nest_siblings() {
        // BUG-786 «Вторая грань»: HTML5 ignores `/>` on a non-void element, so
        // N sibling self-closing tags nest N deep instead of staying siblings.
        let doc = parse_xml_flavoured(r#"<div class="a"/><div class="b"/><div class="c"/>"#);
        let body = doc.body().expect("body");
        let children: Vec<NodeId> = doc.get(body).children.clone();
        assert_eq!(children.len(), 3, "three self-closed divs must be siblings: {}", doc);
    }

    #[test]
    fn xml_flavoured_self_closing_nested_container_closes_at_slash_gt() {
        let doc = parse_xml_flavoured(r#"<div id="outer"><span id="inner"/>tail</div>"#);
        let body = doc.body().expect("body");
        let outer = doc.get(body).children.first().copied().expect("outer div");
        // `tail` must be a sibling of the self-closed span, not its descendant.
        assert_eq!(doc.get(outer).children.len(), 2, "span + tail text: {}", doc);
    }

    #[test]
    fn plain_parse_ignores_self_closing_on_non_void_element() {
        // HTML5 semantics (default `parse`) — self-closing flag is ignored
        // outside void elements, so siblings still nest.
        let doc = parse(r#"<div class="a"/><div class="b"/>"#);
        let body = doc.body().expect("body");
        let children: Vec<NodeId> = doc.get(body).children.clone();
        assert_eq!(children.len(), 1, "plain HTML5 parse must nest, not close: {}", doc);
    }

    #[test]
    fn xml_flavoured_self_closing_script_does_not_swallow_following_markup() {
        // GAP-XMLDOC срез 16 case 2: a self-closing `<script src="…"/>` in a
        // plain HTML5 parse eats everything up to the next real `</script>`.
        // In xml_mode it must close immediately, leaving `<p>` a sibling.
        let doc = parse_xml_flavoured(r#"<script src="a.js"/><p>after</p>"#);
        let body = doc.body().expect("body");
        let has_p = doc.get(body).children.iter().any(|&n| {
            matches!(&doc.get(n).data, lumen_dom::NodeData::Element { name, .. } if name.local == "p")
        });
        assert!(has_p, "<p> after self-closing <script/> must survive: {}", doc);
    }

    #[test]
    fn xml_flavoured_self_closing_textarea_does_not_swallow_following_markup() {
        // GAP-XMLDOC срез 9: `<textarea/>` was not covered by срез 2's
        // RAWTEXT fix (title/style/script/noframes) because its start-tag
        // handler lives in mode_in_body, not mode_in_head, and never
        // consulted `self_closing`/`xml_mode` at all. In a plain HTML5
        // parse and, until this fix, also in xml_mode, a self-closing
        // `<textarea/>` still switched the tokenizer into Text mode and
        // swallowed everything up to the next literal `</textarea>`.
        let doc = parse_xml_flavoured(r#"<textarea/><p>after</p>"#);
        let body = doc.body().expect("body");
        let has_p = doc.get(body).children.iter().any(|&n| {
            matches!(&doc.get(n).data, lumen_dom::NodeData::Element { name, .. } if name.local == "p")
        });
        assert!(has_p, "<p> after self-closing <textarea/> must survive: {}", doc);
    }

    #[test]
    fn xml_flavoured_self_closing_h_script_with_src_is_requestable() {
        // WPT reftest idiom, 212 corpus files (GAP-XMLDOC срез 11 measurement):
        // `<svg xmlns:h="…/1999/xhtml"><h:script src="…"/></svg>`. Срез 5 already
        // strips the `h:` prefix into Namespace::Html; срез 2 already makes
        // self-closing skip RAWTEXT/stack-push in xml_mode. This test pins that
        // the combination — prefixed name AND self-closing AND an attribute —
        // still produces a normal HTML `<script src>` element a loader can find,
        // with a sibling surviving after it (not swallowed as RAWTEXT to EOF).
        let doc = parse_xml_flavoured(
            r#"<svg xmlns:h="http://www.w3.org/1999/xhtml"><h:script src="/common/reftest-wait.js"/><rect/></svg>"#,
        );
        let script = doc
            .find_first_element(
                |n| matches!(&n.data, NodeData::Element { name, .. } if name.local == "script"),
            )
            .unwrap_or_else(|| panic!("script element: {doc}"));
        let NodeData::Element { name, attrs, .. } = &script.data else {
            unreachable!()
        };
        assert_eq!(name.namespace, Namespace::Html, "h:script namespace: {doc}");
        assert!(
            attrs.iter().any(|a| a.name.local == "src" && a.value == "/common/reftest-wait.js"),
            "src attribute must survive on the self-closing element: {doc}"
        );
        assert!(script.children.is_empty(), "self-closing script must have no children: {doc}");
        let has_rect = doc.find_first_element(
            |n| matches!(&n.data, NodeData::Element { name, .. } if name.local == "rect"),
        );
        assert!(has_rect.is_some(), "<rect/> after self-closing <h:script/> must survive: {doc}");
    }

    #[test]
    fn plain_parse_self_closing_textarea_still_consumes_following_text() {
        // HTML5 semantics (default `parse`) — self-closing flag stays
        // ignored on non-void elements, so <textarea/> still opens a real
        // textarea whose content runs until the next `</textarea>`.
        let doc = parse("<textarea/>after</textarea><p>tail</p>");
        let body = doc.body().expect("body");
        let textarea = doc.get(body).children.first().copied().expect("textarea");
        let NodeData::Element { name, .. } = &doc.get(textarea).data else {
            panic!("textarea must be an element: {doc}");
        };
        assert_eq!(name.local, "textarea", "plain parse: {doc}");
        let has_p = doc.get(body).children.iter().any(|&n| {
            matches!(&doc.get(n).data, lumen_dom::NodeData::Element { name, .. } if name.local == "p")
        });
        assert!(has_p, "<p> after </textarea> must survive: {}", doc);
    }

    #[test]
    fn svg_descendants_get_svg_namespace() {
        // GAP-XMLDOC срез 3, BUG-685: everything under <svg> must stop landing
        // in Namespace::Html.
        let doc = parse("<body><svg><rect/><g><circle/></g></svg></body>");
        let body = doc.body().expect("body");
        let svg = doc.get(body).children.first().copied().expect("svg");
        let NodeData::Element { name, .. } = &doc.get(svg).data else {
            panic!("svg must be an element: {doc}");
        };
        assert_eq!(name.namespace, Namespace::Svg, "svg element: {doc}");
        let rect = doc.get(svg).children.first().copied().expect("rect");
        let NodeData::Element { name: rect_name, .. } = &doc.get(rect).data else {
            panic!("rect must be an element: {doc}");
        };
        assert_eq!(rect_name.namespace, Namespace::Svg, "rect element: {doc}");
    }

    #[test]
    fn svg_camel_case_tag_names_are_restored() {
        // The tokenizer lower-cases every tag name; foreign content must map
        // it back via the SVG spec's mixed-case table (BUG-685).
        let doc = parse("<svg><lineargradient id=\"g\"></lineargradient></svg>");
        let svg_id = doc
            .get(doc.root())
            .children
            .iter()
            .find_map(|&c| find_node(&doc, c, "svg"))
            .unwrap_or_else(|| panic!("svg node id: {doc}"));
        let grad = doc.get(svg_id).children.first().copied().expect("linearGradient child");
        let NodeData::Element { name, .. } = &doc.get(grad).data else {
            panic!("child must be an element: {doc}");
        };
        assert_eq!(name.local, "linearGradient", "case must be restored: {doc}");
    }

    #[test]
    fn svg_self_closing_shapes_do_not_nest_siblings() {
        // §13.2.6.5 step 4 honours the self-closing flag unconditionally in
        // foreign content, unlike ordinary HTML elements (BUG-685).
        let doc = parse("<svg><rect/><circle/></svg>");
        let svg = doc
            .get(doc.root())
            .children
            .iter()
            .find_map(|&c| find_node(&doc, c, "svg"))
            .unwrap_or_else(|| panic!("svg node id: {doc}"));
        assert_eq!(
            doc.get(svg).children.len(),
            2,
            "rect + circle must be siblings, not nested: {}",
            doc
        );
    }

    #[test]
    fn svg_breakout_tag_returns_to_html_namespace() {
        // <div> is on the §13.2.6.5 breakout list — it must land back in
        // Namespace::Html even while nested inside plain <svg> markup
        // (no integration point involved: <g> is an ordinary SVG element).
        let doc = parse("<svg><g><div>text</div></g></svg>");
        let div = doc
            .find_first_element(|n| matches!(&n.data, NodeData::Element { name, .. } if name.local == "div"))
            .expect("div element");
        let NodeData::Element { name, .. } = &div.data else {
            unreachable!()
        };
        assert_eq!(name.namespace, Namespace::Html, "breakout div: {doc}");
        let body = doc.body().expect("body");
        assert_eq!(
            div.parent,
            Some(body),
            "breakout must pop <g>/<svg> off the stack, landing div as a body child: {doc}"
        );
    }

    #[test]
    fn svg_foreign_object_is_html_integration_point() {
        // GAP-XMLDOC срез 8, BUG-685: `<foreignObject>` is an HTML LS
        // §13.2.6.5 integration point — markup nested inside it is genuine
        // HTML content, kept nested (not popped back out like the ordinary
        // breakout list does for plain SVG elements).
        let doc = parse("<svg><foreignObject><div>text</div></foreignObject></svg>");
        let foreign_object = doc
            .find_first_element(
                |n| matches!(&n.data, NodeData::Element { name, .. } if name.local == "foreignObject"),
            )
            .expect("foreignObject element");
        let div = foreign_object.children.first().copied().expect("div child");
        let NodeData::Element { name, .. } = &doc.get(div).data else {
            panic!("div must be an element: {doc}");
        };
        assert_eq!(name.namespace, Namespace::Html, "div under foreignObject: {doc}");
    }

    #[test]
    fn svg_desc_is_html_integration_point() {
        // Only <desc> here, not <title>: the tokenizer decides RAWTEXT/RCDATA
        // by bare tag name, with no namespace awareness (same root cause as
        // GAP-XMLDOC срез 5's `html:title`/`h:title` carve-out) — a plain
        // `<title>` start tag always switches to RCDATA, HTML or SVG, so an
        // SVG `<title>` containing markup mis-tokenizes exactly like a
        // misplaced HTML one does. Out of scope here: fixing it needs the
        // tokenizer to consult tree-construction namespace state, which is a
        // bigger structural change than this slice's point fix.
        let doc = parse("<svg><desc><span>d</span></desc></svg>");
        let span = doc
            .find_first_element(|n| matches!(&n.data, NodeData::Element { name, .. } if name.local == "span"))
            .expect("span element");
        let NodeData::Element { name, .. } = &span.data else {
            unreachable!()
        };
        assert_eq!(name.namespace, Namespace::Html, "span under desc: {doc}");
    }

    #[test]
    fn svg_reenters_foreign_namespace_from_inside_integration_point() {
        // A nested <svg>/<math> inside an integration point's HTML content
        // must switch back to its own foreign namespace — integration
        // points don't disable re-entry, only default new elements to HTML.
        let doc = parse("<svg><foreignObject><math><mi>x</mi></math></foreignObject></svg>");
        let math = doc
            .find_first_element(|n| matches!(&n.data, NodeData::Element { name, .. } if name.local == "math"))
            .expect("math element");
        let NodeData::Element { name, .. } = &math.data else {
            unreachable!()
        };
        assert_eq!(name.namespace, Namespace::MathMl, "nested math: {doc}");
    }

    #[test]
    fn mathml_descendants_get_mathml_namespace() {
        // GAP-XMLDOC срез 6, BUG-685: same gap as srez 3, other namespace.
        let doc = parse("<body><math><mrow><mi>x</mi></mrow></math></body>");
        let body = doc.body().expect("body");
        let math = doc.get(body).children.first().copied().expect("math");
        let NodeData::Element { name, .. } = &doc.get(math).data else {
            panic!("math must be an element: {doc}");
        };
        assert_eq!(name.namespace, Namespace::MathMl, "math element: {doc}");
        let mrow = doc.get(math).children.first().copied().expect("mrow");
        let NodeData::Element { name: mrow_name, .. } = &doc.get(mrow).data else {
            panic!("mrow must be an element: {doc}");
        };
        assert_eq!(mrow_name.namespace, Namespace::MathMl, "mrow element: {doc}");
    }

    #[test]
    fn mathml_definitionurl_attribute_case_is_restored() {
        let doc = parse(r#"<math><mo definitionurl="foo">x</mo></math>"#);
        let mo = doc
            .find_first_element(|n| matches!(&n.data, NodeData::Element { name, .. } if name.local == "mo"))
            .expect("mo element");
        let NodeData::Element { attrs, .. } = &mo.data else {
            unreachable!()
        };
        assert!(
            attrs.iter().any(|a| a.name.local == "definitionURL"),
            "definitionurl must be case-restored: {doc}"
        );
    }

    #[test]
    fn svg_xlink_href_gets_xlink_namespace() {
        // GAP-XMLDOC срез 10, BUG-685: §13.2.6.5 "adjust foreign attributes".
        let doc = parse(r##"<svg><use xlink:href="#a"></use></svg>"##);
        let use_el = doc
            .find_first_element(|n| matches!(&n.data, NodeData::Element { name, .. } if name.local == "use"))
            .expect("use element");
        let NodeData::Element { attrs, .. } = &use_el.data else {
            unreachable!()
        };
        let attr = attrs
            .iter()
            .find(|a| a.name.local == "xlink:href")
            .unwrap_or_else(|| panic!("xlink:href attribute: {doc}"));
        assert_eq!(attr.name.namespace, Namespace::XLink);
        assert_eq!(attr.value, "#a");
    }

    #[test]
    fn mathml_xlink_href_gets_xlink_namespace() {
        // Foreign-attribute adjustment runs for MathML too, not just SVG.
        let doc = parse(r##"<math><mi xlink:href="#a">x</mi></math>"##);
        let mi = doc
            .find_first_element(|n| matches!(&n.data, NodeData::Element { name, .. } if name.local == "mi"))
            .expect("mi element");
        let NodeData::Element { attrs, .. } = &mi.data else {
            unreachable!()
        };
        let attr = attrs
            .iter()
            .find(|a| a.name.local == "xlink:href")
            .unwrap_or_else(|| panic!("xlink:href attribute: {doc}"));
        assert_eq!(attr.name.namespace, Namespace::XLink);
    }

    #[test]
    fn xmlns_and_xml_lang_get_their_namespace() {
        let doc = parse(r#"<svg xmlns:xlink="http://www.w3.org/1999/xlink"><rect xml:lang="en"></rect></svg>"#);
        let svg = doc
            .find_first_element(|n| matches!(&n.data, NodeData::Element { name, .. } if name.local == "svg"))
            .expect("svg element");
        let NodeData::Element { attrs, .. } = &svg.data else {
            unreachable!()
        };
        let xmlns_xlink = attrs
            .iter()
            .find(|a| a.name.local == "xmlns:xlink")
            .unwrap_or_else(|| panic!("xmlns:xlink attribute: {doc}"));
        assert_eq!(xmlns_xlink.name.namespace, Namespace::XmlNs);

        let rect = doc
            .find_first_element(|n| matches!(&n.data, NodeData::Element { name, .. } if name.local == "rect"))
            .expect("rect element");
        let NodeData::Element { attrs, .. } = &rect.data else {
            unreachable!()
        };
        let xml_lang = attrs
            .iter()
            .find(|a| a.name.local == "xml:lang")
            .unwrap_or_else(|| panic!("xml:lang attribute: {doc}"));
        assert_eq!(xml_lang.name.namespace, Namespace::Xml);
    }

    #[test]
    fn plain_svg_attributes_keep_html_namespace() {
        // Only the eleven listed names get a real namespace — everything
        // else (including `href` itself, unlike its `xlink:` sibling) stays
        // Html-namespaced, matching every other SVG/MathML attribute.
        let doc = parse(r##"<svg><a href="#a"></a></svg>"##);
        let a = doc
            .find_first_element(|n| matches!(&n.data, NodeData::Element { name, .. } if name.local == "a"))
            .expect("a element");
        let NodeData::Element { attrs, .. } = &a.data else {
            unreachable!()
        };
        let href = attrs
            .iter()
            .find(|attr| attr.name.local == "href")
            .unwrap_or_else(|| panic!("href attribute: {doc}"));
        assert_eq!(href.name.namespace, Namespace::Html);
    }

    #[test]
    fn mathml_self_closing_elements_do_not_nest_siblings() {
        let doc = parse("<math><mspace/><mspace/></math>");
        let math = doc
            .get(doc.root())
            .children
            .iter()
            .find_map(|&c| find_node(&doc, c, "math"))
            .unwrap_or_else(|| panic!("math node id: {doc}"));
        assert_eq!(
            doc.get(math).children.len(),
            2,
            "mspace + mspace must be siblings, not nested: {}",
            doc
        );
    }

    #[test]
    fn mathml_breakout_tag_returns_to_html_namespace() {
        // <div> is on the shared §13.2.6.5 breakout list — must land back in
        // Namespace::Html even while nested inside plain <math> markup
        // (no integration point involved: <mrow> is an ordinary MathML
        // element).
        let doc = parse("<math><mrow><div>text</div></mrow></math>");
        let div = doc
            .find_first_element(|n| matches!(&n.data, NodeData::Element { name, .. } if name.local == "div"))
            .expect("div element");
        let NodeData::Element { name, .. } = &div.data else {
            unreachable!()
        };
        assert_eq!(name.namespace, Namespace::Html, "breakout div: {doc}");
        let body = doc.body().expect("body");
        assert_eq!(
            div.parent,
            Some(body),
            "breakout must pop <mrow>/<math> off the stack, landing div as a body child: {doc}"
        );
    }

    #[test]
    fn mathml_text_integration_point_nests_html_children() {
        // GAP-XMLDOC срез 8, BUG-685: MathML text integration points
        // (`mi`/`mo`/`mn`/`ms`/`mtext`) keep HTML content nested instead of
        // popping back out like the ordinary breakout list — same
        // "integration point" concept as SVG's foreignObject/desc/title.
        let doc = parse("<math><mtext><div>text</div></mtext></math>");
        let mtext = doc
            .find_first_element(|n| matches!(&n.data, NodeData::Element { name, .. } if name.local == "mtext"))
            .expect("mtext element");
        let div = mtext.children.first().copied().expect("div child");
        let NodeData::Element { name, .. } = &doc.get(div).data else {
            panic!("div must be an element: {doc}");
        };
        assert_eq!(name.namespace, Namespace::Html, "div under mtext: {doc}");
    }

    #[test]
    fn mathml_text_integration_point_mglyph_stays_mathml() {
        // The one exception in the MathML text integration point rule:
        // `mglyph`/`malignmark` children stay MathML, not HTML, even
        // directly inside `mi`/`mo`/`mn`/`ms`/`mtext`.
        let doc = parse("<math><mtext><mglyph/></mtext></math>");
        let mglyph = doc
            .find_first_element(
                |n| matches!(&n.data, NodeData::Element { name, .. } if name.local == "mglyph"),
            )
            .expect("mglyph element");
        let NodeData::Element { name, .. } = &mglyph.data else {
            unreachable!()
        };
        assert_eq!(name.namespace, Namespace::MathMl, "mglyph under mtext: {doc}");
    }

    #[test]
    fn mathml_annotation_xml_html_encoding_is_integration_point() {
        let doc = parse(r#"<math><annotation-xml encoding="text/html"><div>x</div></annotation-xml></math>"#);
        let anno = doc
            .find_first_element(
                |n| matches!(&n.data, NodeData::Element { name, .. } if name.local == "annotation-xml"),
            )
            .expect("annotation-xml element");
        let div = anno.children.first().copied().expect("div child");
        let NodeData::Element { name, .. } = &doc.get(div).data else {
            panic!("div must be an element: {doc}");
        };
        assert_eq!(name.namespace, Namespace::Html, "div under annotation-xml: {doc}");
    }

    #[test]
    fn mathml_annotation_xml_without_html_encoding_is_not_integration_point() {
        let doc = parse(r#"<math><annotation-xml encoding="application/mathml+xml"><mrow/></annotation-xml></math>"#);
        let mrow = doc
            .find_first_element(|n| matches!(&n.data, NodeData::Element { name, .. } if name.local == "mrow"))
            .expect("mrow element");
        let NodeData::Element { name, .. } = &mrow.data else {
            unreachable!()
        };
        assert_eq!(name.namespace, Namespace::MathMl, "mrow under non-html annotation-xml: {doc}");
    }

    #[test]
    fn svg_always_becomes_svg_under_annotation_xml_regardless_of_encoding() {
        // §13.2.6.5 "insert a foreign element" exception: <svg> as a direct
        // child of annotation-xml is always SVG, even without an
        // HTML-flavoured encoding.
        let doc = parse(r#"<math><annotation-xml encoding="application/mathml+xml"><svg><rect/></svg></annotation-xml></math>"#);
        let svg = doc
            .find_first_element(|n| matches!(&n.data, NodeData::Element { name, .. } if name.local == "svg"))
            .expect("svg element");
        let NodeData::Element { name, .. } = &svg.data else {
            unreachable!()
        };
        assert_eq!(name.namespace, Namespace::Svg, "svg under annotation-xml: {doc}");
    }

    #[test]
    fn html_prefixed_script_breaks_out_of_svg_and_stays_rawtext() {
        // GAP-XMLDOC срез 5 (BUG-685, «Третья грань, случай 1»): WPT's
        // `<svg xmlns:h="…/1999/xhtml"><h:script>…</h:script></svg>` idiom —
        // `script` is not on the ordinary §13.2.6.5 breakout list, but the
        // `h:`/`html:` prefix forces it anyway, and the element must land in
        // Namespace::Html (`getBBox` etc. must not apply to it).
        let doc = parse_xml_flavoured("<svg><h:script>var x = 1;</h:script></svg>");
        let script = doc
            .find_first_element(
                |n| matches!(&n.data, NodeData::Element { name, .. } if name.local == "script"),
            )
            .unwrap_or_else(|| panic!("script element: {doc}"));
        let NodeData::Element { name, .. } = &script.data else {
            unreachable!()
        };
        assert_eq!(name.namespace, Namespace::Html, "h:script namespace: {doc}");
        assert_eq!(
            script.children.len(),
            1,
            "script body must be a single RAWTEXT node, not parsed markup: {doc}"
        );
    }

    #[test]
    fn html_prefixed_script_cdata_body_is_unwrapped() {
        // Same breakout as above, combined with the срез 1 CDATA strip —
        // both fixes must compose on the same element.
        let doc =
            parse_xml_flavoured("<svg><html:script><![CDATA[var x = 1 < 2;]]></html:script></svg>");
        let script = doc
            .find_first_element(
                |n| matches!(&n.data, NodeData::Element { name, .. } if name.local == "script"),
            )
            .unwrap_or_else(|| panic!("script element: {doc}"));
        let &child = script
            .children
            .first()
            .unwrap_or_else(|| panic!("script text child: {doc}"));
        let NodeData::Text(text) = &doc.get(child).data else {
            panic!("script child must be text: {doc}");
        };
        assert_eq!(text, "var x = 1 < 2;");
    }

    #[test]
    fn html_prefixed_void_element_breaks_out() {
        let doc = parse_xml_flavoured(r#"<svg><h:meta charset="utf-8"/></svg>"#);
        let meta = doc
            .find_first_element(
                |n| matches!(&n.data, NodeData::Element { name, .. } if name.local == "meta"),
            )
            .unwrap_or_else(|| panic!("meta element: {doc}"));
        let NodeData::Element { name, .. } = &meta.data else {
            unreachable!()
        };
        assert_eq!(name.namespace, Namespace::Html, "h:meta namespace: {doc}");
    }

    #[test]
    fn html_prefixed_end_tag_closes_broken_out_element() {
        // Regression for the prefix carrying over to the *closing* tag too:
        // by the time `</html:div>` arrives the element is already in
        // Namespace::Html, so the close must not require an exact
        // "html:div" == "html:div" match against the (unprefixed) open
        // element — two siblings, not one nesting the other.
        let doc = parse_xml_flavoured("<svg><html:div>a</html:div><html:div>b</html:div></svg>");
        // Breaking out of foreign content pops all the way past <svg> (same
        // as `svg_breakout_tag_returns_to_html_namespace` above) — the divs
        // land as <body> children, siblings of <svg>, not inside it.
        let body = body_of(&doc);
        let divs: Vec<NodeId> = doc
            .get(body)
            .children
            .iter()
            .copied()
            .filter(|&c| {
                matches!(&doc.get(c).data, NodeData::Element { name, .. } if name.local == "div")
            })
            .collect();
        assert_eq!(divs.len(), 2, "two html:div siblings must not nest: {doc}");
        for child in divs {
            let NodeData::Element { name, .. } = &doc.get(child).data else {
                panic!("expected element child: {doc}");
            };
            assert_eq!(name.local, "div");
            assert_eq!(name.namespace, Namespace::Html);
        }
    }

    #[test]
    fn other_namespace_prefixes_do_not_break_out() {
        // `d:` (SVG 1.1 test-metadata namespace) and `svg:` are not `h:`/
        // `html:` — must stay untouched, still SVG-namespaced with the
        // literal prefixed local name (no resolver, see
        // `foreign_content::strip_known_html_prefix`).
        let doc = parse_xml_flavoured("<svg><d:testDescription>note</d:testDescription></svg>");
        // Not on the SVG tag-name-casing table (`adjust_svg_tag_name` only
        // knows official SVG local names) — stays exactly as the tokenizer
        // lower-cased it, same as any other unrecognized foreign tag.
        let el = doc
            .find_first_element(
                |n| matches!(&n.data, NodeData::Element { name, .. } if name.local == "d:testdescription"),
            )
            .unwrap_or_else(|| panic!("d:testdescription element: {doc}"));
        let NodeData::Element { name, .. } = &el.data else {
            unreachable!()
        };
        assert_eq!(name.namespace, Namespace::Svg, "d: prefix must stay SVG: {doc}");
    }

    /// Test helper: depth-first search under `id` for the first element whose
    /// local name is `local`, ASCII-case-insensitively.
    fn find_node(doc: &Document, id: NodeId, local: &str) -> Option<NodeId> {
        if matches!(&doc.get(id).data, NodeData::Element { name, .. } if name.local.eq_ignore_ascii_case(local))
        {
            return Some(id);
        }
        doc.get(id)
            .children
            .iter()
            .find_map(|&c| find_node(doc, c, local))
    }
}
