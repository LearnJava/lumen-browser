//! Arena-based DOM tree. Build via `Document::create_*` and `append_child`.
//!
//! # Invariant (10B / ADR-008)
//! The entire node graph lives in a **contiguous `Vec<Node>` arena** addressed by
//! `NodeId(u32)`. No `Rc<RefCell<…>>` exists in the graph — children and parents are
//! plain index values. This makes the tree `Send + Sync`, enables O(1) random access,
//! and guarantees that the snapshot serialised by [`Document::to_bytes`] is a flat
//! byte blob with no pointer fixups.

// Долг по документации: файл написан до включения `missing_docs` и пока не
// покрыт. Область исключения — файл, а не крейт, поэтому НОВЫЙ файл обязан
// документировать публичный API. Счётчики по крейтам — docs/lint-policy.md §10.
#![allow(missing_docs)]

// Catch the most common forms of accidental Rc-in-arena.
#![deny(clippy::rc_buffer)]

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::fmt;

use serde::{Deserialize, Serialize};

pub use lumen_core::sandbox::{parse_sandbox_value, SandboxFlags};

pub mod attr_int;
pub mod contenteditable;
pub use contenteditable::{CommandHistory, DomCommand, DragData, PasteData, drop_into, paste_into};

pub mod vtt;
pub use vtt::{TrackInfo, VideoTracks, VttCue, VttCueSettings, collect_video_tracks, parse_vtt};

mod forms;
pub use forms::{
    check_form_gate, check_validity_form, collect_dom_form_fields, element_validity,
    find_ancestor_form, invalid_controls_in_form, submit_form, FormInfo, FormSubmitEvent,
    InputMode, InputType, ValidityState,
};
#[cfg(test)]
use forms::collect_forms;

mod selection;
pub use selection::{
    delete_range, insert_paragraph_break, insert_text_at, locate_text_offset_range,
    node_child_count, node_length, node_text_content, range_text, split_text_node,
    DomPosition, Range, Selection,
};

mod ime;
pub use ime::{
    CompositionData, CompositionEvent, CompositionEventType, CompositionState, EditInputType,
    InputEvent,
};

mod font_faces;
pub use font_faces::{FontFace, FontFaceSet, FontFaceStatus};

mod performance;
pub use performance::{PerformanceEntries, PerformanceEntry, PerformanceEntryType, PerformanceObserver};

/// Width dimension of a `<meta name=viewport>` tag.
///
/// `DeviceWidth` means `width=device-width` — match the physical viewport.
/// `Pixels(f32)` is an explicit CSS px width (e.g. `width=375`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ViewportWidth {
    /// `width=device-width` — layout viewport equals the physical viewport width.
    DeviceWidth,
    /// `width=<N>` — fixed CSS pixel width.
    Pixels(f32),
}

/// Parsed `<meta name="viewport" content="…">` descriptor.
///
/// Extracted by the HTML parser; consumed by the shell to compute the effective
/// CSS layout viewport and by layout for `@media` matching.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ViewportMeta {
    /// `initial-scale` value. Default `1.0` when not specified.
    pub initial_scale: f32,
    /// `width` dimension. `None` if omitted.
    pub width: Option<ViewportWidth>,
}

impl Default for ViewportMeta {
    fn default() -> Self {
        Self { initial_scale: 1.0, width: None }
    }
}

/// Error returned by [`Document::to_bytes`] and [`Document::from_bytes`].
#[derive(Debug)]
pub enum DomSnapshotError {
    /// bincode encode failed.
    Encode(bincode::Error),
    /// bincode decode failed.
    Decode(bincode::Error),
}

impl fmt::Display for DomSnapshotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Encode(e) => write!(f, "DOM snapshot encode error: {e}"),
            Self::Decode(e) => write!(f, "DOM snapshot decode error: {e}"),
        }
    }
}

impl std::error::Error for DomSnapshotError {}

/// Hard limit on the number of nodes in a single `Document` arena.
///
/// JS-driven `document.createElement()` returns a `QuotaExceededError` when this
/// threshold is reached. HTML-parser allocations are not gated (they use
/// `create_element` directly), so overly large HTML files may still exceed the
/// limit in the arena — this guard is a JS-mutation fence, not a hard memory cap.
pub const MAX_DOM_NODES: usize = 50_000;

/// Soft warning threshold — `console.warn` fires once when node count crosses this.
pub const WARN_DOM_NODES: usize = 40_000;

/// Returned by [`Document::try_create_element`] when [`MAX_DOM_NODES`] is reached.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeLimitExceeded;

impl fmt::Display for NodeLimitExceeded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "DOM node limit exceeded (max {MAX_DOM_NODES})")
    }
}

impl std::error::Error for NodeLimitExceeded {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodeId(u32);

impl NodeId {
    pub fn index(self) -> usize {
        self.0 as usize
    }

    pub fn from_index(i: usize) -> Self {
        NodeId(i as u32)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Namespace {
    Html,
    Svg,
    MathMl,
    Xml,
    XmlNs,
    XLink,
    /// No namespace — `Element.namespaceURI` is `null` (DOM Standard §4.5
    /// "validate and extract", e.g. `createElementNS(null, name)` /
    /// `createElementNS("", name)`). Distinct from `Html`: BUG-328.
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct QualName {
    pub namespace: Namespace,
    pub local: String,
}

impl QualName {
    pub fn html(local: impl Into<String>) -> Self {
        Self {
            namespace: Namespace::Html,
            local: local.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attribute {
    pub name: QualName,
    pub value: String,
}

/// Shadow root mode per Shadow DOM spec §4.2.
///
/// `Open` — JS can access the shadow root via `element.shadowRoot`.
/// `Closed` — `element.shadowRoot` returns `null` (encapsulated).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShadowRootMode {
    Open,
    Closed,
}

impl fmt::Display for ShadowRootMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Open => f.write_str("open"),
            Self::Closed => f.write_str("closed"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NodeData {
    Document,
    Doctype {
        name: String,
        public_id: String,
        system_id: String,
    },
    /// Root of a shadow tree attached to a shadow host element.
    ///
    /// Not a regular DOM child — the host stores a pointer via
    /// `Document.shadow_roots`. Contains the shadow subtree as DOM children.
    /// Layout uses this through the composed (flat) tree; see `build_flat_tree`.
    ShadowRoot {
        mode: ShadowRootMode,
    },
    Element {
        name: QualName,
        attrs: Vec<Attribute>,
    },
    Text(String),
    Comment(String),
    /// Inert subtree used as the content container for `<template>` elements.
    ///
    /// DOM Living Standard §4.5: a DocumentFragment has no parent and is not
    /// rendered directly. The `<template>` element stores its content here;
    /// callers clone the fragment into the live tree via `deep_clone`.
    ///
    /// Stored in the arena like any node. The mapping `template → fragment` is
    /// kept in `Document::template_contents`.
    DocumentFragment,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
    pub data: NodeData,
}

impl Node {
    pub fn element_name(&self) -> Option<&QualName> {
        match &self.data {
            NodeData::Element { name, .. } => Some(name),
            _ => None,
        }
    }

    /// Возвращает значение атрибута по имени (ASCII case-insensitive). На
    /// текстовых узлах и комментариях — `None`.
    pub fn get_attr(&self, name: &str) -> Option<&str> {
        match &self.data {
            NodeData::Element { attrs, .. } => attrs
                .iter()
                .find(|a| a.name.local.eq_ignore_ascii_case(name))
                .map(|a| a.value.as_str()),
            _ => None,
        }
    }

    /// Sandbox-ограничения для `<iframe sandbox="...">` по HTML LS §7.6.5.
    ///
    /// Возвращает `None` для всех не-`iframe` элементов. Для `<iframe>` без
    /// атрибута `sandbox` — `SandboxFlags::empty()` (без ограничений). Для
    /// `<iframe sandbox>` или `<iframe sandbox="">` — `SandboxFlags::all_restrictions()`.
    /// Конкретные `allow-*` keyword-ы снимают соответствующие биты.
    pub fn sandbox_flags(&self) -> Option<SandboxFlags> {
        let name = self.element_name()?;
        if !name.local.eq_ignore_ascii_case("iframe") {
            return None;
        }
        Some(parse_sandbox_value(self.get_attr("sandbox")))
    }

    /// HTML5 form input type для `<input type="...">`. Возвращает None
    /// для всех не-`input` элементов. Для `<input>` без явного `type` —
    /// `InputType::Text` (HTML5 default). Парсинг case-insensitive,
    /// неизвестные имена → `Other(String)` для forward-compat.
    pub fn input_type(&self) -> Option<InputType> {
        let name = self.element_name()?;
        if !name.local.eq_ignore_ascii_case("input") {
            return None;
        }
        let raw = self.get_attr("type").unwrap_or("text");
        Some(InputType::parse(raw))
    }

    /// Virtual keyboard hint for `<input inputmode="...">` and `<textarea inputmode="...">`.
    /// Returns `InputMode::Text` (default) for all non-input/non-textarea elements or when
    /// the attribute is absent. Parsing is case-insensitive; unknown values default to `Text`.
    ///
    /// Used by shell to select IME/virtual keyboard type for text input fields.
    pub fn input_mode(&self) -> Option<InputMode> {
        let name = self.element_name()?;
        if !name.local.eq_ignore_ascii_case("input") && !name.local.eq_ignore_ascii_case("textarea") {
            return None;
        }
        let raw = self.get_attr("inputmode").unwrap_or("text");
        Some(InputMode::parse(raw))
    }
}

/// Парсинг-режим документа по HTML5 §13.2.6.2 «The insertion mode».
///
/// Решается tree builder-ом по DOCTYPE-токену (см. §13.2.5.1
/// «The initial insertion mode»). На один Document приходится один режим
/// — он фиксируется в момент обработки первого DOCTYPE и больше не
/// меняется. Используется hot-path-ами layout/cascade для переключения
/// десятков legacy CSS-поведений (table sizing, body-background
/// propagation, font-size в `<table>`, и т.д.).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum DocumentMode {
    /// Standards / no-quirks mode — действуют современные правила.
    /// Сюда попадают `<!DOCTYPE html>` и большинство XHTML DOCTYPE.
    #[default]
    NoQuirks,
    /// Quirks mode — legacy-режим без DOCTYPE или с устаревшими
    /// PUBLIC IDs (HTML 2.0/3.x, HTML 4.x не-Strict без system_id).
    Quirks,
    /// Limited-quirks mode — узкий промежуточный режим для HTML 4.0/4.01
    /// Frameset / Transitional с правильным system_id и XHTML 1.0
    /// Frameset / Transitional. Большинство правил совпадает с
    /// no-quirks, но несколько (например, table cellpadding) — quirks.
    LimitedQuirks,
}

// ── Selection / Range ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    nodes: Vec<Node>,
    root: NodeId,
    mode: DocumentMode,
    target_id: Option<String>,
    /// Maps each shadow host `NodeId` to its shadow root `NodeId`.
    ///
    /// Shadow roots are stored in the arena like regular nodes but are not
    /// DOM children of the host. The flat tree (see `build_flat_tree`) uses
    /// this map to route layout traversal through shadow trees.
    shadow_roots: HashMap<NodeId, NodeId>,
    /// Maps each `<template>` element `NodeId` to its content `DocumentFragment` `NodeId`.
    ///
    /// The fragment is stored in the arena but is not a DOM child of the
    /// template element — `template.children` is always empty. Callers access
    /// template content via [`Document::template_content`].
    template_contents: HashMap<NodeId, NodeId>,
    /// The current text selection. Updated by the shell on mouse events;
    /// read by layout for `selection_rects` and by JS via `window.getSelection()`.
    selection: Selection,
    /// The active IME composition session (if any).
    /// Tracks preedit text and range while the user is composing via an IME.
    /// Cleared when composition ends.
    composition: Option<CompositionState>,
    /// Collection of FontFace objects from @font-face rules in stylesheets.
    /// Populated when stylesheets are parsed; exposed to JS via document.fonts.
    fonts: FontFaceSet,
    /// Performance entries (marks, measures, resource timings).
    /// Exposed to JS via window.performance.
    performance: PerformanceEntries,
    /// Navigation start time (milliseconds since epoch).
    /// Used as the reference point (0.0) for all performance timings.
    timing_origin: f64,
    /// Counts live JS wrapper objects referencing each `NodeId`.
    ///
    /// Incremented by [`Document::acquire_js_ref`] when the JS runtime creates a
    /// wrapper object for a DOM node; decremented by [`Document::release_js_ref`]
    /// when the QuickJS finalizer fires (Phase 3: P3 wires the finalizer callback).
    ///
    /// Not serialised — JS objects do not survive tab hibernation. On restore,
    /// the JS heap is rebuilt from scratch and wrappers re-acquire refs.
    #[serde(skip)]
    js_refs: HashMap<NodeId, u32>,
    /// Parsed `<meta name="viewport">` descriptor, if any.
    ///
    /// Set by the HTML parser when the viewport meta tag is encountered in `<head>`.
    /// Used by the shell to compute the effective CSS layout viewport width/scale.
    viewport_meta: Option<ViewportMeta>,
    /// Active pointer captures: maps `pointerId` → captured `NodeId`.
    ///
    /// Set by `Element.setPointerCapture(pointerId)` (W3C Pointer Events L3 §4.1).
    /// All pointer events for a captured `pointerId` are routed to the capture
    /// target instead of the hit-tested element, until the capture is released.
    /// Not serialised — captures are transient input state cleared on page restore.
    #[serde(skip)]
    pointer_captures: HashMap<u32, NodeId>,
    /// Runtime («dirty») value of a form control, keyed by its `NodeId`.
    ///
    /// HTML LS §4.10.5.5: an `<input>`/`<textarea>` has a *value* that the
    /// content attribute (`value=` / the child text) only **seeds**. Once
    /// anything sets the value — the user typing, a script assigning
    /// `el.value`, a picker — the control's dirty value flag is raised and the
    /// two diverge for good: the attribute stays the *default* value that
    /// `defaultValue` reads and `form.reset()` restores.
    ///
    /// An entry here **is** that dirty value flag: present → this map is the
    /// value; absent → the default derived from the DOM. Everything that needs
    /// the current value (layout, form submission, constraint validation,
    /// `:placeholder-shown`) goes through [`Document::control_value`], so the
    /// engine has exactly one source of truth for what the field shows and
    /// submits (BUG-441).
    ///
    /// Serialised: the text a user typed must survive tab hibernation.
    #[serde(default)]
    dirty_values: HashMap<NodeId, String>,
    /// Runtime («dirty») checkedness of a checkbox/radio `<input>`, keyed by
    /// its `NodeId` — the same shape as [`Document::dirty_values`], for the
    /// same reason (BUG-444). HTML LS §4.10.5.5 distinguishes *checkedness*
    /// (current state, toggled by the user or a script) from the `checked`
    /// content attribute (the default `defaultChecked` reads and
    /// `form.reset()` restores): once anything sets checkedness, the two
    /// diverge for good via the control's "dirty checkedness flag".
    ///
    /// An entry here **is** that flag: present → this map is the current
    /// checkedness; absent → it falls back to the `checked` attribute, which
    /// is exactly the spec rule for a content-attribute change reaching a
    /// non-dirty control. [`Document::control_checked`] is the one place that
    /// reads current checkedness (`:checked`/`:indeterminate` matching,
    /// checkbox painting, form submission, constraint validation).
    ///
    /// Serialised: matches `dirty_values` — a checkbox a user (un)ticked must
    /// stay that way across tab hibernation.
    #[serde(default)]
    dirty_checkedness: HashMap<NodeId, bool>,
    /// `document.designMode` (HTML LS §6.6.3): when `true`, the whole document
    /// becomes an editing host even though no element carries an explicit
    /// `contenteditable` attribute — see [`find_editing_host`].
    ///
    /// Serialised: matches `dirty_values` — a page that turned itself into an
    /// editor should stay editable across tab hibernation.
    #[serde(default)]
    design_mode: bool,
    /// WHATWG Encoding Standard canonical name of the encoding this document
    /// was decoded with (e.g. `"UTF-8"`, `"windows-1251"`) — backs
    /// `document.characterSet`/`charset`/`inputEncoding` (DOM §7.3, BUG-358).
    /// Set once by the shell right after `lumen_encoding::detect()` runs
    /// (`parse_and_layout`), before any script observes it.
    ///
    /// Serialised: this is a fact about how the document's bytes were decoded,
    /// not runtime UI state — it stays valid (and must stay readable) across
    /// bfcache restore, unlike `js_refs`/`pointer_captures`.
    #[serde(default = "default_character_set")]
    character_set: String,
    /// MIME type of this document (e.g. `"text/html"`) — backs
    /// `document.contentType` (DOM §4.5, BUG-358). Set once by the shell from
    /// the `content_type` hint `parse_and_layout` already receives.
    #[serde(default = "default_content_type")]
    content_type: String,
}

/// Default for [`Document::character_set`] — matches [`Document::new`] and
/// the Encoding Standard's canonical name for `"utf-8"`.
fn default_character_set() -> String {
    "UTF-8".to_string()
}

/// Default for [`Document::content_type`] — matches [`Document::new`] and
/// every `PageSource::load_bytes` HTML variant's hardcoded content type.
fn default_content_type() -> String {
    "text/html".to_string()
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

impl Document {
    pub fn new() -> Self {
        let root_node = Node {
            parent: None,
            children: Vec::new(),
            data: NodeData::Document,
        };
        Self {
            nodes: vec![root_node],
            root: NodeId(0),
            mode: DocumentMode::default(),
            target_id: None,
            shadow_roots: HashMap::new(),
            template_contents: HashMap::new(),
            selection: Selection::default(),
            composition: None,
            fonts: FontFaceSet::new(),
            performance: PerformanceEntries::new(),
            timing_origin: 0.0,
            js_refs: HashMap::new(),
            viewport_meta: None,
            pointer_captures: HashMap::new(),
            dirty_values: HashMap::new(),
            dirty_checkedness: HashMap::new(),
            design_mode: false,
            character_set: default_character_set(),
            content_type: default_content_type(),
        }
    }

    /// Current value of `document.designMode` (HTML LS §6.6.3).
    pub fn design_mode(&self) -> bool {
        self.design_mode
    }

    /// Set `document.designMode`. Driven by the JS shim's setter.
    pub fn set_design_mode(&mut self, enabled: bool) {
        self.design_mode = enabled;
    }

    /// `document.characterSet`/`charset`/`inputEncoding` (DOM §7.3, BUG-358).
    pub fn character_set(&self) -> &str {
        &self.character_set
    }

    /// Set the document's encoding name. Called once by the shell right after
    /// `lumen_encoding::detect()` runs.
    pub fn set_character_set(&mut self, character_set: String) {
        self.character_set = character_set;
    }

    /// `document.contentType` (DOM §4.5, BUG-358).
    pub fn content_type(&self) -> &str {
        &self.content_type
    }

    /// Set the document's MIME type. Called once by the shell from the
    /// `content_type` hint already passed into `parse_and_layout`.
    pub fn set_content_type(&mut self, content_type: String) {
        self.content_type = content_type;
    }

    pub fn root(&self) -> NodeId {
        self.root
    }

    /// Текущий парсинг-режим. Tree builder выставляет его при
    /// обработке DOCTYPE (или его отсутствии в конце потока) — для
    /// программно созданных документов и Document::new()-результата по
    /// умолчанию NoQuirks.
    pub fn mode(&self) -> DocumentMode {
        self.mode
    }

    /// Установить режим. Использует tree builder при инициализации
    /// документа — пользовательский код вызывает редко.
    pub fn set_mode(&mut self, mode: DocumentMode) {
        self.mode = mode;
    }

    /// Parsed `<meta name="viewport">` descriptor, if the page declared one.
    pub fn viewport_meta(&self) -> Option<&ViewportMeta> {
        self.viewport_meta.as_ref()
    }

    /// Set the viewport meta descriptor. Called by the HTML parser when it
    /// encounters `<meta name="viewport" content="…">`.
    pub fn set_viewport_meta(&mut self, meta: ViewportMeta) {
        self.viewport_meta = Some(meta);
    }

    /// Current selection. The shell updates this on mouse events; JS reads it
    /// via `window.getSelection()`.
    pub fn get_selection(&self) -> &Selection {
        &self.selection
    }

    /// Replace the current selection.
    pub fn set_selection(&mut self, sel: Selection) {
        self.selection = sel;
    }

    /// Clear the selection.
    pub fn clear_selection(&mut self) {
        self.selection.clear();
    }

    /// The control's **current** value (HTML LS §4.10.5.5 «value»): the dirty
    /// value if the control has one, otherwise its default value — the `value`
    /// content attribute for `<input>`, the child text for `<textarea>`
    /// (§4.10.11 «text content model»).
    ///
    /// This is what layout paints, what form submission collects and what
    /// constraint validation checks. Non-control elements simply have no
    /// dirty value and no `value` attribute, so they come back as `""`.
    pub fn control_value(&self, id: NodeId) -> Cow<'_, str> {
        if let Some(v) = self.dirty_values.get(&id) {
            return Cow::Borrowed(v.as_str());
        }
        let node = self.get(id);
        if node
            .element_name()
            .is_some_and(|n| n.local.eq_ignore_ascii_case("textarea"))
        {
            return Cow::Owned(dom_text_content(self, id));
        }
        Cow::Borrowed(node.get_attr("value").unwrap_or(""))
    }

    /// The dirty value alone, `None` when the control still shows its default.
    ///
    /// Presence here *is* the dirty value flag — use it when the distinction
    /// matters (e.g. deciding whether a `value=` attribute write is still
    /// allowed to change what the field shows).
    pub fn dirty_value(&self, id: NodeId) -> Option<&str> {
        self.dirty_values.get(&id).map(String::as_str)
    }

    /// Set the control's value and raise its dirty value flag — the single
    /// write path for typing, `el.value = …`, pickers and the driver.
    ///
    /// The `value` content attribute is deliberately left untouched: the spec
    /// forbids reflecting the IDL value into it, and it must keep holding the
    /// default value for `defaultValue`/`form.reset()`.
    pub fn set_control_value(&mut self, id: NodeId, value: impl Into<String>) {
        self.dirty_values.insert(id, value.into());
    }

    /// Drop the control's dirty value, so it falls back to its default —
    /// what `form.reset()` does to every control it owns (HTML LS §4.10.21.3).
    pub fn clear_control_value(&mut self, id: NodeId) {
        self.dirty_values.remove(&id);
    }

    /// The control's **current** checkedness (HTML LS §4.10.5.5): the dirty
    /// checkedness if the control has one, otherwise the default derived from
    /// the `checked` content attribute.
    ///
    /// This is what `:checked`/`:indeterminate` match, what layout paints and
    /// what form submission and constraint validation read. A non-checkbox/
    /// radio element simply has no dirty checkedness and no `checked`
    /// attribute, so it comes back `false` (BUG-444).
    pub fn control_checked(&self, id: NodeId) -> bool {
        if let Some(v) = self.dirty_checkedness.get(&id) {
            return *v;
        }
        self.get(id).get_attr("checked").is_some()
    }

    /// The dirty checkedness alone, `None` when the control still tracks the
    /// `checked` attribute.
    pub fn dirty_checked(&self, id: NodeId) -> Option<bool> {
        self.dirty_checkedness.get(&id).copied()
    }

    /// Set the control's checkedness and raise its dirty checkedness flag —
    /// the single write path for a checkbox/radio click and `el.checked = …`.
    ///
    /// The `checked` content attribute is deliberately left untouched: it
    /// stays the default that `defaultChecked`/`form.reset()` restore.
    pub fn set_control_checked(&mut self, id: NodeId, checked: bool) {
        self.dirty_checkedness.insert(id, checked);
    }

    /// Drop the control's dirty checkedness, so it falls back to the
    /// `checked` attribute — what `form.reset()` does to every checkbox/radio
    /// it owns (HTML LS §4.10.21.3).
    pub fn clear_control_checked(&mut self, id: NodeId) {
        self.dirty_checkedness.remove(&id);
    }

    /// Текущий target — id из URL fragment (без ведущего `#`), к которому
    /// привязан `:target` pseudo-class (CSS Selectors L4 §9.6, HTML LS
    /// §7.10.6 «the indicated part of the document»). `None`, если URL без
    /// fragment-а либо fragment пустой / не указывает на существующий
    /// element с этим id. Сравнение `:target` matcher-а case-sensitive
    /// (HTML id attribute case-sensitive per HTML LS §3.2.6).
    ///
    /// Phase 0: значение здесь не выставляется автоматически — это shell-
    /// интеграция (P3): при загрузке URL парсить fragment и звать
    /// [`Document::set_target`] до style cascade, чтобы matcher имел
    /// корректное значение к моменту layout.
    pub fn target(&self) -> Option<&str> {
        self.target_id.as_deref()
    }

    /// Установить current target (id без `#`). `None` — нет fragment-а в URL.
    /// Caller отвечает за rerun style cascade: пересчёт `:target` matcher-а
    /// не вызывается отсюда.
    pub fn set_target<S: Into<String>>(&mut self, id: Option<S>) {
        self.target_id = id.map(Into::into).filter(|s| !s.is_empty());
    }

    /// Attach a shadow root to `host` and return its `NodeId`.
    ///
    /// The shadow root is allocated in the arena but is **not** a DOM child of
    /// `host`. Children appended to the shadow root form the shadow tree.
    /// Calling twice on the same host replaces the old shadow root (old root
    /// remains in the arena as an orphan — no automatic cleanup in Phase 0).
    ///
    /// Shadow DOM spec §4.2 «Attaching a shadow root».
    pub fn attach_shadow(&mut self, host: NodeId, mode: ShadowRootMode) -> NodeId {
        let sr = self.alloc(NodeData::ShadowRoot { mode });
        self.shadow_roots.insert(host, sr);
        sr
    }

    /// Return the shadow root attached to `host`, or `None` if not a shadow host.
    pub fn shadow_root_of(&self, host: NodeId) -> Option<NodeId> {
        self.shadow_roots.get(&host).copied()
    }

    /// Whether `id` is a shadow host (has an attached shadow root).
    pub fn is_shadow_host(&self, id: NodeId) -> bool {
        self.shadow_roots.contains_key(&id)
    }

    pub fn get(&self, id: NodeId) -> &Node {
        &self.nodes[id.index()]
    }

    pub fn get_mut(&mut self, id: NodeId) -> &mut Node {
        &mut self.nodes[id.index()]
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.len() <= 1
    }

    /// HTML5 §4.2.3 — найти первый `<base href="...">` в документе и
    /// вернуть значение атрибута `href`. Используется для resolve
    /// относительных URL (`<a>`, `<img>`, `<link>`, `<script>`). Если
    /// нет `<base>` или нет атрибута href — `None`.
    ///
    /// Поиск в pre-order обходе (depth-first, элементы по порядку
    /// исходного HTML). Имена тегов и атрибутов в HTML lowercase'нуты
    /// парсером.
    pub fn base_href(&self) -> Option<&str> {
        self.find_first_element(|node| {
            node.element_name()
                .map(|n| n.local == "base")
                .unwrap_or(false)
        })
        .and_then(|n| n.get_attr("href"))
    }

    /// Returns the document's root element `NodeId` — the `<html>` element that is
    /// a direct child of the document node. This is DOM's `document.documentElement`,
    /// distinct from `root()` which is the `Document` node itself (`NodeType.DOCUMENT_NODE`).
    /// Returns `None` for documents that have no `<html>` child (e.g. a document under
    /// construction before the parser has inserted the root element).
    pub fn document_element(&self) -> Option<NodeId> {
        self.get(self.root).children.iter().copied().find(|&c| {
            matches!(&self.get(c).data, NodeData::Element { name, .. } if name.local == "html")
        })
    }

    /// Returns the `<body>` element's `NodeId`, walking root → `<html>` → `<body>`.
    /// Returns `None` for documents that have no `<html>` or no `<body>` child.
    pub fn body(&self) -> Option<NodeId> {
        let html = self.document_element()?;
        self.get(html).children.iter().copied().find(|&c| {
            matches!(&self.get(c).data, NodeData::Element { name, .. } if name.local == "body")
        })
    }

    /// Найти первый элемент, удовлетворяющий предикату. Pre-order обход
    /// от root. Используется для `base_href` и подобных «глобальных»
    /// HTML-помощников.
    pub fn find_first_element(&self, predicate: impl Fn(&Node) -> bool) -> Option<&Node> {
        let mut stack: Vec<NodeId> = vec![self.root];
        while let Some(id) = stack.pop() {
            let node = self.get(id);
            if matches!(node.data, NodeData::Element { .. }) && predicate(node) {
                return Some(node);
            }
            // Push children в обратном порядке, чтобы pop возвращал в
            // прямом source-order.
            for &child in node.children.iter().rev() {
                stack.push(child);
            }
        }
        None
    }

    /// Find a node by its `id` attribute (case-sensitive, per HTML spec).
    ///
    /// Returns the `NodeId` of the first element with matching `id`, or `None` if not found.
    /// Used by accessibility tree and ARIA relationship attributes (aria-labelledby, aria-controls, etc.)
    /// to resolve references.
    pub fn find_by_id(&self, id: &str) -> Option<NodeId> {
        let mut stack: Vec<NodeId> = vec![self.root];
        while let Some(node_id) = stack.pop() {
            let node = self.get(node_id);
            if matches!(node.data, NodeData::Element { .. })
                && node.get_attr("id").is_some_and(|attr_id| attr_id == id)
            {
                return Some(node_id);
            }
            // Push children в обратном порядке для source-order traversal
            for &child in node.children.iter().rev() {
                stack.push(child);
            }
        }
        None
    }

    fn alloc(&mut self, data: NodeData) -> NodeId {
        let id = NodeId(self.nodes.len() as u32);
        self.nodes.push(Node {
            parent: None,
            children: Vec::new(),
            data,
        });
        id
    }

    /// Number of nodes currently allocated in this document's arena (including the root).
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Create an element unconditionally. Used by the HTML parser — does **not** enforce
    /// [`MAX_DOM_NODES`]. JS-driven mutations should use [`try_create_element`][Self::try_create_element].
    pub fn create_element(&mut self, name: QualName) -> NodeId {
        self.alloc(NodeData::Element {
            name,
            attrs: Vec::new(),
        })
    }

    /// Create an element, returning `Err(`[`NodeLimitExceeded`]`)` if the arena already
    /// holds [`MAX_DOM_NODES`] or more nodes.
    ///
    /// Called by the `_lumen_create_element` JS binding so that JS-driven DOM mutations
    /// cannot grow the tree beyond the safety limit.
    pub fn try_create_element(&mut self, name: QualName) -> Result<NodeId, NodeLimitExceeded> {
        if self.nodes.len() >= MAX_DOM_NODES {
            return Err(NodeLimitExceeded);
        }
        Ok(self.alloc(NodeData::Element {
            name,
            attrs: Vec::new(),
        }))
    }

    /// Create a text node unconditionally. Used by the HTML parser — does **not**
    /// enforce [`MAX_DOM_NODES`]. JS-driven mutations should use
    /// [`try_create_text`][Self::try_create_text].
    pub fn create_text(&mut self, content: impl Into<String>) -> NodeId {
        self.alloc(NodeData::Text(content.into()))
    }

    /// Create a text node, returning `Err(`[`NodeLimitExceeded`]`)` if the arena already
    /// holds [`MAX_DOM_NODES`] or more nodes.
    ///
    /// Called by the `_lumen_create_text_node` JS binding so that JS-driven DOM mutations
    /// cannot grow the tree beyond the safety limit (BUG-418: unlike `createElement`,
    /// this path was previously ungated entirely).
    pub fn try_create_text(&mut self, content: impl Into<String>) -> Result<NodeId, NodeLimitExceeded> {
        if self.nodes.len() >= MAX_DOM_NODES {
            return Err(NodeLimitExceeded);
        }
        Ok(self.alloc(NodeData::Text(content.into())))
    }

    /// Create a comment node unconditionally. Used by the HTML parser — does **not**
    /// enforce [`MAX_DOM_NODES`]. JS-driven mutations should use
    /// [`try_create_comment`][Self::try_create_comment].
    pub fn create_comment(&mut self, content: impl Into<String>) -> NodeId {
        self.alloc(NodeData::Comment(content.into()))
    }

    /// Create a comment node, returning `Err(`[`NodeLimitExceeded`]`)` if the arena already
    /// holds [`MAX_DOM_NODES`] or more nodes.
    ///
    /// Called by the `_lumen_create_comment` JS binding so that JS-driven DOM mutations
    /// cannot grow the tree beyond the safety limit (BUG-418: unlike `createElement`,
    /// this path was previously ungated entirely).
    pub fn try_create_comment(&mut self, content: impl Into<String>) -> Result<NodeId, NodeLimitExceeded> {
        if self.nodes.len() >= MAX_DOM_NODES {
            return Err(NodeLimitExceeded);
        }
        Ok(self.alloc(NodeData::Comment(content.into())))
    }

    /// Allocate a `DocumentFragment` node in the arena.
    ///
    /// Used by the tree builder to hold `<template>` content. The fragment is
    /// an inert container: it is never a DOM child of any node and is not
    /// rendered. Register it as a template's content via
    /// [`set_template_content`][Self::set_template_content].
    pub fn create_fragment(&mut self) -> NodeId {
        self.alloc(NodeData::DocumentFragment)
    }

    /// Register `fragment` as the content container for `template`.
    ///
    /// Overwrites any previous mapping. Caller must ensure `fragment` was
    /// created with [`create_fragment`][Self::create_fragment].
    pub fn set_template_content(&mut self, template: NodeId, fragment: NodeId) {
        self.template_contents.insert(template, fragment);
    }

    /// Return the content `DocumentFragment` for a `<template>` element, or
    /// `None` if `template` has no associated content (not a template element).
    pub fn template_content(&self, template: NodeId) -> Option<NodeId> {
        self.template_contents.get(&template).copied()
    }

    pub fn create_doctype(
        &mut self,
        name: impl Into<String>,
        public_id: impl Into<String>,
        system_id: impl Into<String>,
    ) -> NodeId {
        self.alloc(NodeData::Doctype {
            name: name.into(),
            public_id: public_id.into(),
            system_id: system_id.into(),
        })
    }

    /// DEVX-8a: true if `candidate` is `node` itself or one of `node`'s
    /// ancestors, walking up `parent` links. Used to guard `append_child` /
    /// `insert_before` / `insert_after` against creating a DOM cycle (moving a
    /// node under one of its own descendants).
    fn is_self_or_ancestor(&self, candidate: NodeId, node: NodeId) -> bool {
        let mut cur = Some(node);
        while let Some(n) = cur {
            if n == candidate {
                return true;
            }
            cur = self.nodes[n.index()].parent;
        }
        false
    }

    /// Append `child` as the last child of `parent`. If `child` already has a parent, it is detached first.
    pub fn append_child(&mut self, parent: NodeId, child: NodeId) {
        debug_assert!(parent != child, "cannot append a node to itself");
        debug_assert!(
            !self.is_self_or_ancestor(child, parent),
            "DEVX-8a: append_child(parent={parent:?}, child={child:?}) would create a DOM cycle: \
             child is already an ancestor of parent"
        );
        self.detach(child);
        self.nodes[child.index()].parent = Some(parent);
        self.nodes[parent.index()].children.push(child);
    }

    /// Insert `new_node` immediately after `reference` in their shared parent.
    ///
    /// If `reference` has no parent, `new_node` is left without a parent (no-op
    /// other than detaching any previous parent of `new_node`). If `reference` is
    /// the last child, `new_node` is appended.
    pub fn insert_after(&mut self, reference: NodeId, new_node: NodeId) {
        let parent = self.nodes[reference.index()].parent;
        if let Some(p) = parent {
            debug_assert!(
                !self.is_self_or_ancestor(new_node, p),
                "DEVX-8a: insert_after(reference={reference:?}, new_node={new_node:?}) would create \
                 a DOM cycle: new_node is already an ancestor of reference's parent"
            );
        }
        self.detach(new_node);
        let Some(parent) = parent else { return };
        let siblings = &mut self.nodes[parent.index()].children;
        let pos = siblings.iter().position(|&n| n == reference).unwrap_or(siblings.len() - 1);
        siblings.insert(pos + 1, new_node);
        self.nodes[new_node.index()].parent = Some(parent);
    }

    /// Remove `node` from its current parent. The node itself stays in the arena and can be re-attached.
    ///
    /// **GC hook (P3 integration):** After detaching a node, call
    /// [`Document::dead_node_ids`] to check whether the node became collectable
    /// (detached + zero JS wrappers). P3 wires this into the JS finalizer cycle:
    /// the QuickJS finalizer decrements the ref count via [`Document::release_js_ref`];
    /// the shell's idle GC tick drains [`Document::dead_node_ids`] and purges the slots.
    pub fn detach(&mut self, node: NodeId) {
        let parent = self.nodes[node.index()].parent.take();
        if let Some(parent) = parent {
            let siblings = &mut self.nodes[parent.index()].children;
            if let Some(pos) = siblings.iter().position(|&n| n == node) {
                siblings.remove(pos);
            }
        }
    }

    /// Insert `new_node` immediately before `reference` in `reference`'s parent.
    ///
    /// If `reference` has no parent, `new_node` is left without a parent. If
    /// `new_node` already has a parent it is detached first.
    pub fn insert_before(&mut self, new_node: NodeId, reference: NodeId) {
        let parent = self.nodes[reference.index()].parent;
        if let Some(p) = parent {
            debug_assert!(
                !self.is_self_or_ancestor(new_node, p),
                "DEVX-8a: insert_before(new_node={new_node:?}, reference={reference:?}) would create \
                 a DOM cycle: new_node is already an ancestor of reference's parent"
            );
        }
        self.detach(new_node);
        let Some(parent) = parent else { return };
        let siblings = &mut self.nodes[parent.index()].children;
        let pos = siblings
            .iter()
            .position(|&n| n == reference)
            .unwrap_or(siblings.len());
        siblings.insert(pos, new_node);
        self.nodes[new_node.index()].parent = Some(parent);
    }

    /// Deep-clone `node` and (if `deep`) all its descendants.
    ///
    /// Returns the `NodeId` of the new root clone. The clone has no parent.
    /// Does not copy template content maps or shadow roots — those require
    /// explicit re-attachment by the caller.
    pub fn deep_clone(&mut self, node: NodeId, deep: bool) -> NodeId {
        let data = self.nodes[node.index()].data.clone();
        let clone = self.alloc(data);
        if deep {
            let children: Vec<NodeId> = self.nodes[node.index()].children.clone();
            for child in children {
                let child_clone = self.deep_clone(child, true);
                self.nodes[clone.index()].children.push(child_clone);
                self.nodes[child_clone.index()].parent = Some(clone);
            }
        }
        clone
    }

    // ── GC integration: JS wrapper reference tracking ─────────────────────────

    /// Increment the JS wrapper reference count for `node_id`.
    ///
    /// Called by the JS runtime when it creates a wrapper object for this DOM
    /// node (e.g. the first time JS accesses `document.getElementById(…)`).
    /// Returns the new reference count.
    ///
    /// **P3 integration point:** invoke from `lumen-js` when allocating a
    /// QuickJS object whose `_nid` property is set for the first time.
    pub fn acquire_js_ref(&mut self, node_id: NodeId) -> u32 {
        let count = self.js_refs.entry(node_id).or_insert(0);
        *count += 1;
        *count
    }

    /// Decrement the JS wrapper reference count for `node_id`.
    ///
    /// Called by the QuickJS finalizer when the last JS reference to a wrapper
    /// object is collected. Returns the remaining reference count (0 means no
    /// live JS wrappers remain).
    ///
    /// When the count drops to zero **and** the node is detached from the
    /// document tree, it becomes eligible for collection — visible via
    /// [`Document::dead_node_ids`].
    ///
    /// **P3 integration point:** invoke from the `rquickjs` class finalizer
    /// registered for DOM wrapper objects (see `lumen-js::dom`).
    pub fn release_js_ref(&mut self, node_id: NodeId) -> u32 {
        let Some(count) = self.js_refs.get_mut(&node_id) else {
            return 0;
        };
        if *count <= 1 {
            self.js_refs.remove(&node_id);
            return 0;
        }
        *count -= 1;
        *count
    }

    /// Returns the number of live JS wrapper objects currently referencing `node_id`.
    ///
    /// Zero means no JS object holds a reference to this node. Combined with
    /// [`Document::is_detached`], this determines whether a node is collectable.
    pub fn js_ref_count(&self, node_id: NodeId) -> u32 {
        self.js_refs.get(&node_id).copied().unwrap_or(0)
    }

    /// Returns `true` if `node_id` is not reachable from the document tree.
    ///
    /// A node is detached when all of the following hold:
    /// - it is not the document root
    /// - its `parent` field is `None` (removed from the tree or never inserted)
    /// - it is not a shadow root (would have `parent == None` but be in `shadow_roots`)
    /// - it is not a `<template>` content fragment (same reason)
    ///
    /// Detached nodes with zero JS refs are collectable (see [`Document::dead_node_ids`]).
    pub fn is_detached(&self, node_id: NodeId) -> bool {
        if node_id == self.root {
            return false;
        }
        if self.nodes[node_id.index()].parent.is_some() {
            return false;
        }
        if self.shadow_roots.values().any(|&sr| sr == node_id) {
            return false;
        }
        if self.template_contents.values().any(|&tc| tc == node_id) {
            return false;
        }
        true
    }

    /// Returns the IDs of all nodes that are safe to collect from the arena.
    ///
    /// A node is "dead" when it is both detached from the document tree
    /// (see [`Document::is_detached`]) **and** has no live JS wrappers
    /// (see [`Document::js_ref_count`]).
    ///
    /// **Phase 2 contract:** this method identifies collectable nodes but does
    /// not remove them from the arena. The arena remains append-only until Phase 3
    /// adds free-list compaction. P3's idle GC tick should call this method
    /// periodically and drop any external resources associated with the returned
    /// NodeIds (e.g. image decode handles, layout boxes).
    pub fn dead_node_ids(&self) -> Vec<NodeId> {
        // Build a set of "anchored orphan" nodes: shadow roots and template
        // content fragments have parent==None but must not be collected.
        let anchored: HashSet<NodeId> = self
            .shadow_roots
            .values()
            .chain(self.template_contents.values())
            .copied()
            .collect();

        self.nodes
            .iter()
            .enumerate()
            .filter_map(|(i, node)| {
                let id = NodeId::from_index(i);
                if id == self.root {
                    return None;
                }
                if node.parent.is_some() {
                    return None;
                }
                if anchored.contains(&id) {
                    return None;
                }
                if self.js_refs.get(&id).copied().unwrap_or(0) > 0 {
                    return None;
                }
                Some(id)
            })
            .collect()
    }

    // ── T3 hibernation snapshot (ADR-008) ─────────────────────────────────────

    /// Serialise the entire document to a compact binary blob (bincode).
    ///
    /// Used for **T3 hibernation**: when a tab is suspended, the shell calls
    /// `to_bytes()`, stores the blob on disk, and frees the in-memory tree.
    /// On restore the shell calls [`from_bytes`] to reconstruct the tree
    /// without re-parsing HTML. The blob is self-contained — no pointer fixups
    /// are needed because every node reference is a `NodeId(u32)` offset.
    pub fn to_bytes(&self) -> Result<Vec<u8>, DomSnapshotError> {
        bincode::serialize(self).map_err(DomSnapshotError::Encode)
    }

    /// Deserialise a document from a binary blob produced by [`to_bytes`].
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DomSnapshotError> {
        bincode::deserialize(bytes).map_err(DomSnapshotError::Decode)
    }
}

impl fmt::Display for Document {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_tree(self, self.root, 0, f)
    }
}

fn write_tree(doc: &Document, id: NodeId, depth: usize, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    let node = doc.get(id);
    for _ in 0..depth {
        f.write_str("  ")?;
    }
    match &node.data {
        NodeData::Document => writeln!(f, "#document")?,
        NodeData::Doctype { name, .. } => writeln!(f, "<!DOCTYPE {name}>")?,
        NodeData::ShadowRoot { mode } => writeln!(f, "#shadow-root ({mode})")?,
        NodeData::DocumentFragment => writeln!(f, "#document-fragment")?,
        NodeData::Element { name, attrs } => {
            write!(f, "<{}", name.local)?;
            for a in attrs {
                write!(f, " {}=\"{}\"", a.name.local, a.value)?;
            }
            writeln!(f, ">")?;
        }
        NodeData::Text(s) => writeln!(f, "\"{}\"", s.replace('\n', "\\n"))?,
        NodeData::Comment(s) => writeln!(f, "<!--{s}-->")?,
    }
    for &child in &node.children {
        write_tree(doc, child, depth + 1, f)?;
    }
    // Shadow roots are not DOM children — print them after light-tree children.
    if let Some(sr) = doc.shadow_root_of(id) {
        write_tree(doc, sr, depth + 1, f)?;
    }
    // Template content fragments are not DOM children — print inline for debugging.
    if let Some(frag) = doc.template_content(id) {
        write_tree(doc, frag, depth + 1, f)?;
    }
    Ok(())
}

/// Walk up the DOM from `node` and return the first ancestor `<dialog>` element.
///
/// Returns `None` if no dialog ancestor exists. Used by `<form method="dialog">`
/// processing: when a form inside a dialog is submitted, this finds the dialog to close.
pub fn find_ancestor_dialog(doc: &Document, mut node: NodeId) -> Option<NodeId> {
    while let Some(parent) = doc.get(node).parent {
        if doc
            .get(parent)
            .element_name()
            .map(|q| q.local.eq_ignore_ascii_case("dialog"))
            .unwrap_or(false)
        {
            return Some(parent);
        }
        node = parent;
    }
    None
}

/// True when `node` carries `contenteditable=""` or `contenteditable="true"`.
///
/// Per HTML LS §3.1.11 the attribute value is ASCII case-insensitive.  A missing
/// attribute or `contenteditable="false"` returns `false`.
pub fn node_is_contenteditable(doc: &Document, node: NodeId) -> bool {
    if let NodeData::Element { attrs, .. } = &doc.get(node).data {
        for attr in attrs {
            if attr.name.local.eq_ignore_ascii_case("contenteditable") {
                return !attr.value.eq_ignore_ascii_case("false");
            }
        }
    }
    false
}

/// Walk up the tree from `node` (inclusive) and return the nearest element
/// with `contenteditable` set to a truthy value — the *editing host*.
///
/// When no ancestor carries an explicit `contenteditable` and
/// `document.designMode` (HTML LS §6.6.3) is enabled, falls back to the
/// document's `<body>` — design mode makes the whole document an editing
/// host without any element needing the attribute.
///
/// Returns `None` when neither applies.
pub fn find_editing_host(doc: &Document, node: NodeId) -> Option<NodeId> {
    let mut cur = node;
    loop {
        if node_is_contenteditable(doc, cur) {
            return Some(cur);
        }
        match doc.get(cur).parent {
            Some(p) => cur = p,
            None => break,
        }
    }
    if doc.design_mode() { doc.body() } else { None }
}

/// Return `true` when `node` is draggable by default HTML5 rules (HTML LS §9.3.3).
///
/// An element is draggable when:
/// - it has an explicit `draggable="true"` attribute, **or**
/// - it is an `<a>` element with an `href` attribute (user-agent default), **or**
/// - it is an `<img>` element (user-agent default).
///
/// A `draggable="false"` attribute overrides these defaults.
pub fn is_element_draggable(doc: &Document, node: NodeId) -> bool {
    let n = doc.get(node);
    let NodeData::Element { name, attrs } = &n.data else { return false };
    // Explicit draggable attribute overrides everything.
    for attr in attrs {
        if attr.name.local.eq_ignore_ascii_case("draggable") {
            return !attr.value.eq_ignore_ascii_case("false");
        }
    }
    // User-agent defaults: <a href> and <img> are draggable by default.
    let tag: &str = name.local.as_ref();
    if tag.eq_ignore_ascii_case("a") {
        return attrs.iter().any(|a| a.name.local.eq_ignore_ascii_case("href"));
    }
    tag.eq_ignore_ascii_case("img")
}

/// Set pointer capture for `pointer_id` to `node` (W3C Pointer Events L3 §4.1).
///
/// Returns `true` when a prior capture was replaced by a new target.
/// Shell must fire `gotpointercapture` on `node` after calling this.
pub fn set_pointer_capture(doc: &mut Document, node: NodeId, pointer_id: u32) -> bool {
    doc.pointer_captures.insert(pointer_id, node).is_some()
}

/// Release pointer capture for `pointer_id` from `node`.
///
/// No-op when `node` does not hold the capture for `pointer_id`.
/// Shell must fire `lostpointercapture` on `node` after calling this.
pub fn release_pointer_capture(doc: &mut Document, node: NodeId, pointer_id: u32) {
    if doc.pointer_captures.get(&pointer_id) == Some(&node) {
        doc.pointer_captures.remove(&pointer_id);
    }
}

/// Returns `true` if `node` currently holds pointer capture for `pointer_id`.
pub fn has_pointer_capture(doc: &Document, node: NodeId, pointer_id: u32) -> bool {
    doc.pointer_captures.get(&pointer_id) == Some(&node)
}

/// Returns the element that holds pointer capture for `pointer_id`, if any.
///
/// Shell uses this to redirect pointer events to the capture target instead of
/// the element returned by hit testing.
pub fn pointer_capture_target(doc: &Document, pointer_id: u32) -> Option<NodeId> {
    doc.pointer_captures.get(&pointer_id).copied()
}

/// Collects all text content of an element (all Text descendants in DOM order).
fn dom_text_content(doc: &Document, node: NodeId) -> String {
    let mut out = String::new();
    dom_collect_text(doc, node, &mut out);
    out
}

fn dom_collect_text(doc: &Document, node: NodeId, out: &mut String) {
    for &child in &doc.get(node).children {
        match &doc.get(child).data {
            NodeData::Text(s) => out.push_str(s),
            NodeData::Element { .. } => dom_collect_text(doc, child, out),
            _ => {}
        }
    }
}

/// Parses an HTML5 valid floating-point number (§2.5.5).
/// Rejects leading `+`, NaN, and ±∞.
fn parse_html_float(s: &str) -> Option<f64> {
    let s = s.trim();
    if s.is_empty() || s.starts_with('+') {
        return None;
    }
    let v: f64 = s.parse().ok()?;
    if v.is_finite() { Some(v) } else { None }
}

/// Basic email syntax check (HTML5 §4.10.5.1.5 «valid e-mail address»).
/// Phase 0: non-empty local-part + `@` + domain with at least one `.`.
fn is_valid_email_dom(value: &str) -> bool {
    let value = value.trim();
    let Some(at_pos) = value.rfind('@') else { return false; };
    let local = &value[..at_pos];
    let domain = &value[at_pos + 1..];
    if local.is_empty() || domain.is_empty() {
        return false;
    }
    let parts: Vec<&str> = domain.split('.').collect();
    parts.len() >= 2 && parts.iter().all(|p| !p.is_empty())
}

/// Basic URL syntax check (HTML5 §4.10.5.1.15 «valid URL»).
/// Phase 0: presence of `<scheme>://` or known schemeless URIs.
fn is_valid_url_dom(value: &str) -> bool {
    let value = value.trim();
    if let Some(pos) = value.find("://") {
        let scheme = &value[..pos];
        return !scheme.is_empty()
            && scheme.chars().all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.');
    }
    if let Some(pos) = value.find(':') {
        let scheme = &value[..pos];
        if matches!(scheme, "data" | "mailto" | "tel") {
            return !value[pos + 1..].is_empty();
        }
    }
    false
}

/// Информация об якорной ссылке (`<a href>`), найденной в документе.
pub struct AnchorInfo {
    /// Значение атрибута `href`.
    pub href: String,
}

fn collect_anchors(doc: &Document, id: NodeId, out: &mut Vec<AnchorInfo>) {
    let node = doc.get(id);
    if node
        .element_name()
        .map(|n| n.local.eq_ignore_ascii_case("a"))
        .unwrap_or(false)
        && let Some(href) = node.get_attr("href").filter(|h| !h.is_empty())
    {
        out.push(AnchorInfo {
            href: href.to_string(),
        });
    }
    for &child in &node.children.clone() {
        collect_anchors(doc, child, out);
    }
}

// ──────── Shadow DOM: composed (flat) tree ────────

/// Pre-computed composed tree (flat tree) for Shadow DOM layout traversal.
///
/// Shadow DOM spec §8.2: the flat tree replaces the DOM tree for rendering.
/// Shadow hosts are replaced by their shadow subtrees and `<slot>` elements
/// are replaced by their assigned light-tree nodes.
///
/// For documents without Shadow DOM `overrides` is empty, so every lookup
/// falls through to DOM children — zero allocation overhead.
#[derive(Debug, Default)]
pub struct FlatTree {
    /// Nodes whose composed-tree children differ from their DOM children.
    overrides: HashMap<NodeId, Vec<NodeId>>,
}

impl FlatTree {
    /// Composed-tree children of `id`.
    ///
    /// Returns DOM children when no shadow override exists (fast path for
    /// ordinary elements in non-shadow documents).
    pub fn children_of<'a>(&'a self, doc: &'a Document, id: NodeId) -> &'a [NodeId] {
        // BUG-341 S26: whether this document has *any* composed-tree override is
        // a fact about the document, not about `id`, and for every page without
        // Shadow DOM the answer is "no". Without this line each call still hashed
        // `id` to find that out — one SipHash per node per traversal, and the
        // cascade, box build and a11y tree all traverse per pass. Measured at
        // ~13 ns a lookup over the chrome document's 828 elements.
        if self.overrides.is_empty() {
            return doc.get(id).children.as_slice();
        }
        self.overrides
            .get(&id)
            .map(Vec::as_slice)
            .unwrap_or_else(|| doc.get(id).children.as_slice())
    }

    /// Whether the composed tree *is* the DOM tree — no shadow host or slot
    /// moves a node away from its DOM parent.
    ///
    /// BUG-341 S27: a traversal that wants to ask "does this subtree contain
    /// any of these nodes" cheaply does it by walking each of those nodes up to
    /// the root, and `Node::parent` is the DOM parent. That answer is the
    /// composed-tree answer exactly when this holds; a document with shadow
    /// trees keeps the pre-S27 traversal instead of growing a composed-tree
    /// parent index for a case Lumen's own chrome does not have.
    pub fn is_plain(&self) -> bool {
        self.overrides.is_empty()
    }
}

/// Build the composed (flat) tree for the document.
///
/// Shadow DOM spec §8.2. Layout calls this once before `build_box` so that
/// the tree traversal follows shadow boundaries without per-node branching.
///
/// Fast path: if the document has no shadow hosts, returns an empty `FlatTree`
/// (every `children_of` call falls through to DOM children).
#[allow(clippy::expect_used)]  // унаследовано, docs/lint-policy.md §10
pub fn build_flat_tree(doc: &Document) -> FlatTree {
    if doc.shadow_roots.is_empty() {
        return FlatTree::default();
    }

    let mut overrides: HashMap<NodeId, Vec<NodeId>> = HashMap::new();

    for i in 0..doc.len() {
        let id = NodeId::from_index(i);
        if !doc.is_shadow_host(id) {
            continue;
        }
        let sr = doc.shadow_root_of(id).expect("shadow host has no root");

        // Shadow host's composed children = shadow root's DOM children.
        overrides.insert(id, doc.get(sr).children.clone());

        // Distribute light-tree children into matching <slot> elements.
        let slot_map = compute_slot_assignments(doc, id, sr);
        wire_slot_overrides(doc, sr, &slot_map, &mut overrides);
    }

    FlatTree { overrides }
}

/// Maps each `<slot>` NodeId to its assigned light-tree nodes.
type SlotAssignments = HashMap<NodeId, Vec<NodeId>>;

/// Compute slot assignments for `host`'s shadow tree rooted at `sr`.
///
/// Each light-tree child of `host` whose `slot=""` attribute matches a
/// `<slot name="">` in the shadow tree is assigned to that slot. Unmatched
/// children are dropped (they don't appear in the flat tree).
#[allow(clippy::expect_used)]  // унаследовано, docs/lint-policy.md §10
fn compute_slot_assignments(doc: &Document, host: NodeId, sr: NodeId) -> SlotAssignments {
    let mut slots: Vec<(NodeId, String)> = Vec::new();
    collect_slots(doc, sr, &mut slots);

    let mut map: SlotAssignments = HashMap::new();
    for &(slot_id, _) in &slots {
        map.insert(slot_id, Vec::new());
    }

    for &child in &doc.get(host).children {
        let wanted = doc.get(child).get_attr("slot").unwrap_or("").to_string();
        if let Some(&(slot_id, _)) = slots.iter().find(|(_, name)| *name == wanted) {
            map.get_mut(&slot_id).expect("slot in map").push(child);
        }
        // Children with no matching slot are not rendered in the flat tree.
    }

    map
}

fn collect_slots(doc: &Document, id: NodeId, out: &mut Vec<(NodeId, String)>) {
    if let NodeData::Element { name, .. } = &doc.get(id).data
        && name.local == "slot"
    {
        let slot_name = doc.get(id).get_attr("name").unwrap_or("").to_string();
        out.push((id, slot_name));
    }
    for &child in &doc.get(id).children {
        collect_slots(doc, child, out);
    }
}

/// Override each `<slot>` in the shadow tree with its assigned light-tree nodes.
///
/// A slot with assigned nodes gets an override (composed children = assigned).
/// A slot with no assigned nodes keeps its DOM children as fallback content.
fn wire_slot_overrides(
    doc: &Document,
    id: NodeId,
    slot_map: &SlotAssignments,
    overrides: &mut HashMap<NodeId, Vec<NodeId>>,
) {
    if let NodeData::Element { name, .. } = &doc.get(id).data
        && name.local == "slot"
        && let Some(assigned) = slot_map.get(&id)
        && !assigned.is_empty()
    {
        overrides.insert(id, assigned.clone());
        // Empty assignment → no override; slot's DOM children are the fallback.
    }
    for &child in &doc.get(id).children {
        wire_slot_overrides(doc, child, slot_map, overrides);
    }
}

/// Гейт навигации по sandbox-флагу HTML §7.6.5.
///
/// Если `sandbox` содержит [`SandboxFlags::NAVIGATION`] — навигация
/// из sandboxed-документа заблокирована; функция логирует число
/// заблокированных ссылок и возвращает его.
/// Если флаг не установлен — возвращает 0. В Phase 0 реальной навигации
/// нет; вызов устанавливает инфраструктуру для будущего NavigationRuntime.
pub fn check_navigation_gate(doc: &Document, sandbox: SandboxFlags) -> usize {
    let mut anchors = Vec::new();
    collect_anchors(doc, doc.root(), &mut anchors);
    if anchors.is_empty() {
        return 0;
    }
    if sandbox.contains(SandboxFlags::NAVIGATION) {
        eprintln!(
            "sandbox: заблокировано {} ссылок(и) (sandbox=navigation)",
            anchors.len()
        );
        return anchors.len();
    }
    0
}

// ──────────────────────────────────────────────────────────────────────────────
// iframe sandbox
// ──────────────────────────────────────────────────────────────────────────────

/// Данные элемента-хоста вложенного browsing context — URL содержимого и
/// sandbox-ограничения.
///
/// Хостов два тега: `<iframe>` и обсолетный, но по-прежнему разбираемый
/// `<frame>` (HTML LS §16.3.3, BUG-854). Тип назван по первому из них ради
/// совместимости имён; всё, что относится только к `<iframe>` (`srcdoc`,
/// `sandbox`, `loading`, `fetchpriority`), у `<frame>` просто пусто.
///
/// `is_sandboxed` — `true` если у элемента есть атрибут `sandbox` (даже пустой).
/// `sandbox` содержит распарсенные флаги (пустые = нет ограничений, все = максимум).
pub struct IframeInfo {
    /// `NodeId` самого элемента-хоста — адресат load/error-событий и
    /// будущий ключ сопоставления «элемент ↔ browsing context» (BUG-480).
    pub node: NodeId,
    /// Значение атрибута `src`, если задан.
    pub src: Option<String>,
    /// Inline HTML content from `srcdoc` attribute (HTML spec §4.8.5).
    /// When present, this HTML is used instead of fetching `src`.
    pub srcdoc: Option<String>,
    /// Sandbox-флаги согласно HTML §7.6.5. `SandboxFlags::empty()` если атрибута нет.
    pub sandbox: SandboxFlags,
    /// `true` если у элемента есть атрибут `sandbox` (независимо от значения).
    pub is_sandboxed: bool,
    /// `loading="lazy"` (HTML LS §4.8.5): отложить загрузку sub-документа до
    /// приближения к viewport. Phase 0: sub-документы не загружаются вовсе —
    /// поле является проводкой для Phase 1.
    pub loading_lazy: bool,
    /// `fetchpriority` (HTML LS §2.5.7): нормализованное `"high"`/`"low"`;
    /// `auto`, мусор и отсутствие атрибута → `None`.
    pub fetch_priority: Option<String>,
    /// Значение атрибута `name`, если задан — будущий ключ `window[name]`
    /// для именованного доступа к фреймам (BUG-480).
    pub name: Option<String>,
}

/// Нормализует значение атрибута `fetchpriority` (HTML LS §2.5.7):
/// ASCII-lowercase, допустимы только `"high"` и `"low"`; `auto`/мусор/None → `None`.
fn normalize_fetch_priority(raw: Option<&str>) -> Option<String> {
    let lowered = raw?.trim().to_ascii_lowercase();
    match lowered.as_str() {
        "high" | "low" => Some(lowered),
        _ => None,
    }
}

fn collect_iframes_inner(doc: &Document, id: NodeId, out: &mut Vec<IframeInfo>) {
    let node = doc.get(id);
    // `<frame>` соседствует здесь с `<iframe>`, а не живёт отдельным проходом:
    // вложенный browsing context у них один и тот же (HTML LS §16.3.3 «process
    // the frame attributes» — тот же алгоритм, что у §4.8.5), различаются лишь
    // атрибуты, которых у `<frame>` нет. BUG-854.
    let is_iframe = node
        .element_name()
        .map(|n| n.local.eq_ignore_ascii_case("iframe"))
        .unwrap_or(false);
    let is_frame = node
        .element_name()
        .map(|n| n.local.eq_ignore_ascii_case("frame"))
        .unwrap_or(false);
    if is_iframe || is_frame {
        let src = node.get_attr("src").filter(|s| !s.is_empty()).map(str::to_owned);
        // `srcdoc` объявлен только у `<iframe>`; на `<frame>` это обычный
        // неизвестный атрибут, и читать его как источник — выдумка.
        let srcdoc = node
            .get_attr("srcdoc")
            .filter(|_| is_iframe)
            .filter(|s| !s.is_empty())
            .map(str::to_owned);
        let is_sandboxed = node.get_attr("sandbox").is_some();
        let sandbox = node.sandbox_flags().unwrap_or_else(SandboxFlags::empty);
        let loading_lazy = node
            .get_attr("loading")
            .is_some_and(|v| v.eq_ignore_ascii_case("lazy"));
        let fetch_priority = normalize_fetch_priority(node.get_attr("fetchpriority"));
        let name = node.get_attr("name").filter(|s| !s.is_empty()).map(str::to_owned);
        out.push(IframeInfo { node: id, src, srcdoc, sandbox, is_sandboxed, loading_lazy, fetch_priority, name });
    }
    for &child in &node.children.clone() {
        collect_iframes_inner(doc, child, out);
    }
}

/// Собрать все элементы-хосты вложенных browsing context (`<iframe>` и
/// `<frame>`) документа с их sandbox-ограничениями.
///
/// Каждый такой элемент — один `IframeInfo`. Элементы без атрибута `sandbox`
/// включаются с `is_sandboxed = false` и `sandbox = SandboxFlags::empty()`.
/// Порядок — depth-first обход дерева. `<frame>` попадает сюда откуда угодно,
/// а не только из `<frameset>`: браузеры грузят его и в `<body>` (BUG-854).
pub fn collect_iframes(doc: &Document) -> Vec<IframeInfo> {
    let mut out = Vec::new();
    collect_iframes_inner(doc, doc.root(), &mut out);
    out
}

/// Гейт открытия popup-ов (`window.open()`, `target="_blank"`) по sandbox HTML §7.6.5.
///
/// Возвращает `true` если `sandbox` содержит [`SandboxFlags::AUXILIARY_NAVIGATION`]
/// (т.е. `allow-popups` не указан) — popup запрещён.
/// `false` — popup разрешён (флаг снят или sandbox не активен).
pub fn check_popup_gate(sandbox: SandboxFlags) -> bool {
    if sandbox.contains(SandboxFlags::AUXILIARY_NAVIGATION) {
        eprintln!("sandbox: заблокирован popup (sandbox=auxiliary-navigation, нет allow-popups)");
        return true;
    }
    false
}

// ──────────────────────────────────────────────────────────────────────────────
// contenteditable: Input Events Level 2 (P1 part)
// ──────────────────────────────────────────────────────────────────────────────

// Compile-time gate (ADR-008 §11.4, trk 10B): Document must stay Send + Sync so
// tabs can be moved between threads and T3 hibernation snapshots are safe to hand
// off across thread boundaries. Adding Rc<RefCell<_>> or any other !Send/!Sync
// type to Document's fields breaks this assertion — use NodeId indices instead.
const _: fn() = || {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Document>();
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_document_has_root() {
        let doc = Document::new();
        assert_eq!(doc.len(), 1);
        assert!(matches!(doc.get(doc.root()).data, NodeData::Document));
    }

    #[test]
    fn build_simple_tree() {
        let mut doc = Document::new();
        let html = doc.create_element(QualName::html("html"));
        let body = doc.create_element(QualName::html("body"));
        let h1 = doc.create_element(QualName::html("h1"));
        let text = doc.create_text("Hello");

        doc.append_child(doc.root(), html);
        doc.append_child(html, body);
        doc.append_child(body, h1);
        doc.append_child(h1, text);

        assert_eq!(doc.len(), 5);
        assert_eq!(doc.get(html).children, vec![body]);
        assert_eq!(doc.get(body).children, vec![h1]);
        assert_eq!(doc.get(h1).children, vec![text]);
        assert_eq!(doc.get(text).parent, Some(h1));
    }

    #[test]
    fn detach_removes_from_parent_but_keeps_node() {
        let mut doc = Document::new();
        let a = doc.create_element(QualName::html("a"));
        let b = doc.create_element(QualName::html("b"));
        doc.append_child(doc.root(), a);
        doc.append_child(a, b);

        doc.detach(b);

        assert!(doc.get(a).children.is_empty());
        assert_eq!(doc.get(b).parent, None);
        assert_eq!(doc.len(), 3);
    }

    #[test]
    fn append_moves_existing_node() {
        let mut doc = Document::new();
        let a = doc.create_element(QualName::html("a"));
        let b = doc.create_element(QualName::html("b"));
        let c = doc.create_element(QualName::html("c"));
        doc.append_child(doc.root(), a);
        doc.append_child(doc.root(), b);
        doc.append_child(a, c);

        doc.append_child(b, c);

        assert!(doc.get(a).children.is_empty());
        assert_eq!(doc.get(b).children, vec![c]);
        assert_eq!(doc.get(c).parent, Some(b));
    }

    #[test]
    fn cyrillic_text_node() {
        let mut doc = Document::new();
        let html = doc.create_element(QualName::html("html"));
        let body = doc.create_element(QualName::html("body"));
        let h1 = doc.create_element(QualName::html("h1"));
        let text = doc.create_text("Привет, мир! Ёжик");

        doc.append_child(doc.root(), html);
        doc.append_child(html, body);
        doc.append_child(body, h1);
        doc.append_child(h1, text);

        match &doc.get(text).data {
            NodeData::Text(s) => {
                assert_eq!(s, "Привет, мир! Ёжик");
                // Cyrillic is 2 bytes per char in UTF-8, so byte length must exceed char count.
                assert!(s.len() > s.chars().count());
            }
            _ => panic!("expected text node"),
        }

        let printed = doc.to_string();
        assert!(printed.contains("Привет"));
        assert!(printed.contains("Ёжик"));
    }

    #[test]
    fn cyrillic_attribute_value() {
        let mut doc = Document::new();
        let div = doc.create_element(QualName::html("div"));

        let NodeData::Element { attrs, .. } = &mut doc.get_mut(div).data else {
            unreachable!();
        };
        attrs.push(Attribute {
            name: QualName::html("title"),
            value: "Привет, кириллица".to_string(),
        });

        doc.append_child(doc.root(), div);

        let s = doc.to_string();
        assert!(s.contains("title=\"Привет, кириллица\""));
    }

    #[test]
    fn display_format() {
        let mut doc = Document::new();
        let html = doc.create_element(QualName::html("html"));
        let body = doc.create_element(QualName::html("body"));
        let h1 = doc.create_element(QualName::html("h1"));
        let text = doc.create_text("Hello");

        doc.append_child(doc.root(), html);
        doc.append_child(html, body);
        doc.append_child(body, h1);
        doc.append_child(h1, text);

        let s = doc.to_string();
        assert!(s.contains("#document"));
        assert!(s.contains("<html>"));
        assert!(s.contains("\"Hello\""));
    }

    // ──────── base_href / find_first_element ────────

    fn build_doc_with_base(href: &str) -> Document {
        let mut doc = Document::new();
        let html = doc.create_element(QualName::html("html"));
        let head = doc.create_element(QualName::html("head"));
        let base = doc.create_element(QualName::html("base"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(base).data {
            attrs.push(Attribute {
                name: QualName::html("href"),
                value: href.to_string(),
            });
        }
        doc.append_child(doc.root(), html);
        doc.append_child(html, head);
        doc.append_child(head, base);
        doc
    }

    #[test]
    fn base_href_extracts_attribute() {
        let doc = build_doc_with_base("https://example.com/path/");
        assert_eq!(doc.base_href(), Some("https://example.com/path/"));
    }

    #[test]
    fn base_href_returns_none_without_base() {
        let mut doc = Document::new();
        let html = doc.create_element(QualName::html("html"));
        doc.append_child(doc.root(), html);
        assert_eq!(doc.base_href(), None);
    }

    #[test]
    fn base_href_returns_none_when_base_has_no_href() {
        let mut doc = Document::new();
        let html = doc.create_element(QualName::html("html"));
        let head = doc.create_element(QualName::html("head"));
        let base = doc.create_element(QualName::html("base"));  // без href
        doc.append_child(doc.root(), html);
        doc.append_child(html, head);
        doc.append_child(head, base);
        assert_eq!(doc.base_href(), None);
    }

    #[test]
    fn base_href_finds_first_in_document_order() {
        // Два <base> элемента — берём первый в pre-order.
        let mut doc = Document::new();
        let html = doc.create_element(QualName::html("html"));
        let head = doc.create_element(QualName::html("head"));
        let base1 = doc.create_element(QualName::html("base"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(base1).data {
            attrs.push(Attribute {
                name: QualName::html("href"),
                value: "first".to_string(),
            });
        }
        let base2 = doc.create_element(QualName::html("base"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(base2).data {
            attrs.push(Attribute {
                name: QualName::html("href"),
                value: "second".to_string(),
            });
        }
        doc.append_child(doc.root(), html);
        doc.append_child(html, head);
        doc.append_child(head, base1);
        doc.append_child(head, base2);
        assert_eq!(doc.base_href(), Some("first"));
    }

    #[test]
    fn base_href_case_insensitive_attribute() {
        // HTML парсер lower-case-ит, но если что-то попало в HREF — get_attr
        // должен находить.
        let mut doc = Document::new();
        let html = doc.create_element(QualName::html("html"));
        let base = doc.create_element(QualName::html("base"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(base).data {
            attrs.push(Attribute {
                name: QualName::html("HREF"),
                value: "x.com".to_string(),
            });
        }
        doc.append_child(doc.root(), html);
        doc.append_child(html, base);
        assert_eq!(doc.base_href(), Some("x.com"));
    }

    #[test]
    fn find_first_element_returns_none_when_no_match() {
        let mut doc = Document::new();
        let html = doc.create_element(QualName::html("html"));
        doc.append_child(doc.root(), html);
        let found = doc.find_first_element(|n| {
            n.element_name().map(|q| q.local == "nonexistent").unwrap_or(false)
        });
        assert!(found.is_none());
    }

    // ──────── InputType ────────

    fn build_input(input_type: Option<&str>) -> Document {
        let mut doc = Document::new();
        let html = doc.create_element(QualName::html("html"));
        let input = doc.create_element(QualName::html("input"));
        if let Some(t) = input_type
            && let NodeData::Element { attrs, .. } = &mut doc.get_mut(input).data
        {
            attrs.push(Attribute {
                name: QualName::html("type"),
                value: t.to_string(),
            });
        }
        doc.append_child(doc.root(), html);
        doc.append_child(html, input);
        doc
    }

    fn input_node(doc: &Document) -> &Node {
        // root → html → input.
        let html_id = doc.get(doc.root()).children[0];
        let input_id = doc.get(html_id).children[0];
        doc.get(input_id)
    }

    #[test]
    fn input_type_default_is_text() {
        let doc = build_input(None);
        assert_eq!(input_node(&doc).input_type(), Some(InputType::Text));
    }

    #[test]
    fn input_type_explicit_text() {
        let doc = build_input(Some("text"));
        assert_eq!(input_node(&doc).input_type(), Some(InputType::Text));
    }

    #[test]
    fn input_type_password() {
        let doc = build_input(Some("password"));
        assert_eq!(input_node(&doc).input_type(), Some(InputType::Password));
    }

    #[test]
    fn input_type_email() {
        let doc = build_input(Some("email"));
        assert_eq!(input_node(&doc).input_type(), Some(InputType::Email));
    }

    #[test]
    fn input_type_all_standard() {
        // Все 22 стандартных значения.
        for (s, expected) in [
            ("tel", InputType::Tel),
            ("url", InputType::Url),
            ("number", InputType::Number),
            ("search", InputType::Search),
            ("date", InputType::Date),
            ("datetime-local", InputType::DateTimeLocal),
            ("time", InputType::Time),
            ("month", InputType::Month),
            ("week", InputType::Week),
            ("color", InputType::Color),
            ("range", InputType::Range),
            ("checkbox", InputType::Checkbox),
            ("radio", InputType::Radio),
            ("file", InputType::File),
            ("submit", InputType::Submit),
            ("reset", InputType::Reset),
            ("button", InputType::Button),
            ("image", InputType::Image),
            ("hidden", InputType::Hidden),
        ] {
            let doc = build_input(Some(s));
            assert_eq!(input_node(&doc).input_type(), Some(expected), "type={s}");
        }
    }

    #[test]
    fn input_type_case_insensitive() {
        let doc = build_input(Some("EMAIL"));
        assert_eq!(input_node(&doc).input_type(), Some(InputType::Email));
        let doc2 = build_input(Some("Checkbox"));
        assert_eq!(input_node(&doc2).input_type(), Some(InputType::Checkbox));
    }

    #[test]
    fn input_type_unknown_becomes_other() {
        let doc = build_input(Some("future-feature"));
        assert_eq!(
            input_node(&doc).input_type(),
            Some(InputType::Other("future-feature".to_string()))
        );
    }

    #[test]
    fn input_type_empty_string_treated_as_text() {
        let doc = build_input(Some(""));
        assert_eq!(input_node(&doc).input_type(), Some(InputType::Text));
    }

    #[test]
    fn input_type_none_for_non_input_element() {
        let mut doc = Document::new();
        let p = doc.create_element(QualName::html("p"));
        doc.append_child(doc.root(), p);
        let p_id = doc.get(doc.root()).children[0];
        assert_eq!(doc.get(p_id).input_type(), None);
    }

    #[test]
    fn input_type_round_trip_via_as_str() {
        for t in [
            InputType::Text,
            InputType::Password,
            InputType::Email,
            InputType::Tel,
            InputType::Url,
            InputType::Number,
            InputType::Search,
            InputType::Date,
            InputType::DateTimeLocal,
            InputType::Time,
            InputType::Month,
            InputType::Week,
            InputType::Color,
            InputType::Range,
            InputType::Checkbox,
            InputType::Radio,
            InputType::File,
            InputType::Submit,
            InputType::Reset,
            InputType::Button,
            InputType::Image,
            InputType::Hidden,
            InputType::Other("custom".into()),
        ] {
            assert_eq!(InputType::parse(t.as_str()), t);
        }
    }

    #[test]
    fn input_type_is_textual_classification() {
        assert!(InputType::Text.is_textual());
        assert!(InputType::Email.is_textual());
        assert!(InputType::Password.is_textual());
        assert!(InputType::Number.is_textual());
        assert!(!InputType::Checkbox.is_textual());
        assert!(!InputType::File.is_textual());
    }

    #[test]
    fn input_type_is_button_like() {
        assert!(InputType::Submit.is_button_like());
        assert!(InputType::Reset.is_button_like());
        assert!(InputType::Button.is_button_like());
        assert!(InputType::Image.is_button_like());
        assert!(!InputType::Text.is_button_like());
        assert!(!InputType::Checkbox.is_button_like());
    }

    // ──────── DocumentMode ────────

    #[test]
    fn document_default_mode_is_no_quirks() {
        let doc = Document::new();
        assert_eq!(doc.mode(), DocumentMode::NoQuirks);
    }

    #[test]
    fn document_mode_can_be_set() {
        let mut doc = Document::new();
        doc.set_mode(DocumentMode::Quirks);
        assert_eq!(doc.mode(), DocumentMode::Quirks);
        doc.set_mode(DocumentMode::LimitedQuirks);
        assert_eq!(doc.mode(), DocumentMode::LimitedQuirks);
        doc.set_mode(DocumentMode::NoQuirks);
        assert_eq!(doc.mode(), DocumentMode::NoQuirks);
    }

    // ──────── target_id ────────

    #[test]
    fn document_default_target_is_none() {
        let doc = Document::new();
        assert_eq!(doc.target(), None);
    }

    #[test]
    fn document_target_round_trips_set_get() {
        let mut doc = Document::new();
        doc.set_target(Some("intro"));
        assert_eq!(doc.target(), Some("intro"));
        doc.set_target::<String>(None);
        assert_eq!(doc.target(), None);
    }

    #[test]
    fn document_set_target_empty_becomes_none() {
        // Empty fragment («#» в URL) трактуется как «нет target-а»: страница
        // не должна никого подсвечивать. Совпадает с поведением major-браузеров.
        let mut doc = Document::new();
        doc.set_target(Some(""));
        assert_eq!(doc.target(), None);
    }

    // ──────── sandbox_flags ────────

    fn build_iframe(sandbox: Option<&str>) -> (Document, NodeId) {
        let mut doc = Document::new();
        let iframe = doc.create_element(QualName::html("iframe"));
        if let Some(val) = sandbox
            && let NodeData::Element { attrs, .. } = &mut doc.get_mut(iframe).data
        {
            attrs.push(Attribute {
                name: QualName::html("sandbox"),
                value: val.to_string(),
            });
        }
        doc.append_child(doc.root(), iframe);
        (doc, iframe)
    }

    #[test]
    fn sandbox_flags_none_for_non_iframe() {
        let mut doc = Document::new();
        let div = doc.create_element(QualName::html("div"));
        doc.append_child(doc.root(), div);
        assert_eq!(doc.get(div).sandbox_flags(), None);
    }

    #[test]
    fn sandbox_flags_iframe_without_attribute_is_empty() {
        let (doc, iframe) = build_iframe(None);
        let flags = doc.get(iframe).sandbox_flags().unwrap();
        assert!(flags.is_empty());
    }

    #[test]
    fn sandbox_flags_iframe_empty_attribute_all_restrictions() {
        let (doc, iframe) = build_iframe(Some(""));
        let flags = doc.get(iframe).sandbox_flags().unwrap();
        assert_eq!(flags, SandboxFlags::all_restrictions());
    }

    #[test]
    fn sandbox_flags_allow_scripts_lifts_scripts() {
        let (doc, iframe) = build_iframe(Some("allow-scripts"));
        let flags = doc.get(iframe).sandbox_flags().unwrap();
        assert!(!flags.contains(SandboxFlags::SCRIPTS));
        assert!(flags.contains(SandboxFlags::FORMS));
    }

    #[test]
    fn sandbox_flags_allow_forms_and_scripts() {
        let (doc, iframe) = build_iframe(Some("allow-scripts allow-forms"));
        let flags = doc.get(iframe).sandbox_flags().unwrap();
        assert!(!flags.contains(SandboxFlags::SCRIPTS));
        assert!(!flags.contains(SandboxFlags::FORMS));
        assert!(flags.contains(SandboxFlags::ORIGIN));
    }

    #[test]
    fn sandbox_flags_allow_same_origin() {
        let (doc, iframe) = build_iframe(Some("allow-same-origin"));
        let flags = doc.get(iframe).sandbox_flags().unwrap();
        assert!(!flags.contains(SandboxFlags::ORIGIN));
        assert!(flags.contains(SandboxFlags::SCRIPTS));
    }

    // ──────── collect_forms / check_form_gate ────────

    fn build_doc_with_form(
        action: Option<&str>,
        method: Option<&str>,
        controls: &[&str],
    ) -> Document {
        let mut doc = Document::new();
        let html = doc.create_element(QualName::html("html"));
        let body = doc.create_element(QualName::html("body"));
        let form = doc.create_element(QualName::html("form"));
        if let Some(a) = action
            && let NodeData::Element { attrs, .. } = &mut doc.get_mut(form).data
        {
            attrs.push(Attribute {
                name: QualName::html("action"),
                value: a.to_string(),
            });
        }
        if let Some(m) = method
            && let NodeData::Element { attrs, .. } = &mut doc.get_mut(form).data
        {
            attrs.push(Attribute {
                name: QualName::html("method"),
                value: m.to_string(),
            });
        }
        doc.append_child(doc.root(), html);
        doc.append_child(html, body);
        doc.append_child(body, form);
        for &tag in controls {
            let el = doc.create_element(QualName::html(tag));
            doc.append_child(form, el);
        }
        doc
    }

    #[test]
    fn collect_forms_finds_form_with_action_and_method() {
        let doc = build_doc_with_form(Some("/submit"), Some("post"), &["input"]);
        let mut forms = Vec::new();
        collect_forms(&doc, doc.root(), &mut forms);
        assert_eq!(forms.len(), 1);
        assert_eq!(forms[0].action, "/submit");
        assert_eq!(forms[0].method, "post");
        assert_eq!(forms[0].field_count, 1);
    }

    #[test]
    fn collect_forms_defaults_action_and_method() {
        let doc = build_doc_with_form(None, None, &[]);
        let mut forms = Vec::new();
        collect_forms(&doc, doc.root(), &mut forms);
        assert_eq!(forms.len(), 1);
        assert_eq!(forms[0].action, "");
        assert_eq!(forms[0].method, "get");
        assert_eq!(forms[0].field_count, 0);
    }

    #[test]
    fn collect_forms_counts_all_control_types() {
        let doc =
            build_doc_with_form(None, None, &["input", "select", "textarea", "button"]);
        let mut forms = Vec::new();
        collect_forms(&doc, doc.root(), &mut forms);
        assert_eq!(forms[0].field_count, 4);
    }

    #[test]
    fn collect_forms_skips_non_form_elements() {
        let mut doc = Document::new();
        let div = doc.create_element(QualName::html("div"));
        doc.append_child(doc.root(), div);
        let mut forms = Vec::new();
        collect_forms(&doc, doc.root(), &mut forms);
        assert!(forms.is_empty());
    }

    #[test]
    fn check_form_gate_no_forms_returns_zero() {
        let doc = Document::new();
        assert_eq!(check_form_gate(&doc, SandboxFlags::empty()), 0);
        assert_eq!(check_form_gate(&doc, SandboxFlags::FORMS), 0);
    }

    #[test]
    fn check_form_gate_blocked_by_sandbox_returns_count() {
        let doc = build_doc_with_form(Some("/login"), None, &["input"]);
        assert_eq!(check_form_gate(&doc, SandboxFlags::FORMS), 1);
    }

    #[test]
    fn check_form_gate_allowed_returns_zero() {
        let doc = build_doc_with_form(Some("/login"), None, &["input"]);
        assert_eq!(check_form_gate(&doc, SandboxFlags::empty()), 0);
    }

    // ──────── collect_anchors / check_navigation_gate ────────

    fn build_doc_with_anchors(hrefs: &[&str]) -> Document {
        let mut doc = Document::new();
        let html = doc.create_element(QualName::html("html"));
        let body = doc.create_element(QualName::html("body"));
        doc.append_child(doc.root(), html);
        doc.append_child(html, body);
        for &href in hrefs {
            let a = doc.create_element(QualName::html("a"));
            if let NodeData::Element { attrs, .. } = &mut doc.get_mut(a).data {
                attrs.push(Attribute {
                    name: QualName::html("href"),
                    value: href.to_string(),
                });
            }
            doc.append_child(body, a);
        }
        doc
    }

    #[test]
    fn collect_anchors_finds_href_links() {
        let doc = build_doc_with_anchors(&["/page1", "/page2"]);
        let mut anchors = Vec::new();
        collect_anchors(&doc, doc.root(), &mut anchors);
        assert_eq!(anchors.len(), 2);
        assert_eq!(anchors[0].href, "/page1");
        assert_eq!(anchors[1].href, "/page2");
    }

    #[test]
    fn collect_anchors_skips_empty_href() {
        let doc = build_doc_with_anchors(&[""]);
        let mut anchors = Vec::new();
        collect_anchors(&doc, doc.root(), &mut anchors);
        assert!(anchors.is_empty());
    }

    #[test]
    fn collect_anchors_skips_anchor_without_href() {
        let mut doc = Document::new();
        let a = doc.create_element(QualName::html("a"));
        doc.append_child(doc.root(), a);
        let mut anchors = Vec::new();
        collect_anchors(&doc, doc.root(), &mut anchors);
        assert!(anchors.is_empty());
    }

    #[test]
    fn check_navigation_gate_no_anchors_returns_zero() {
        let doc = Document::new();
        assert_eq!(check_navigation_gate(&doc, SandboxFlags::empty()), 0);
        assert_eq!(check_navigation_gate(&doc, SandboxFlags::NAVIGATION), 0);
    }

    #[test]
    fn check_navigation_gate_blocked_by_sandbox_returns_count() {
        let doc = build_doc_with_anchors(&["/a", "/b"]);
        assert_eq!(check_navigation_gate(&doc, SandboxFlags::NAVIGATION), 2);
    }

    #[test]
    fn check_navigation_gate_allowed_returns_zero() {
        let doc = build_doc_with_anchors(&["/a"]);
        assert_eq!(check_navigation_gate(&doc, SandboxFlags::empty()), 0);
    }

    // ──────── Shadow DOM ────────

    fn build_shadow_host() -> (Document, NodeId, NodeId) {
        // <div id="host">  ← shadow host
        //   #shadow-root(open)
        //     <span>shadow</span>
        //   <p>light</p>   ← light-tree child (no slot match → not in flat tree)
        let mut doc = Document::new();
        let host = doc.create_element(QualName::html("div"));
        doc.append_child(doc.root(), host);

        let sr = doc.attach_shadow(host, ShadowRootMode::Open);
        let span = doc.create_element(QualName::html("span"));
        let text = doc.create_text("shadow");
        doc.append_child(sr, span);
        doc.append_child(span, text);

        let light_p = doc.create_element(QualName::html("p"));
        doc.append_child(host, light_p);

        (doc, host, sr)
    }

    #[test]
    fn attach_shadow_registers_host() {
        let (doc, host, sr) = build_shadow_host();
        assert!(doc.is_shadow_host(host));
        assert_eq!(doc.shadow_root_of(host), Some(sr));
    }

    #[test]
    fn shadow_root_node_data_variant() {
        let (doc, _, sr) = build_shadow_host();
        assert!(matches!(
            doc.get(sr).data,
            NodeData::ShadowRoot { mode: ShadowRootMode::Open }
        ));
    }

    #[test]
    fn shadow_root_mode_display() {
        assert_eq!(ShadowRootMode::Open.to_string(), "open");
        assert_eq!(ShadowRootMode::Closed.to_string(), "closed");
    }

    #[test]
    fn flat_tree_no_shadow_is_zero_alloc() {
        let mut doc = Document::new();
        let html = doc.create_element(QualName::html("html"));
        let body = doc.create_element(QualName::html("body"));
        doc.append_child(doc.root(), html);
        doc.append_child(html, body);

        let flat = build_flat_tree(&doc);
        // No overrides — fast path, HashMap is empty.
        assert!(flat.overrides.is_empty());
        // children_of falls through to DOM children.
        assert_eq!(flat.children_of(&doc, html), &[body]);
    }

    #[test]
    fn flat_tree_host_children_are_shadow_root_children() {
        let (doc, host, sr) = build_shadow_host();
        let flat = build_flat_tree(&doc);

        // Host's composed children = shadow root's DOM children (the <span>).
        let sr_children = doc.get(sr).children.clone();
        assert_eq!(flat.children_of(&doc, host), sr_children.as_slice());
    }

    #[test]
    fn flat_tree_slot_distributes_light_children() {
        // Shadow: <slot name="x"> … </slot>
        // Light:  <p slot="x">light</p>
        // After flat tree: slot's composed children = [<p>]
        let mut doc = Document::new();
        let host = doc.create_element(QualName::html("div"));
        doc.append_child(doc.root(), host);

        let sr = doc.attach_shadow(host, ShadowRootMode::Open);

        let slot = doc.create_element(QualName::html("slot"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(slot).data {
            attrs.push(Attribute { name: QualName::html("name"), value: "x".into() });
        }
        let fallback = doc.create_text("fallback");
        doc.append_child(sr, slot);
        doc.append_child(slot, fallback);

        let light_p = doc.create_element(QualName::html("p"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(light_p).data {
            attrs.push(Attribute { name: QualName::html("slot"), value: "x".into() });
        }
        doc.append_child(host, light_p);

        let flat = build_flat_tree(&doc);

        // Slot is overridden with assigned light node, not fallback.
        assert_eq!(flat.children_of(&doc, slot), &[light_p]);
    }

    #[test]
    fn flat_tree_slot_fallback_when_no_assigned_nodes() {
        // Slot with name "x" but no light-tree child with slot="x".
        let mut doc = Document::new();
        let host = doc.create_element(QualName::html("div"));
        doc.append_child(doc.root(), host);

        let sr = doc.attach_shadow(host, ShadowRootMode::Open);
        let slot = doc.create_element(QualName::html("slot"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(slot).data {
            attrs.push(Attribute { name: QualName::html("name"), value: "y".into() });
        }
        let fallback = doc.create_text("fallback");
        doc.append_child(sr, slot);
        doc.append_child(slot, fallback);

        let flat = build_flat_tree(&doc);
        // No assignment → no override → slot keeps its DOM children (fallback).
        assert_eq!(flat.children_of(&doc, slot), &[fallback]);
    }

    #[test]
    fn flat_tree_nested_shadow_with_slot_delegation() {
        // Scenario:
        // <custom-component>
        //   #shadow-root(open)
        //     <slot name="item"></slot>
        //   <custom-item slot="item">
        //     #shadow-root(open)
        //       <div>Item content</div>
        //   </custom-item>
        //
        // Expected flat tree:
        // - custom-component's composed children = [custom-item (from shadow root)]
        // - slot's composed children = [custom-item (from light tree assignment)]
        // - custom-item's composed children = [<div>Item content</div> (from its shadow root)]

        let mut doc = Document::new();

        // Create outer component with shadow tree
        let outer_host = doc.create_element(QualName::html("custom-component"));
        doc.append_child(doc.root(), outer_host);

        let outer_shadow = doc.attach_shadow(outer_host, ShadowRootMode::Open);
        let outer_slot = doc.create_element(QualName::html("slot"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(outer_slot).data {
            attrs.push(Attribute {
                name: QualName::html("name"),
                value: "item".into(),
            });
        }
        doc.append_child(outer_shadow, outer_slot);

        // Create inner component with shadow tree and slot attribute
        let inner_host = doc.create_element(QualName::html("custom-item"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(inner_host).data {
            attrs.push(Attribute {
                name: QualName::html("slot"),
                value: "item".into(),
            });
        }
        doc.append_child(outer_host, inner_host); // Light tree child of outer

        let inner_shadow = doc.attach_shadow(inner_host, ShadowRootMode::Open);
        let inner_content = doc.create_element(QualName::html("div"));
        doc.append_child(inner_shadow, inner_content);

        let flat = build_flat_tree(&doc);

        // Outer host should have shadow root children (which includes slot)
        assert_eq!(flat.children_of(&doc, outer_host), &[outer_slot]);

        // Outer slot should have inner_host as its assigned child
        assert_eq!(flat.children_of(&doc, outer_slot), &[inner_host]);

        // Inner host should have inner_content as its composed child (from its shadow root)
        assert_eq!(flat.children_of(&doc, inner_host), &[inner_content]);
    }

    #[test]
    fn flat_tree_nested_slot_fallback() {
        // Scenario:
        // <outer-component>
        //   #shadow-root(open)
        //     <slot name="header">
        //       <default-header></default-header>
        //     </slot>
        //   <!-- light tree: no child with slot="header", so fallback is used -->
        //
        // Expected: slot should have its DOM child (default-header) as composed children.

        let mut doc = Document::new();

        let outer_host = doc.create_element(QualName::html("outer-component"));
        doc.append_child(doc.root(), outer_host);

        let outer_shadow = doc.attach_shadow(outer_host, ShadowRootMode::Open);
        let slot = doc.create_element(QualName::html("slot"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(slot).data {
            attrs.push(Attribute {
                name: QualName::html("name"),
                value: "header".into(),
            });
        }
        doc.append_child(outer_shadow, slot);

        let fallback = doc.create_element(QualName::html("default-header"));
        doc.append_child(slot, fallback);

        // No light-tree children with slot="header", so fallback should be used.

        let flat = build_flat_tree(&doc);

        // Slot should have fallback as its composed children (no assignment).
        assert_eq!(flat.children_of(&doc, slot), &[fallback]);
    }

    #[test]
    fn shadow_root_printed_in_display() {
        let (doc, _, _) = build_shadow_host();
        let s = doc.to_string();
        assert!(s.contains("#shadow-root (open)"));
    }

    // ── form submission helpers ──────────────────────────────────────────────

    fn make_form_doc() -> (Document, NodeId, NodeId, NodeId) {
        // <form action="/send" method="post">
        //   <input name="user" value="alice">
        //   <input type="submit">
        // </form>
        let mut doc = Document::new();
        let form = doc.create_element(QualName::html("form"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(form).data {
            attrs.push(Attribute { name: QualName::html("action"), value: "/send".into() });
            attrs.push(Attribute { name: QualName::html("method"), value: "post".into() });
        }
        let input = doc.create_element(QualName::html("input"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(input).data {
            attrs.push(Attribute { name: QualName::html("name"), value: "user".into() });
            attrs.push(Attribute { name: QualName::html("value"), value: "alice".into() });
        }
        let submit = doc.create_element(QualName::html("input"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(submit).data {
            attrs.push(Attribute { name: QualName::html("type"), value: "submit".into() });
        }
        doc.append_child(doc.root(), form);
        doc.append_child(form, input);
        doc.append_child(form, submit);
        (doc, form, input, submit)
    }

    #[test]
    fn find_ancestor_form_direct_child() {
        let (doc, form, input, _) = make_form_doc();
        assert_eq!(find_ancestor_form(&doc, input), Some(form));
    }

    #[test]
    fn find_ancestor_form_nested() {
        let mut doc = Document::new();
        let form = doc.create_element(QualName::html("form"));
        let div = doc.create_element(QualName::html("div"));
        let input = doc.create_element(QualName::html("input"));
        doc.append_child(doc.root(), form);
        doc.append_child(form, div);
        doc.append_child(div, input);
        assert_eq!(find_ancestor_form(&doc, input), Some(form));
    }

    #[test]
    fn find_ancestor_form_no_form() {
        let mut doc = Document::new();
        let div = doc.create_element(QualName::html("div"));
        let input = doc.create_element(QualName::html("input"));
        doc.append_child(doc.root(), div);
        doc.append_child(div, input);
        assert_eq!(find_ancestor_form(&doc, input), None);
    }

    #[test]
    fn find_ancestor_dialog_direct() {
        let mut doc = Document::new();
        let dialog = doc.create_element(QualName::html("dialog"));
        let form = doc.create_element(QualName::html("form"));
        doc.append_child(doc.root(), dialog);
        doc.append_child(dialog, form);
        assert_eq!(find_ancestor_dialog(&doc, form), Some(dialog));
    }

    #[test]
    fn find_ancestor_dialog_nested() {
        let mut doc = Document::new();
        let dialog = doc.create_element(QualName::html("dialog"));
        let div = doc.create_element(QualName::html("div"));
        let btn = doc.create_element(QualName::html("button"));
        doc.append_child(doc.root(), dialog);
        doc.append_child(dialog, div);
        doc.append_child(div, btn);
        assert_eq!(find_ancestor_dialog(&doc, btn), Some(dialog));
    }

    #[test]
    fn find_ancestor_dialog_none() {
        let mut doc = Document::new();
        let div = doc.create_element(QualName::html("div"));
        let btn = doc.create_element(QualName::html("button"));
        doc.append_child(doc.root(), div);
        doc.append_child(div, btn);
        assert_eq!(find_ancestor_dialog(&doc, btn), None);
    }

    // ── node_is_contenteditable / find_editing_host ───────────────────────────

    #[test]
    fn node_is_contenteditable_true_for_empty_attr() {
        let mut doc = Document::new();
        let div = doc.create_element(QualName::html("div"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(div).data {
            attrs.push(Attribute { name: QualName::html("contenteditable"), value: String::new() });
        }
        assert!(node_is_contenteditable(&doc, div));
    }

    #[test]
    fn node_is_contenteditable_true_for_value_true() {
        let mut doc = Document::new();
        let div = doc.create_element(QualName::html("div"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(div).data {
            attrs.push(Attribute { name: QualName::html("contenteditable"), value: "true".into() });
        }
        assert!(node_is_contenteditable(&doc, div));
    }

    #[test]
    fn node_is_contenteditable_false_for_value_false() {
        let mut doc = Document::new();
        let div = doc.create_element(QualName::html("div"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(div).data {
            attrs.push(Attribute { name: QualName::html("contenteditable"), value: "false".into() });
        }
        assert!(!node_is_contenteditable(&doc, div));
    }

    #[test]
    fn node_is_contenteditable_false_without_attr() {
        let mut doc = Document::new();
        let div = doc.create_element(QualName::html("div"));
        assert!(!node_is_contenteditable(&doc, div));
    }

    #[test]
    fn find_editing_host_self() {
        let mut doc = Document::new();
        let div = doc.create_element(QualName::html("div"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(div).data {
            attrs.push(Attribute { name: QualName::html("contenteditable"), value: String::new() });
        }
        assert_eq!(find_editing_host(&doc, div), Some(div));
    }

    #[test]
    fn find_editing_host_ancestor() {
        let mut doc = Document::new();
        let outer = doc.create_element(QualName::html("div"));
        let inner = doc.create_element(QualName::html("span"));
        let text = doc.create_text("hi");
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(outer).data {
            attrs.push(Attribute { name: QualName::html("contenteditable"), value: String::new() });
        }
        doc.append_child(doc.root(), outer);
        doc.append_child(outer, inner);
        doc.append_child(inner, text);
        assert_eq!(find_editing_host(&doc, text), Some(outer));
    }

    #[test]
    fn find_editing_host_none_when_no_editable() {
        let mut doc = Document::new();
        let div = doc.create_element(QualName::html("div"));
        let text = doc.create_text("hi");
        doc.append_child(doc.root(), div);
        doc.append_child(div, text);
        assert_eq!(find_editing_host(&doc, text), None);
    }

    /// Builds `<html><body><div>hi</div></body></html>` with no `contenteditable`
    /// attribute anywhere, for `document.designMode` (BUG-353) coverage.
    fn make_design_mode_doc() -> (Document, NodeId, NodeId) {
        let mut doc = Document::new();
        let html = doc.create_element(QualName::html("html"));
        let body = doc.create_element(QualName::html("body"));
        let div = doc.create_element(QualName::html("div"));
        let text = doc.create_text("hi");
        doc.append_child(doc.root(), html);
        doc.append_child(html, body);
        doc.append_child(body, div);
        doc.append_child(div, text);
        (doc, body, text)
    }

    #[test]
    fn find_editing_host_design_mode_off_stays_none() {
        let (doc, _body, text) = make_design_mode_doc();
        assert!(!doc.design_mode());
        assert_eq!(find_editing_host(&doc, text), None);
    }

    #[test]
    fn find_editing_host_design_mode_on_falls_back_to_body() {
        let (mut doc, body, text) = make_design_mode_doc();
        doc.set_design_mode(true);
        assert_eq!(find_editing_host(&doc, text), Some(body));
        // The document's own root has no `<body>`-derived fallback issue: any
        // node in the tree resolves to the same host.
        assert_eq!(find_editing_host(&doc, body), Some(body));
    }

    #[test]
    fn find_editing_host_design_mode_on_still_honours_explicit_contenteditable() {
        let (mut doc, body, text) = make_design_mode_doc();
        doc.set_design_mode(true);
        let div = doc.get(text).parent.unwrap();
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(div).data {
            attrs.push(Attribute { name: QualName::html("contenteditable"), value: "true".into() });
        }
        // The explicit, nearer host wins over the design-mode fallback.
        assert_eq!(find_editing_host(&doc, text), Some(div));
        assert_ne!(find_editing_host(&doc, text), Some(body));
    }

    #[test]
    fn collect_dom_form_fields_basic() {
        let (doc, form, _, _) = make_form_doc();
        let fields = collect_dom_form_fields(&doc, form);
        // submit input должен быть исключён; только "user" должен попасть
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].0, "user");
        assert_eq!(fields[0].1, "alice");
    }

    #[test]
    fn collect_dom_form_fields_skips_disabled() {
        let mut doc = Document::new();
        let form = doc.create_element(QualName::html("form"));
        let input = doc.create_element(QualName::html("input"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(input).data {
            attrs.push(Attribute { name: QualName::html("name"), value: "x".into() });
            attrs.push(Attribute { name: QualName::html("value"), value: "v".into() });
            attrs.push(Attribute { name: QualName::html("disabled"), value: String::new() });
        }
        doc.append_child(doc.root(), form);
        doc.append_child(form, input);
        let fields = collect_dom_form_fields(&doc, form);
        assert!(fields.is_empty());
    }

    #[test]
    fn collect_dom_form_fields_skips_nameless() {
        let mut doc = Document::new();
        let form = doc.create_element(QualName::html("form"));
        let input = doc.create_element(QualName::html("input"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(input).data {
            attrs.push(Attribute { name: QualName::html("value"), value: "v".into() });
            // no "name" attribute
        }
        doc.append_child(doc.root(), form);
        doc.append_child(form, input);
        let fields = collect_dom_form_fields(&doc, form);
        assert!(fields.is_empty());
    }

    #[test]
    fn collect_dom_form_fields_unchecked_checkbox_excluded() {
        let mut doc = Document::new();
        let form = doc.create_element(QualName::html("form"));
        let cb = doc.create_element(QualName::html("input"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(cb).data {
            attrs.push(Attribute { name: QualName::html("type"), value: "checkbox".into() });
            attrs.push(Attribute { name: QualName::html("name"), value: "agree".into() });
            // no "checked" attribute — не отмечен
        }
        doc.append_child(doc.root(), form);
        doc.append_child(form, cb);
        let fields = collect_dom_form_fields(&doc, form);
        assert!(fields.is_empty());
    }

    #[test]
    fn collect_dom_form_fields_checked_checkbox_included() {
        let mut doc = Document::new();
        let form = doc.create_element(QualName::html("form"));
        let cb = doc.create_element(QualName::html("input"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(cb).data {
            attrs.push(Attribute { name: QualName::html("type"), value: "checkbox".into() });
            attrs.push(Attribute { name: QualName::html("name"), value: "agree".into() });
            attrs.push(Attribute { name: QualName::html("checked"), value: String::new() });
        }
        doc.append_child(doc.root(), form);
        doc.append_child(form, cb);
        let fields = collect_dom_form_fields(&doc, form);
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].0, "agree");
        assert_eq!(fields[0].1, "on"); // default checkbox value
    }

    #[test]
    fn collect_dom_form_fields_textarea() {
        // HTML LS §4.10.11: a textarea's default value is its child text —
        // it has no `value` content attribute (BUG-441).
        let mut doc = Document::new();
        let form = doc.create_element(QualName::html("form"));
        let ta = doc.create_element(QualName::html("textarea"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(ta).data {
            attrs.push(Attribute { name: QualName::html("name"), value: "msg".into() });
        }
        let text = doc.create_text("hello".to_string());
        doc.append_child(doc.root(), form);
        doc.append_child(form, ta);
        doc.append_child(ta, text);
        let fields = collect_dom_form_fields(&doc, form);
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0], ("msg".into(), "hello".into()));
    }

    // ── Runtime («dirty») control value — BUG-441 ─────────────────────────────

    #[test]
    fn control_value_falls_back_to_default_then_follows_dirty_value() {
        let mut doc = Document::new();
        let inp = doc.create_element(QualName::html("input"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(inp).data {
            attrs.push(Attribute { name: QualName::html("value"), value: "seed".into() });
        }
        doc.append_child(doc.root(), inp);

        assert_eq!(doc.control_value(inp), "seed");
        assert_eq!(doc.dirty_value(inp), None);

        doc.set_control_value(inp, "typed");
        assert_eq!(doc.control_value(inp), "typed");
        // The attribute keeps holding the default value (`defaultValue`).
        assert_eq!(doc.get(inp).get_attr("value"), Some("seed"));

        doc.clear_control_value(inp);
        assert_eq!(doc.control_value(inp), "seed");
    }

    #[test]
    fn control_value_of_textarea_shadows_child_text() {
        let mut doc = Document::new();
        let ta = doc.create_element(QualName::html("textarea"));
        let text = doc.create_text("default".to_string());
        doc.append_child(doc.root(), ta);
        doc.append_child(ta, text);

        assert_eq!(doc.control_value(ta), "default");
        doc.set_control_value(ta, "edited");
        assert_eq!(doc.control_value(ta), "edited");
    }

    #[test]
    fn collect_dom_form_fields_uses_runtime_value() {
        let (mut doc, form, user, _) = make_form_doc();
        doc.set_control_value(user, "bob");
        let fields = collect_dom_form_fields(&doc, form);
        assert_eq!(fields, vec![("user".to_string(), "bob".to_string())]);
    }

    // ── Runtime («dirty») checkedness — BUG-444 ───────────────────────────────

    /// Build `<form><input type=checkbox name=opt checked></form>` — the
    /// shape the whole default-vs-current distinction turns on.
    fn make_checkbox_form_doc() -> (Document, NodeId, NodeId) {
        let mut doc = Document::new();
        let form = doc.create_element(QualName::html("form"));
        let cb = doc.create_element(QualName::html("input"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(cb).data {
            attrs.push(Attribute { name: QualName::html("type"), value: "checkbox".into() });
            attrs.push(Attribute { name: QualName::html("name"), value: "opt".into() });
            attrs.push(Attribute { name: QualName::html("checked"), value: String::new() });
        }
        doc.append_child(doc.root(), form);
        doc.append_child(form, cb);
        (doc, form, cb)
    }

    #[test]
    fn control_checked_falls_back_to_default_then_follows_dirty_checkedness() {
        let (mut doc, _, cb) = make_checkbox_form_doc();

        assert!(doc.control_checked(cb));
        assert_eq!(doc.dirty_checked(cb), None);

        doc.set_control_checked(cb, false);
        assert!(!doc.control_checked(cb));
        assert_eq!(doc.dirty_checked(cb), Some(false));
        // The attribute keeps holding the default (`defaultChecked`) — this is
        // the whole point of BUG-444: an unticking click must not destroy it.
        assert!(doc.get(cb).get_attr("checked").is_some());

        // `form.reset()` drops the dirty checkedness → back to the default.
        doc.clear_control_checked(cb);
        assert!(doc.control_checked(cb));
        assert_eq!(doc.dirty_checked(cb), None);
    }

    /// A checkbox with no `checked` attribute defaults to unchecked, and
    /// ticking it must not invent an attribute either.
    #[test]
    fn control_checked_of_unchecked_default_is_dirty_only() {
        let (mut doc, _, cb) = make_checkbox_form_doc();
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(cb).data {
            attrs.retain(|a| a.name.local.as_str() != "checked");
        }
        assert!(!doc.control_checked(cb));

        doc.set_control_checked(cb, true);
        assert!(doc.control_checked(cb));
        assert!(doc.get(cb).get_attr("checked").is_none());

        doc.clear_control_checked(cb);
        assert!(!doc.control_checked(cb));
    }

    #[test]
    fn collect_dom_form_fields_uses_runtime_checkedness() {
        let (mut doc, form, cb) = make_checkbox_form_doc();
        // Checked by attribute → submitted with the spec's default value `on`.
        assert_eq!(collect_dom_form_fields(&doc, form), vec![("opt".to_string(), "on".to_string())]);

        // Unticked at runtime → omitted, even though the attribute is still there.
        doc.set_control_checked(cb, false);
        assert!(collect_dom_form_fields(&doc, form).is_empty());

        // …and back again.
        doc.set_control_checked(cb, true);
        assert_eq!(collect_dom_form_fields(&doc, form), vec![("opt".to_string(), "on".to_string())]);
    }

    #[test]
    fn validity_reads_runtime_checkedness() {
        let (mut doc, _, cb) = make_checkbox_form_doc();
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(cb).data {
            attrs.push(Attribute { name: QualName::html("required"), value: String::new() });
        }
        // Ticked by default → satisfied.
        assert!(!element_validity(&doc, cb).unwrap().value_missing);

        // Unticked at runtime → valueMissing, on current state not the default.
        doc.set_control_checked(cb, false);
        assert!(element_validity(&doc, cb).unwrap().value_missing);
    }

    #[test]
    fn validity_reads_runtime_value() {
        let mut doc = Document::new();
        let inp = doc.create_element(QualName::html("input"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(inp).data {
            attrs.push(Attribute { name: QualName::html("type"), value: "email".into() });
            attrs.push(Attribute { name: QualName::html("required"), value: String::new() });
        }
        doc.append_child(doc.root(), inp);

        // Empty field → valueMissing.
        let vs = element_validity(&doc, inp).unwrap();
        assert!(vs.value_missing);

        // Filled in at runtime → no longer missing, but still a type mismatch.
        doc.set_control_value(inp, "not-an-email");
        let vs = element_validity(&doc, inp).unwrap();
        assert!(!vs.value_missing);
        assert!(vs.type_mismatch);

        doc.set_control_value(inp, "a@b.com");
        assert!(element_validity(&doc, inp).unwrap().valid());
    }

    #[test]
    fn collect_dom_form_fields_multiple() {
        let mut doc = Document::new();
        let form = doc.create_element(QualName::html("form"));
        for (name, val) in [("a", "1"), ("b", "2"), ("c", "3")] {
            let inp = doc.create_element(QualName::html("input"));
            if let NodeData::Element { attrs, .. } = &mut doc.get_mut(inp).data {
                attrs.push(Attribute { name: QualName::html("name"), value: name.into() });
                attrs.push(Attribute { name: QualName::html("value"), value: val.into() });
            }
            doc.append_child(form, inp);
        }
        doc.append_child(doc.root(), form);
        let fields = collect_dom_form_fields(&doc, form);
        assert_eq!(fields.len(), 3);
        assert_eq!(fields[0], ("a".into(), "1".into()));
        assert_eq!(fields[1], ("b".into(), "2".into()));
        assert_eq!(fields[2], ("c".into(), "3".into()));
    }

    // ──────────────────────────────────────────────────────────────────────────
    // ValidityState tests
    // ──────────────────────────────────────────────────────────────────────────

    fn make_input(attrs: &[(&str, &str)]) -> (Document, NodeId, NodeId) {
        let mut doc = Document::new();
        let form = doc.create_element(QualName::html("form"));
        let inp = doc.create_element(QualName::html("input"));
        if let NodeData::Element { attrs: a, .. } = &mut doc.get_mut(inp).data {
            for &(name, val) in attrs {
                a.push(Attribute { name: QualName::html(name), value: val.into() });
            }
        }
        doc.append_child(doc.root(), form);
        doc.append_child(form, inp);
        (doc, form, inp)
    }

    #[test]
    fn validity_non_form_element_returns_none() {
        let mut doc = Document::new();
        let div = doc.create_element(QualName::html("div"));
        doc.append_child(doc.root(), div);
        assert_eq!(element_validity(&doc, div), None);
    }

    #[test]
    fn validity_hidden_input_returns_none() {
        let (doc, _, inp) = make_input(&[("type", "hidden"), ("required", "")]);
        assert_eq!(element_validity(&doc, inp), None);
    }

    #[test]
    fn validity_submit_input_returns_none() {
        let (doc, _, inp) = make_input(&[("type", "submit")]);
        assert_eq!(element_validity(&doc, inp), None);
    }

    #[test]
    fn validity_disabled_input_returns_none() {
        let (doc, _, inp) = make_input(&[("required", ""), ("disabled", "")]);
        assert_eq!(element_validity(&doc, inp), None);
    }

    #[test]
    fn validity_required_empty_value_missing() {
        let (doc, _, inp) = make_input(&[("required", ""), ("value", "")]);
        let vs = element_validity(&doc, inp).unwrap();
        assert!(vs.value_missing);
        assert!(!vs.valid());
    }

    #[test]
    fn validity_required_with_value_not_missing() {
        let (doc, _, inp) = make_input(&[("required", ""), ("value", "alice")]);
        let vs = element_validity(&doc, inp).unwrap();
        assert!(!vs.value_missing);
        assert!(vs.valid());
    }

    #[test]
    fn validity_required_checkbox_unchecked_missing() {
        let (doc, _, inp) = make_input(&[("type", "checkbox"), ("required", "")]);
        let vs = element_validity(&doc, inp).unwrap();
        assert!(vs.value_missing);
    }

    #[test]
    fn validity_required_checkbox_checked_ok() {
        let (doc, _, inp) = make_input(&[("type", "checkbox"), ("required", ""), ("checked", "")]);
        let vs = element_validity(&doc, inp).unwrap();
        assert!(!vs.value_missing);
        assert!(vs.valid());
    }

    #[test]
    fn validity_email_type_mismatch() {
        let (doc, _, inp) = make_input(&[("type", "email"), ("value", "notanemail")]);
        let vs = element_validity(&doc, inp).unwrap();
        assert!(vs.type_mismatch);
        assert!(!vs.valid());
    }

    #[test]
    fn validity_email_valid() {
        let (doc, _, inp) = make_input(&[("type", "email"), ("value", "user@example.com")]);
        let vs = element_validity(&doc, inp).unwrap();
        assert!(!vs.type_mismatch);
        assert!(vs.valid());
    }

    #[test]
    fn validity_url_type_mismatch() {
        let (doc, _, inp) = make_input(&[("type", "url"), ("value", "notaurl")]);
        let vs = element_validity(&doc, inp).unwrap();
        assert!(vs.type_mismatch);
    }

    #[test]
    fn validity_url_valid() {
        let (doc, _, inp) = make_input(&[("type", "url"), ("value", "https://example.com")]);
        let vs = element_validity(&doc, inp).unwrap();
        assert!(!vs.type_mismatch);
        assert!(vs.valid());
    }

    #[test]
    fn validity_range_underflow() {
        let (doc, _, inp) = make_input(&[("type", "number"), ("min", "10"), ("value", "5")]);
        let vs = element_validity(&doc, inp).unwrap();
        assert!(vs.range_underflow);
        assert!(!vs.range_overflow);
        assert!(!vs.valid());
    }

    #[test]
    fn validity_range_overflow() {
        let (doc, _, inp) = make_input(&[("type", "number"), ("max", "10"), ("value", "20")]);
        let vs = element_validity(&doc, inp).unwrap();
        assert!(vs.range_overflow);
        assert!(!vs.range_underflow);
        assert!(!vs.valid());
    }

    #[test]
    fn validity_number_in_range() {
        let (doc, _, inp) = make_input(&[("type", "number"), ("min", "0"), ("max", "100"), ("value", "50")]);
        let vs = element_validity(&doc, inp).unwrap();
        assert!(!vs.range_underflow);
        assert!(!vs.range_overflow);
        assert!(vs.valid());
    }

    #[test]
    fn validity_too_long() {
        let (doc, _, inp) = make_input(&[("maxlength", "3"), ("value", "hello")]);
        let vs = element_validity(&doc, inp).unwrap();
        assert!(vs.too_long);
        assert!(!vs.valid());
    }

    #[test]
    fn validity_too_short() {
        let (doc, _, inp) = make_input(&[("minlength", "5"), ("value", "hi")]);
        let vs = element_validity(&doc, inp).unwrap();
        assert!(vs.too_short);
        assert!(!vs.valid());
    }

    #[test]
    fn validity_length_ok() {
        let (doc, _, inp) = make_input(&[("minlength", "2"), ("maxlength", "10"), ("value", "hello")]);
        let vs = element_validity(&doc, inp).unwrap();
        assert!(!vs.too_short);
        assert!(!vs.too_long);
        assert!(vs.valid());
    }

    #[test]
    fn validity_empty_value_not_too_short() {
        // tooShort only applies when field has a value; empty is valueMissing territory.
        let (doc, _, inp) = make_input(&[("minlength", "5"), ("value", "")]);
        let vs = element_validity(&doc, inp).unwrap();
        assert!(!vs.too_short);
    }

    #[test]
    fn check_validity_form_all_valid() {
        let mut doc = Document::new();
        let form = doc.create_element(QualName::html("form"));
        let inp = doc.create_element(QualName::html("input"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(inp).data {
            attrs.push(Attribute { name: QualName::html("required"), value: "".into() });
            attrs.push(Attribute { name: QualName::html("value"), value: "filled".into() });
        }
        doc.append_child(doc.root(), form);
        doc.append_child(form, inp);
        assert!(check_validity_form(&doc, form));
    }

    #[test]
    fn check_validity_form_one_invalid() {
        let mut doc = Document::new();
        let form = doc.create_element(QualName::html("form"));
        // valid input
        let inp1 = doc.create_element(QualName::html("input"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(inp1).data {
            attrs.push(Attribute { name: QualName::html("value"), value: "ok".into() });
        }
        // invalid input: required but empty
        let inp2 = doc.create_element(QualName::html("input"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(inp2).data {
            attrs.push(Attribute { name: QualName::html("required"), value: "".into() });
            attrs.push(Attribute { name: QualName::html("value"), value: "".into() });
        }
        doc.append_child(doc.root(), form);
        doc.append_child(form, inp1);
        doc.append_child(form, inp2);
        assert!(!check_validity_form(&doc, form));
    }

    #[test]
    fn invalid_controls_in_form_finds_them() {
        let mut doc = Document::new();
        let form = doc.create_element(QualName::html("form"));
        let inp1 = doc.create_element(QualName::html("input"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(inp1).data {
            attrs.push(Attribute { name: QualName::html("value"), value: "ok".into() });
        }
        let inp2 = doc.create_element(QualName::html("input"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(inp2).data {
            attrs.push(Attribute { name: QualName::html("required"), value: "".into() });
            attrs.push(Attribute { name: QualName::html("value"), value: "".into() });
        }
        doc.append_child(doc.root(), form);
        doc.append_child(form, inp1);
        doc.append_child(form, inp2);
        let invalid = invalid_controls_in_form(&doc, form);
        assert_eq!(invalid.len(), 1);
        assert_eq!(invalid[0], inp2);
    }

    // ──────── submit_form (HTML5 §4.10.22) ────────

    #[test]
    fn submit_form_valid_single_field() {
        let mut doc = Document::new();
        let form = doc.create_element(QualName::html("form"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(form).data {
            attrs.push(Attribute { name: QualName::html("action"), value: "/submit".into() });
            attrs.push(Attribute { name: QualName::html("method"), value: "POST".into() });
        }
        let inp = doc.create_element(QualName::html("input"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(inp).data {
            attrs.push(Attribute { name: QualName::html("name"), value: "username".into() });
            attrs.push(Attribute { name: QualName::html("value"), value: "alice".into() });
        }
        doc.append_child(doc.root(), form);
        doc.append_child(form, inp);

        let result = submit_form(&doc, form);
        if let FormSubmitEvent::Valid { action, method, fields } = result {
            assert_eq!(action, "/submit");
            assert_eq!(method, "post"); // lowercase
            assert_eq!(fields.len(), 1);
            assert_eq!(fields[0], ("username".to_string(), "alice".to_string()));
        } else {
            panic!("Expected Valid but got {:?}", result);
        }
    }

    #[test]
    fn submit_form_valid_multiple_fields() {
        let mut doc = Document::new();
        let form = doc.create_element(QualName::html("form"));
        doc.append_child(doc.root(), form);

        // Field 1: text input
        let inp1 = doc.create_element(QualName::html("input"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(inp1).data {
            attrs.push(Attribute { name: QualName::html("name"), value: "field1".into() });
            attrs.push(Attribute { name: QualName::html("value"), value: "value1".into() });
        }
        doc.append_child(form, inp1);

        // Field 2: checkbox (unchecked, should not be included)
        let inp2 = doc.create_element(QualName::html("input"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(inp2).data {
            attrs.push(Attribute { name: QualName::html("type"), value: "checkbox".into() });
            attrs.push(Attribute { name: QualName::html("name"), value: "field2".into() });
            attrs.push(Attribute { name: QualName::html("value"), value: "checked_val".into() });
        }
        doc.append_child(form, inp2);

        // Field 3: checkbox (checked)
        let inp3 = doc.create_element(QualName::html("input"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(inp3).data {
            attrs.push(Attribute { name: QualName::html("type"), value: "checkbox".into() });
            attrs.push(Attribute { name: QualName::html("name"), value: "field3".into() });
            attrs.push(Attribute { name: QualName::html("value"), value: "checked_val".into() });
            attrs.push(Attribute { name: QualName::html("checked"), value: "".into() });
        }
        doc.append_child(form, inp3);

        let result = submit_form(&doc, form);
        if let FormSubmitEvent::Valid { fields, .. } = result {
            assert_eq!(fields.len(), 2); // only field1 and field3
            assert_eq!(fields[0], ("field1".to_string(), "value1".to_string()));
            assert_eq!(fields[1], ("field3".to_string(), "checked_val".to_string()));
        } else {
            panic!("Expected Valid but got {:?}", result);
        }
    }

    #[test]
    fn submit_form_invalid_required_field() {
        let mut doc = Document::new();
        let form = doc.create_element(QualName::html("form"));
        let inp = doc.create_element(QualName::html("input"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(inp).data {
            attrs.push(Attribute { name: QualName::html("required"), value: "".into() });
            attrs.push(Attribute { name: QualName::html("value"), value: "".into() });
        }
        doc.append_child(doc.root(), form);
        doc.append_child(form, inp);

        let result = submit_form(&doc, form);
        if let FormSubmitEvent::Invalid { invalid_controls } = result {
            assert_eq!(invalid_controls.len(), 1);
            assert_eq!(invalid_controls[0], inp);
        } else {
            panic!("Expected Invalid but got {:?}", result);
        }
    }

    #[test]
    fn submit_form_invalid_email() {
        let mut doc = Document::new();
        let form = doc.create_element(QualName::html("form"));
        let inp = doc.create_element(QualName::html("input"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(inp).data {
            attrs.push(Attribute { name: QualName::html("type"), value: "email".into() });
            attrs.push(Attribute { name: QualName::html("value"), value: "not-an-email".into() });
        }
        doc.append_child(doc.root(), form);
        doc.append_child(form, inp);

        let result = submit_form(&doc, form);
        if let FormSubmitEvent::Invalid { invalid_controls } = result {
            assert_eq!(invalid_controls.len(), 1);
            assert_eq!(invalid_controls[0], inp);
        } else {
            panic!("Expected Invalid but got {:?}", result);
        }
    }

    #[test]
    fn submit_form_multiple_invalid_fields() {
        let mut doc = Document::new();
        let form = doc.create_element(QualName::html("form"));

        // Invalid field 1: required but empty
        let inp1 = doc.create_element(QualName::html("input"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(inp1).data {
            attrs.push(Attribute { name: QualName::html("required"), value: "".into() });
            attrs.push(Attribute { name: QualName::html("value"), value: "".into() });
        }

        // Invalid field 2: too short
        let inp2 = doc.create_element(QualName::html("input"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(inp2).data {
            attrs.push(Attribute { name: QualName::html("minlength"), value: "5".into() });
            attrs.push(Attribute { name: QualName::html("value"), value: "hi".into() });
        }

        doc.append_child(doc.root(), form);
        doc.append_child(form, inp1);
        doc.append_child(form, inp2);

        let result = submit_form(&doc, form);
        if let FormSubmitEvent::Invalid { invalid_controls } = result {
            assert_eq!(invalid_controls.len(), 2);
            assert_eq!(invalid_controls[0], inp1);
            assert_eq!(invalid_controls[1], inp2);
        } else {
            panic!("Expected Invalid but got {:?}", result);
        }
    }

    #[test]
    fn submit_form_defaults_action_and_method() {
        let mut doc = Document::new();
        let form = doc.create_element(QualName::html("form"));
        let inp = doc.create_element(QualName::html("input"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(inp).data {
            attrs.push(Attribute { name: QualName::html("name"), value: "f".into() });
            attrs.push(Attribute { name: QualName::html("value"), value: "v".into() });
        }
        doc.append_child(doc.root(), form);
        doc.append_child(form, inp);

        let result = submit_form(&doc, form);
        if let FormSubmitEvent::Valid { action, method, .. } = result {
            assert_eq!(action, ""); // default empty action
            assert_eq!(method, "get"); // default get
        } else {
            panic!("Expected Valid but got {:?}", result);
        }
    }

    #[test]
    fn submit_form_non_form_element() {
        let mut doc = Document::new();
        let div = doc.create_element(QualName::html("div"));
        let inp = doc.create_element(QualName::html("input"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(inp).data {
            attrs.push(Attribute { name: QualName::html("required"), value: "".into() });
        }
        doc.append_child(doc.root(), div);
        doc.append_child(div, inp);

        // submit_form called on non-form element should treat as vacuously valid
        let result = submit_form(&doc, div);
        if let FormSubmitEvent::Valid { fields, .. } = result {
            assert_eq!(fields.len(), 0);
        } else {
            panic!("Expected Valid but got {:?}", result);
        }
    }

    #[test]
    fn test_form_submit_event_valid_variant() {
        let event = FormSubmitEvent::Valid {
            action: "/test".to_string(),
            method: "post".to_string(),
            fields: vec![],
        };
        if let FormSubmitEvent::Valid { action, .. } = event {
            assert_eq!(action, "/test");
        } else {
            panic!("Expected Valid variant");
        }
    }

    // ──────── EditInputType ────────

    #[test]
    fn edit_input_type_as_str_round_trip() {
        let cases = [
            (EditInputType::InsertText, "insertText"),
            (EditInputType::InsertParagraph, "insertParagraph"),
            (EditInputType::InsertLineBreak, "insertLineBreak"),
            (EditInputType::DeleteContentBackward, "deleteContentBackward"),
            (EditInputType::DeleteContentForward, "deleteContentForward"),
            (EditInputType::DeleteWordBackward, "deleteWordBackward"),
            (EditInputType::DeleteWordForward, "deleteWordForward"),
            (EditInputType::InsertFromPaste, "insertFromPaste"),
            (EditInputType::DeleteByCut, "deleteByCut"),
            (EditInputType::SelectAll, "selectAll"),
            (EditInputType::HistoryUndo, "historyUndo"),
            (EditInputType::HistoryRedo, "historyRedo"),
        ];
        for (variant, expected) in cases {
            assert_eq!(variant.as_str(), expected, "mismatch for {:?}", variant);
        }
    }

    // ──────── insert_text_at ────────

    fn make_text_doc(content: &str) -> (Document, NodeId, NodeId) {
        let mut doc = Document::new();
        let div = doc.create_element(QualName::html("div"));
        let text = doc.create_text(content);
        doc.append_child(doc.root(), div);
        doc.append_child(div, text);
        (doc, div, text)
    }

    #[test]
    fn insert_text_at_start() {
        let (mut doc, _, text) = make_text_doc("world");
        let pos = DomPosition { container: text, offset: 0 };
        let new_pos = insert_text_at(&mut doc, pos, "Hello ");
        match &doc.get(text).data {
            NodeData::Text(s) => assert_eq!(s, "Hello world"),
            _ => panic!("not a text node"),
        }
        assert_eq!(new_pos.container, text);
        assert_eq!(new_pos.offset, 6);
    }

    #[test]
    fn insert_text_at_end() {
        let (mut doc, _, text) = make_text_doc("Hello");
        let pos = DomPosition { container: text, offset: 5 };
        let new_pos = insert_text_at(&mut doc, pos, " world");
        match &doc.get(text).data {
            NodeData::Text(s) => assert_eq!(s, "Hello world"),
            _ => panic!("not a text node"),
        }
        assert_eq!(new_pos.offset, 11);
    }

    #[test]
    fn insert_text_at_mid() {
        let (mut doc, _, text) = make_text_doc("Helo");
        let pos = DomPosition { container: text, offset: 3 };
        let new_pos = insert_text_at(&mut doc, pos, "l");
        match &doc.get(text).data {
            NodeData::Text(s) => assert_eq!(s, "Hello"),
            _ => panic!("not a text node"),
        }
        assert_eq!(new_pos.offset, 4);
    }

    #[test]
    fn insert_text_at_empty_node() {
        let (mut doc, _, text) = make_text_doc("");
        let pos = DomPosition { container: text, offset: 0 };
        let new_pos = insert_text_at(&mut doc, pos, "Hi");
        match &doc.get(text).data {
            NodeData::Text(s) => assert_eq!(s, "Hi"),
            _ => panic!("not a text node"),
        }
        assert_eq!(new_pos.offset, 2);
    }

    #[test]
    fn insert_text_at_element_creates_text_child() {
        let mut doc = Document::new();
        let div = doc.create_element(QualName::html("div"));
        doc.append_child(doc.root(), div);
        let pos = DomPosition { container: div, offset: 0 };
        let new_pos = insert_text_at(&mut doc, pos, "abc");
        // A text child was created.
        let children = &doc.get(div).children;
        assert_eq!(children.len(), 1);
        match &doc.get(children[0]).data {
            NodeData::Text(s) => assert_eq!(s, "abc"),
            _ => panic!("no text child created"),
        }
        assert_eq!(new_pos.offset, 3);
    }

    #[test]
    fn insert_text_at_multibyte_utf8() {
        // "Привет" — each Cyrillic char is 2 bytes in UTF-8.
        // Char boundaries: П=0, р=2, и=4, в=6, е=8, т=10, end=12.
        // offset 4 is exactly the start of "и" — insert X before "и".
        let (mut doc, _, text) = make_text_doc("Привет");
        let pos = DomPosition { container: text, offset: 4 };
        let new_pos = insert_text_at(&mut doc, pos, "X");
        match &doc.get(text).data {
            NodeData::Text(s) => assert_eq!(s, "ПрXивет"),
            _ => panic!("not a text node"),
        }
        // 4 bytes before + 1 byte "X" = offset 5.
        assert_eq!(new_pos.offset, 5);
    }

    #[test]
    fn insert_text_noop_when_empty_string() {
        let (mut doc, _, text) = make_text_doc("abc");
        let pos = DomPosition { container: text, offset: 1 };
        let new_pos = insert_text_at(&mut doc, pos, "");
        match &doc.get(text).data {
            NodeData::Text(s) => assert_eq!(s, "abc"),
            _ => panic!("not a text node"),
        }
        assert_eq!(new_pos, pos);
    }

    // ──────── delete_range ────────

    #[test]
    fn delete_range_same_node_full() {
        let (mut doc, _, text) = make_text_doc("Hello");
        let range = Range {
            start: DomPosition { container: text, offset: 0 },
            end:   DomPosition { container: text, offset: 5 },
        };
        let pos = delete_range(&mut doc, &range);
        match &doc.get(text).data {
            NodeData::Text(s) => assert!(s.is_empty()),
            _ => panic!("not a text node"),
        }
        assert_eq!(pos.offset, 0);
    }

    #[test]
    fn delete_range_same_node_partial() {
        let (mut doc, _, text) = make_text_doc("Hello world");
        let range = Range {
            start: DomPosition { container: text, offset: 5 },
            end:   DomPosition { container: text, offset: 11 },
        };
        let pos = delete_range(&mut doc, &range);
        match &doc.get(text).data {
            NodeData::Text(s) => assert_eq!(s, "Hello"),
            _ => panic!("not a text node"),
        }
        assert_eq!(pos.offset, 5);
    }

    #[test]
    fn delete_range_collapsed_noop() {
        let (mut doc, _, text) = make_text_doc("abc");
        let range = Range::collapsed(DomPosition { container: text, offset: 1 });
        let pos = delete_range(&mut doc, &range);
        match &doc.get(text).data {
            NodeData::Text(s) => assert_eq!(s, "abc"),
            _ => panic!("not a text node"),
        }
        assert_eq!(pos.offset, 1);
    }

    // ──────── split_text_node ────────

    #[test]
    fn split_text_node_basic() {
        let (mut doc, div, text) = make_text_doc("Hello world");
        let second = split_text_node(&mut doc, text, 5);
        // First node: "Hello"
        match &doc.get(text).data {
            NodeData::Text(s) => assert_eq!(s, "Hello"),
            _ => panic!(),
        }
        // Second node: " world"
        match &doc.get(second).data {
            NodeData::Text(s) => assert_eq!(s, " world"),
            _ => panic!(),
        }
        // Parent has two text children in correct order.
        let children = &doc.get(div).children;
        assert_eq!(children, &[text, second]);
    }

    #[test]
    fn split_text_node_at_start() {
        let (mut doc, div, text) = make_text_doc("abc");
        let second = split_text_node(&mut doc, text, 0);
        match &doc.get(text).data {
            NodeData::Text(s) => assert!(s.is_empty()),
            _ => panic!(),
        }
        match &doc.get(second).data {
            NodeData::Text(s) => assert_eq!(s, "abc"),
            _ => panic!(),
        }
        assert_eq!(doc.get(div).children, vec![text, second]);
    }

    #[test]
    fn split_text_node_at_end() {
        let (mut doc, div, text) = make_text_doc("abc");
        let second = split_text_node(&mut doc, text, 3);
        match &doc.get(text).data {
            NodeData::Text(s) => assert_eq!(s, "abc"),
            _ => panic!(),
        }
        match &doc.get(second).data {
            NodeData::Text(s) => assert!(s.is_empty()),
            _ => panic!(),
        }
        assert_eq!(doc.get(div).children, vec![text, second]);
    }

    // ──────── insert_paragraph_break ────────

    #[test]
    fn insert_paragraph_break_creates_br() {
        let (mut doc, div, text) = make_text_doc("Hello world");
        let pos = DomPosition { container: text, offset: 5 };
        let new_pos = insert_paragraph_break(&mut doc, pos, div);

        // The div should now have: [text("Hello"), br, text(" world")]
        let children = doc.get(div).children.clone();
        assert_eq!(children.len(), 3);

        match &doc.get(children[0]).data {
            NodeData::Text(s) => assert_eq!(s, "Hello"),
            _ => panic!("expected first text node"),
        }
        match &doc.get(children[1]).data {
            NodeData::Element { name, .. } => assert_eq!(name.local, "br"),
            _ => panic!("expected br element"),
        }
        match &doc.get(children[2]).data {
            NodeData::Text(s) => assert_eq!(s, " world"),
            _ => panic!("expected second text node"),
        }
        // New caret position is at the start of the second text node.
        assert_eq!(new_pos.offset, 0);
        assert_eq!(new_pos.container, children[2]);
    }

    #[test]
    fn insert_paragraph_break_on_element_appends_br() {
        let mut doc = Document::new();
        let div = doc.create_element(QualName::html("div"));
        doc.append_child(doc.root(), div);
        let pos = DomPosition { container: div, offset: 0 };
        let new_pos = insert_paragraph_break(&mut doc, pos, div);
        let children = doc.get(div).children.clone();
        // Should have a <br> and an empty text node.
        assert_eq!(children.len(), 2);
        match &doc.get(children[0]).data {
            NodeData::Element { name, .. } => assert_eq!(name.local, "br"),
            _ => panic!("expected br"),
        }
        assert_eq!(new_pos.offset, 0);
    }

    // ── collect_iframes ───────────────────────────────────────────────────────

    fn make_iframe(sandbox: Option<&str>, src: Option<&str>) -> Document {
        let mut doc = Document::new();
        let iframe = doc.create_element(QualName::html("iframe"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(iframe).data {
            if let Some(s) = sandbox {
                attrs.push(Attribute { name: QualName::html("sandbox"), value: s.to_string() });
            }
            if let Some(s) = src {
                attrs.push(Attribute { name: QualName::html("src"), value: s.to_string() });
            }
        }
        doc.append_child(doc.root(), iframe);
        doc
    }

    #[test]
    fn collect_iframes_empty_document() {
        let mut doc = Document::new();
        let div = doc.create_element(QualName::html("div"));
        doc.append_child(doc.root(), div);
        assert!(collect_iframes(&doc).is_empty());
    }

    #[test]
    fn collect_iframes_finds_iframe_without_sandbox() {
        let doc = make_iframe(None, Some("https://example.com"));
        let frames = collect_iframes(&doc);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].src.as_deref(), Some("https://example.com"));
        assert!(!frames[0].is_sandboxed);
        assert!(frames[0].sandbox.is_empty());
    }

    /// BUG-854: `<frame>` — такой же хост вложенного browsing context.
    /// Строится не через `make_iframe`, чтобы тег был виден в самом тесте.
    fn make_frame(attrs_in: &[(&str, &str)]) -> Document {
        let mut doc = Document::new();
        let frame = doc.create_element(QualName::html("frame"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(frame).data {
            for (k, v) in attrs_in {
                attrs.push(Attribute { name: QualName::html(*k), value: (*v).to_string() });
            }
        }
        doc.append_child(doc.root(), frame);
        doc
    }

    #[test]
    fn collect_iframes_finds_frame_element() {
        let doc = make_frame(&[("src", "child.html"), ("name", "f1")]);
        let frames = collect_iframes(&doc);
        assert_eq!(frames.len(), 1, "<frame> is a nested browsing context host too");
        assert_eq!(frames[0].src.as_deref(), Some("child.html"));
        assert_eq!(frames[0].name.as_deref(), Some("f1"));
        assert!(!frames[0].is_sandboxed);
    }

    #[test]
    fn collect_iframes_frame_srcdoc_is_not_a_source() {
        // `srcdoc` объявлен только у `<iframe>` — на `<frame>` это обычный
        // неизвестный атрибут, и брать его как источник нельзя.
        let doc = make_frame(&[("srcdoc", "<p>inline"), ("src", "child.html")]);
        let frames = collect_iframes(&doc);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].srcdoc, None);
        assert_eq!(frames[0].src.as_deref(), Some("child.html"));
    }

    #[test]
    fn collect_iframes_sandboxed_empty_attr_all_restrictions() {
        let doc = make_iframe(Some(""), Some("page.html"));
        let frames = collect_iframes(&doc);
        assert_eq!(frames.len(), 1);
        assert!(frames[0].is_sandboxed);
        assert_eq!(frames[0].sandbox, SandboxFlags::all_restrictions());
        assert!(frames[0].sandbox.contains(SandboxFlags::SCRIPTS));
        assert!(frames[0].sandbox.contains(SandboxFlags::FORMS));
        assert!(frames[0].sandbox.contains(SandboxFlags::AUXILIARY_NAVIGATION));
    }

    #[test]
    fn collect_iframes_allow_scripts_lifts_scripts_flag() {
        let doc = make_iframe(Some("allow-scripts"), Some("a.html"));
        let frames = collect_iframes(&doc);
        assert_eq!(frames.len(), 1);
        assert!(frames[0].is_sandboxed);
        assert!(!frames[0].sandbox.contains(SandboxFlags::SCRIPTS));
        assert!(frames[0].sandbox.contains(SandboxFlags::FORMS));
    }

    #[test]
    fn collect_iframes_loading_and_fetchpriority() {
        let mut doc = Document::new();
        // iframe1: loading="lazy", fetchpriority="high"
        let iframe1 = doc.create_element(QualName::html("iframe"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(iframe1).data {
            attrs.push(Attribute { name: QualName::html("src"), value: "a.html".to_string() });
            attrs.push(Attribute { name: QualName::html("loading"), value: "LAZY".to_string() });
            attrs.push(Attribute { name: QualName::html("fetchpriority"), value: "HIGH".to_string() });
        }
        // iframe2: loading="eager", fetchpriority="low"
        let iframe2 = doc.create_element(QualName::html("iframe"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(iframe2).data {
            attrs.push(Attribute { name: QualName::html("src"), value: "b.html".to_string() });
            attrs.push(Attribute { name: QualName::html("loading"), value: "eager".to_string() });
            attrs.push(Attribute { name: QualName::html("fetchpriority"), value: "low".to_string() });
        }
        // iframe3: fetchpriority="auto" → None, без loading
        let iframe3 = doc.create_element(QualName::html("iframe"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(iframe3).data {
            attrs.push(Attribute { name: QualName::html("src"), value: "c.html".to_string() });
            attrs.push(Attribute { name: QualName::html("fetchpriority"), value: "auto".to_string() });
        }
        doc.append_child(doc.root(), iframe1);
        doc.append_child(doc.root(), iframe2);
        doc.append_child(doc.root(), iframe3);
        let iframes = collect_iframes(&doc);
        assert_eq!(iframes.len(), 3);
        assert!(iframes[0].loading_lazy, "loading=LAZY (case-insensitive) must set loading_lazy");
        assert_eq!(iframes[0].fetch_priority, Some("high".to_string()));
        assert!(!iframes[1].loading_lazy, "loading=eager must not set loading_lazy");
        assert_eq!(iframes[1].fetch_priority, Some("low".to_string()));
        assert!(!iframes[2].loading_lazy, "absent loading must not set loading_lazy");
        assert_eq!(iframes[2].fetch_priority, None, "fetchpriority=auto must map to None");
    }

    #[test]
    fn collect_iframes_multiple_iframes() {
        let mut doc = Document::new();
        let body = doc.create_element(QualName::html("body"));
        doc.append_child(doc.root(), body);

        // iframe 1: no sandbox
        let f1 = doc.create_element(QualName::html("iframe"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(f1).data {
            attrs.push(Attribute { name: QualName::html("src"), value: "a.html".to_string() });
        }
        doc.append_child(body, f1);

        // iframe 2: allow-scripts allow-forms
        let f2 = doc.create_element(QualName::html("iframe"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(f2).data {
            attrs.push(Attribute {
                name: QualName::html("sandbox"),
                value: "allow-scripts allow-forms".to_string(),
            });
            attrs.push(Attribute { name: QualName::html("src"), value: "b.html".to_string() });
        }
        doc.append_child(body, f2);

        // iframe 3: sandbox="" (all restrictions)
        let f3 = doc.create_element(QualName::html("iframe"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(f3).data {
            attrs.push(Attribute { name: QualName::html("sandbox"), value: String::new() });
            attrs.push(Attribute { name: QualName::html("src"), value: "c.html".to_string() });
        }
        doc.append_child(body, f3);

        let frames = collect_iframes(&doc);
        assert_eq!(frames.len(), 3);
        assert!(!frames[0].is_sandboxed);
        assert!(frames[1].is_sandboxed);
        assert!(!frames[1].sandbox.contains(SandboxFlags::SCRIPTS));
        assert!(!frames[1].sandbox.contains(SandboxFlags::FORMS));
        assert!(frames[2].is_sandboxed);
        assert_eq!(frames[2].sandbox, SandboxFlags::all_restrictions());
    }

    #[test]
    fn collect_iframes_srcdoc_attribute() {
        let mut doc = Document::new();
        let body = doc.create_element(QualName::html("body"));
        doc.append_child(doc.root(), body);

        let f = doc.create_element(QualName::html("iframe"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(f).data {
            attrs.push(Attribute {
                name: QualName::html("srcdoc"),
                value: "<p>hello</p>".to_string(),
            });
        }
        doc.append_child(body, f);

        let frames = collect_iframes(&doc);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].srcdoc.as_deref(), Some("<p>hello</p>"));
        assert!(frames[0].src.is_none());
    }

    // ── check_popup_gate ──────────────────────────────────────────────────────

    #[test]
    fn popup_gate_blocked_when_auxiliary_navigation_set() {
        assert!(check_popup_gate(SandboxFlags::AUXILIARY_NAVIGATION));
    }

    #[test]
    fn popup_gate_allowed_when_flag_not_set() {
        assert!(!check_popup_gate(SandboxFlags::empty()));
    }

    #[test]
    fn popup_gate_blocked_when_all_restrictions() {
        assert!(check_popup_gate(SandboxFlags::all_restrictions()));
    }

    #[test]
    fn popup_gate_allowed_after_allow_popups() {
        let flags = lumen_core::parse_sandbox_value(Some("allow-popups"));
        assert!(!check_popup_gate(flags));
    }

    // ── DOM snapshot (T3 hibernation) ─────────────────────────────────────────

    #[test]
    fn snapshot_empty_document_roundtrip() {
        let doc = Document::new();
        let bytes = doc.to_bytes().expect("encode");
        let restored = Document::from_bytes(&bytes).expect("decode");
        assert_eq!(restored.mode(), doc.mode());
        assert_eq!(restored.root(), doc.root());
    }

    #[test]
    fn snapshot_document_with_elements_roundtrip() {
        let mut doc = Document::new();
        let html = doc.create_element(QualName::html("html"));
        doc.append_child(doc.root(), html);
        let body = doc.create_element(QualName::html("body"));
        doc.append_child(html, body);
        let text = doc.create_text("hello world");
        doc.append_child(body, text);
        doc.set_mode(DocumentMode::NoQuirks);

        let bytes = doc.to_bytes().expect("encode");
        let restored = Document::from_bytes(&bytes).expect("decode");

        assert_eq!(restored.mode(), DocumentMode::NoQuirks);
        let root_children = restored.get(restored.root()).children.clone();
        assert_eq!(root_children.len(), 1);
        let html_id = root_children[0];
        let html_children = restored.get(html_id).children.clone();
        assert_eq!(html_children.len(), 1);
        let body_id = html_children[0];
        let body_children = restored.get(body_id).children.clone();
        assert_eq!(body_children.len(), 1);
        let text_id = body_children[0];
        assert!(matches!(&restored.get(text_id).data, NodeData::Text(s) if s == "hello world"));
    }

    #[test]
    fn snapshot_document_with_attributes_roundtrip() {
        let mut doc = Document::new();
        let div = doc.create_element(QualName::html("div"));
        // Manually push attributes via NodeData::Element.
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(div).data {
            attrs.push(Attribute { name: QualName::html("class"), value: "container".into() });
            attrs.push(Attribute { name: QualName::html("id"), value: "main".into() });
        }
        doc.append_child(doc.root(), div);

        let bytes = doc.to_bytes().expect("encode");
        let restored = Document::from_bytes(&bytes).expect("decode");

        let div_id = restored.get(restored.root()).children[0];
        assert_eq!(restored.get(div_id).get_attr("class"), Some("container"));
        assert_eq!(restored.get(div_id).get_attr("id"), Some("main"));
    }

    #[test]
    fn snapshot_document_with_shadow_root_roundtrip() {
        let mut doc = Document::new();
        let host = doc.create_element(QualName::html("div"));
        doc.append_child(doc.root(), host);
        let shadow = doc.attach_shadow(host, ShadowRootMode::Open);
        let span = doc.create_element(QualName::html("span"));
        doc.append_child(shadow, span);

        let bytes = doc.to_bytes().expect("encode");
        let restored = Document::from_bytes(&bytes).expect("decode");

        let host_id = restored.get(restored.root()).children[0];
        let shadow_id = restored.shadow_root_of(host_id).expect("shadow root");
        let shadow_children = &restored.get(shadow_id).children;
        assert_eq!(shadow_children.len(), 1);
    }

    #[test]
    fn snapshot_quirks_mode_preserved() {
        let mut doc = Document::new();
        doc.set_mode(DocumentMode::Quirks);
        let bytes = doc.to_bytes().expect("encode");
        let restored = Document::from_bytes(&bytes).expect("decode");
        assert_eq!(restored.mode(), DocumentMode::Quirks);
    }

    #[test]
    fn snapshot_selection_preserved() {
        let mut doc = Document::new();
        let text = doc.create_text("abcdef");
        doc.append_child(doc.root(), text);
        let sel = Selection {
            anchor: Some(DomPosition { container: text, offset: 0 }),
            focus: Some(DomPosition { container: text, offset: 3 }),
        };
        doc.set_selection(sel.clone());

        let bytes = doc.to_bytes().expect("encode");
        let restored = Document::from_bytes(&bytes).expect("decode");
        assert_eq!(restored.get_selection(), &sel);
    }

    #[test]
    fn snapshot_blob_is_compact() {
        // Ensure the snapshot is a reasonable size (not accidentally inflated).
        let mut doc = Document::new();
        let body = doc.create_element(QualName::html("body"));
        doc.append_child(doc.root(), body);
        let text = doc.create_text("hello");
        doc.append_child(body, text);
        let bytes = doc.to_bytes().expect("encode");
        // A 3-node tree should serialize well under 1 KB.
        assert!(bytes.len() < 1024, "snapshot too large: {} bytes", bytes.len());
    }

    // ──────────────────────────────────────────────────────────────────────────
    // IME Composition Events tests
    // ──────────────────────────────────────────────────────────────────────────

    #[test]
    fn composition_event_type_as_str() {
        assert_eq!(CompositionEventType::Start.as_str(), "compositionstart");
        assert_eq!(CompositionEventType::Update.as_str(), "compositionupdate");
        assert_eq!(CompositionEventType::End.as_str(), "compositionend");
    }

    #[test]
    fn composition_event_constructors() {
        let start = CompositionEvent::start("あ".to_string(), Some("ja".to_string()));
        assert_eq!(start.event_type, CompositionEventType::Start);
        assert_eq!(start.data.data, "あ");
        assert_eq!(start.data.locale, Some("ja".to_string()));
        assert_eq!(start.data.range, None);

        let update = CompositionEvent::update("あい".to_string(), Some((0, 2)));
        assert_eq!(update.event_type, CompositionEventType::Update);
        assert_eq!(update.data.data, "あい");
        assert_eq!(update.data.locale, None);
        assert_eq!(update.data.range, Some((0, 2)));

        let end = CompositionEvent::end("あいう".to_string());
        assert_eq!(end.event_type, CompositionEventType::End);
        assert_eq!(end.data.data, "あいう");
        assert_eq!(end.data.locale, None);
        assert_eq!(end.data.range, None);
    }

    // ──────────────────────────────────────────────────────────────────────────
    // Event.isTrusted (DOM §2.10) tests
    // ──────────────────────────────────────────────────────────────────────────

    #[test]
    fn composition_event_helper_constructors_are_trusted() {
        // start/update/end are used by the native IME pipeline; they MUST default
        // to trusted so Cloudflare/DataDome-style isTrusted gates pass for real
        // user keystrokes routed through winit Ime::Preedit/Ime::Commit.
        assert!(CompositionEvent::start("a".into(), None).is_trusted);
        assert!(CompositionEvent::update("ab".into(), Some((0, 2))).is_trusted);
        assert!(CompositionEvent::end("abc".into()).is_trusted);
    }

    #[test]
    fn composition_event_new_is_trusted() {
        // `CompositionEvent::new` is the native-pipeline constructor; it MUST
        // default to trusted. Script-synthesized events go through `untrusted`.
        let data = CompositionData {
            data: "x".to_string(),
            locale: None,
            range: None,
        };
        let evt = CompositionEvent::new(CompositionEventType::Update, data);
        assert!(evt.is_trusted, "CompositionEvent::new must default to trusted");
    }

    #[test]
    fn composition_event_untrusted_constructor() {
        // Page-script-synthesized events (`new CompositionEvent(...)` in JS)
        // are never trusted per DOM §2.10.
        let data = CompositionData {
            data: "x".to_string(),
            locale: None,
            range: None,
        };
        let evt = CompositionEvent::untrusted(CompositionEventType::Start, data);
        assert!(!evt.is_trusted);
        assert_eq!(evt.event_type, CompositionEventType::Start);
    }

    #[test]
    fn input_event_trusted_constructor() {
        let evt = InputEvent::trusted(EditInputType::InsertText, Some("a".into()), false);
        assert!(evt.is_trusted);
        assert_eq!(evt.input_type, EditInputType::InsertText);
        assert_eq!(evt.data.as_deref(), Some("a"));
        assert!(!evt.is_composing);
    }

    #[test]
    fn input_event_untrusted_constructor() {
        // Script-synthesized `new InputEvent('input', { ... })` events must
        // never be trusted, regardless of the `inputType`.
        let evt = InputEvent::untrusted(EditInputType::DeleteContentBackward, None, false);
        assert!(!evt.is_trusted);
        assert_eq!(evt.input_type, EditInputType::DeleteContentBackward);
    }

    #[test]
    fn input_event_is_trusted_independent_of_is_composing() {
        // is_trusted (provenance) is orthogonal to is_composing (IME state):
        // a real user IME keystroke is both trusted AND composing.
        let real_ime = InputEvent::trusted(EditInputType::InsertText, Some("あ".into()), true);
        assert!(real_ime.is_trusted);
        assert!(real_ime.is_composing);

        // A script-dispatched InputEvent can also carry is_composing=true; the
        // flags must not be conflated.
        let script_ime = InputEvent::untrusted(EditInputType::InsertText, Some("あ".into()), true);
        assert!(!script_ime.is_trusted);
        assert!(script_ime.is_composing);
    }

    #[test]
    fn input_event_clone_preserves_is_trusted() {
        // Cloning must preserve trustedness — the bit is part of event identity
        // per DOM §2.10 (Event.isTrusted is set at construction, never mutated).
        let trusted = InputEvent::trusted(EditInputType::InsertText, Some("x".into()), false);
        let copy = trusted.clone();
        assert!(copy.is_trusted);

        let untrusted = InputEvent::untrusted(EditInputType::InsertText, Some("x".into()), false);
        let copy = untrusted.clone();
        assert!(!copy.is_trusted);
    }

    #[test]
    fn composition_event_clone_preserves_is_trusted() {
        let trusted = CompositionEvent::start("a".into(), None);
        assert!(trusted.clone().is_trusted);

        let data = CompositionData {
            data: "a".to_string(),
            locale: None,
            range: None,
        };
        let untrusted = CompositionEvent::untrusted(CompositionEventType::Start, data);
        assert!(!untrusted.clone().is_trusted);
    }

    #[test]
    fn document_begin_composition() {
        let mut doc = Document::new();
        let input = doc.create_element(QualName::html("input"));
        doc.append_child(doc.root(), input);

        // No composition initially
        assert!(doc.get_composition().is_none());

        // Begin composition
        doc.begin_composition(input, "あ".to_string(), Some("ja".to_string()));
        let comp = doc.get_composition();
        assert!(comp.is_some());
        let comp = comp.unwrap();
        assert_eq!(comp.node, input);
        assert_eq!(comp.text, "あ");
        assert_eq!(comp.locale, Some("ja".to_string()));
        assert_eq!(comp.selection, None);
    }

    #[test]
    fn document_update_composition() {
        let mut doc = Document::new();
        let input = doc.create_element(QualName::html("input"));
        doc.begin_composition(input, "あ".to_string(), Some("ja".to_string()));

        // Update with new preedit and selection
        doc.update_composition("あい".to_string(), Some((0, 2)));
        let comp = doc.get_composition().unwrap();
        assert_eq!(comp.text, "あい");
        assert_eq!(comp.selection, Some((0, 2)));
        // Locale should remain unchanged
        assert_eq!(comp.locale, Some("ja".to_string()));
    }

    #[test]
    fn document_update_composition_no_active() {
        let mut doc = Document::new();
        // Updating without active composition should be a no-op
        doc.update_composition("text".to_string(), Some((0, 4)));
        assert!(doc.get_composition().is_none());
    }

    #[test]
    fn document_end_composition() {
        let mut doc = Document::new();
        let input = doc.create_element(QualName::html("input"));
        doc.begin_composition(input, "あ".to_string(), Some("ja".to_string()));

        // End composition returns the state
        let ended = doc.end_composition();
        assert!(ended.is_some());
        let ended = ended.unwrap();
        assert_eq!(ended.node, input);
        assert_eq!(ended.text, "あ");

        // Composition should now be None
        assert!(doc.get_composition().is_none());
    }

    #[test]
    fn document_end_composition_no_active() {
        let mut doc = Document::new();
        // Ending without active composition should return None
        assert!(doc.end_composition().is_none());
    }

    #[test]
    fn document_composition_sequence() {
        let mut doc = Document::new();
        let input = doc.create_element(QualName::html("input"));

        // Simulates a full IME composition sequence (Japanese input).
        // User wants to type "こんにちは" (konnichiha).

        // 1. Start: User types first key
        doc.begin_composition(input, "こ".to_string(), Some("ja".to_string()));
        assert_eq!(doc.get_composition().unwrap().text, "こ");

        // 2. Update: User continues typing
        doc.update_composition("こん".to_string(), Some((0, 2)));
        assert_eq!(doc.get_composition().unwrap().text, "こん");

        doc.update_composition("こんに".to_string(), Some((0, 3)));
        assert_eq!(doc.get_composition().unwrap().text, "こんに");

        // 3. End: User commits the input
        let final_state = doc.end_composition();
        assert!(final_state.is_some());
        assert_eq!(final_state.unwrap().text, "こんに");

        // Composition is now cleared
        assert!(doc.get_composition().is_none());
    }

    #[test]
    fn composition_state_snapshot_roundtrip() {
        let mut doc = Document::new();
        let input = doc.create_element(QualName::html("input"));
        doc.append_child(doc.root(), input);
        doc.begin_composition(input, "test".to_string(), Some("en".to_string()));

        // Serialize and deserialize
        let bytes = doc.to_bytes().expect("encode");
        let restored = Document::from_bytes(&bytes).expect("decode");

        // Composition state should be preserved
        let restored_comp = restored.get_composition();
        assert!(restored_comp.is_some());
        let restored_comp = restored_comp.unwrap();
        assert_eq!(restored_comp.text, "test");
        assert_eq!(restored_comp.locale, Some("en".to_string()));
    }

    #[test]
    fn composition_helper_is_composing() {
        let mut doc = Document::new();
        let input = doc.create_element(QualName::html("input"));

        // Not composing initially
        assert!(!doc.is_composing());

        // Begin composition
        doc.begin_composition(input, "あ".to_string(), Some("ja".to_string()));
        assert!(doc.is_composing());

        // End composition
        doc.end_composition();
        assert!(!doc.is_composing());
    }

    #[test]
    fn composition_helper_get_range() {
        let mut doc = Document::new();
        let input = doc.create_element(QualName::html("input"));

        // No range initially
        assert!(doc.get_composition_range().is_none());

        // Begin composition without range
        doc.begin_composition(input, "a".to_string(), None);
        assert!(doc.get_composition_range().is_none());

        // Update with range
        doc.update_composition("ab".to_string(), Some((0, 2)));
        assert_eq!(doc.get_composition_range(), Some((0, 2)));

        // Update with different range
        doc.update_composition("abc".to_string(), Some((0, 3)));
        assert_eq!(doc.get_composition_range(), Some((0, 3)));

        // End composition clears range
        doc.end_composition();
        assert!(doc.get_composition_range().is_none());
    }

    #[test]
    fn composition_helper_get_target() {
        let mut doc = Document::new();
        let input1 = doc.create_element(QualName::html("input"));
        let input2 = doc.create_element(QualName::html("textarea"));

        // No target initially
        assert!(doc.get_composition_target().is_none());

        // Begin composition on input1
        doc.begin_composition(input1, "text".to_string(), None);
        assert_eq!(doc.get_composition_target(), Some(input1));

        // End and start on input2
        doc.end_composition();
        doc.begin_composition(input2, "more".to_string(), None);
        assert_eq!(doc.get_composition_target(), Some(input2));

        // End composition clears target
        doc.end_composition();
        assert!(doc.get_composition_target().is_none());
    }

    #[test]
    fn composition_helpers_with_ranges() {
        let mut doc = Document::new();
        let contenteditable = doc.create_element(QualName::html("div"));

        // Simulate IME input with range tracking (UI Events §5.2.5)
        doc.begin_composition(contenteditable, "c".to_string(), Some("ru".to_string()));
        assert!(doc.is_composing());
        assert_eq!(doc.get_composition_target(), Some(contenteditable));

        // User updates composition
        doc.update_composition("ч".to_string(), Some((0, 1)));
        assert_eq!(doc.get_composition_range(), Some((0, 1)));

        doc.update_composition("чт".to_string(), Some((0, 2)));
        assert_eq!(doc.get_composition_range(), Some((0, 2)));

        // Final commit
        let final_state = doc.end_composition();
        assert!(!doc.is_composing());
        assert!(final_state.is_some());
        assert_eq!(final_state.unwrap().text, "чт");
    }

    #[test]
    fn composition_event_dispatching_ready() {
        // Test CompositionEvent readiness for P3 dispatch (UI Events §5.2.5)
        // P3 will serialize these events to JS runtime

        // compositionstart event
        let start_evt = CompositionEvent::start("初".to_string(), Some("zh".to_string()));
        assert_eq!(start_evt.event_type, CompositionEventType::Start);
        assert_eq!(start_evt.event_type.as_str(), "compositionstart");
        assert_eq!(start_evt.data.data, "初");
        assert_eq!(start_evt.data.locale, Some("zh".to_string()));

        // compositionupdate events track user edits and cursor position
        let update1 = CompositionEvent::update("初".to_string(), Some((0, 1)));
        assert_eq!(update1.event_type.as_str(), "compositionupdate");
        assert_eq!(update1.data.range, Some((0, 1))); // cursor at offset 0, length 1

        let update2 = CompositionEvent::update("初中".to_string(), Some((0, 2)));
        assert_eq!(update2.data.data, "初中");
        assert_eq!(update2.data.range, Some((0, 2))); // preedit text spans 2 characters

        // compositionend event with final committed text
        let end_evt = CompositionEvent::end("初中文".to_string());
        assert_eq!(end_evt.event_type.as_str(), "compositionend");
        assert_eq!(end_evt.data.data, "初中文");
        assert_eq!(end_evt.data.range, None); // no range on final commit
    }

    #[test]
    fn composition_event_empty_data() {
        // Edge case: some IMEs send compositionstart with empty data
        let start_empty = CompositionEvent::start("".to_string(), Some("ja".to_string()));
        assert_eq!(start_empty.data.data, "");
        assert_eq!(start_empty.data.locale, Some("ja".to_string()));

        // compositionupdate may not have locale info
        let update = CompositionEvent::update("text".to_string(), Some((0, 4)));
        assert_eq!(update.data.locale, None);

        // compositionend may have empty data (commit cleared by IME)
        let end_empty = CompositionEvent::end("".to_string());
        assert_eq!(end_empty.data.data, "");
        assert_eq!(end_empty.data.range, None);
    }

    #[test]
    fn composition_multi_codepoint_range() {
        // Test range handling with multi-byte UTF-16 characters
        // Some characters (emoji, etc.) are 2 UTF-16 code units

        // surrogate pair emoji: 👍 = 2 UTF-16 code units
        let emoji_composition = CompositionEvent::update("👍".to_string(), Some((0, 2)));
        assert_eq!(emoji_composition.data.range, Some((0, 2)));

        // More complex: multiple characters with mixed widths
        let mixed = CompositionEvent::update("😀text😀".to_string(), Some((0, 6)));
        // emoji(2) + t(1) + e(1) + x(1) + t(1) + emoji(2) = 8 UTF-16 units
        assert_eq!(mixed.data.range, Some((0, 6)));
    }

    // ──────── FontFace and FontFaceSet ────────

    #[test]
    fn font_face_creation() {
        let face = FontFace::new(
            "Roboto".to_string(),
            "normal".to_string(),
            "400".to_string(),
            None,
            None,
            "url(\"roboto.ttf\")".to_string(),
        );

        assert_eq!(face.family, "Roboto");
        assert_eq!(face.style, "normal");
        assert_eq!(face.weight, "400");
        assert_eq!(face.stretch, None);
        assert_eq!(face.unicode_range, None);
        assert_eq!(face.src, "url(\"roboto.ttf\")");
        assert_eq!(face.status, FontFaceStatus::Unloaded);
    }

    #[test]
    fn font_face_with_properties() {
        let face = FontFace::new(
            "Inter".to_string(),
            "italic".to_string(),
            "700".to_string(),
            Some("condensed".to_string()),
            Some("U+0000-FFFF".to_string()),
            "url(\"inter-700i.woff2\") format(\"woff2\")".to_string(),
        );

        assert_eq!(face.family, "Inter");
        assert_eq!(face.style, "italic");
        assert_eq!(face.weight, "700");
        assert_eq!(face.stretch, Some("condensed".to_string()));
        assert_eq!(face.unicode_range, Some("U+0000-FFFF".to_string()));
    }

    #[test]
    fn font_face_set_add_and_size() {
        let mut set = FontFaceSet::new();
        assert_eq!(set.size(), 0);

        let face1 = FontFace::new(
            "Roboto".to_string(),
            "normal".to_string(),
            "400".to_string(),
            None,
            None,
            "url(\"roboto.ttf\")".to_string(),
        );
        set.add(face1);
        assert_eq!(set.size(), 1);

        let face2 = FontFace::new(
            "Roboto".to_string(),
            "normal".to_string(),
            "700".to_string(),
            None,
            None,
            "url(\"roboto-bold.ttf\")".to_string(),
        );
        set.add(face2);
        assert_eq!(set.size(), 2);
    }

    #[test]
    fn font_face_set_has_family() {
        let mut set = FontFaceSet::new();
        assert!(!set.has_family("Roboto"));

        let face = FontFace::new(
            "Roboto".to_string(),
            "normal".to_string(),
            "400".to_string(),
            None,
            None,
            "url(\"roboto.ttf\")".to_string(),
        );
        set.add(face);
        assert!(set.has_family("Roboto"));
        assert!(!set.has_family("Inter"));
    }

    #[test]
    fn font_face_set_get_by_family() {
        let mut set = FontFaceSet::new();

        let face1 = FontFace::new(
            "Roboto".to_string(),
            "normal".to_string(),
            "400".to_string(),
            None,
            None,
            "url(\"roboto-400.ttf\")".to_string(),
        );
        set.add(face1);

        let face2 = FontFace::new(
            "Roboto".to_string(),
            "normal".to_string(),
            "700".to_string(),
            None,
            None,
            "url(\"roboto-700.ttf\")".to_string(),
        );
        set.add(face2);

        let face3 = FontFace::new(
            "Inter".to_string(),
            "normal".to_string(),
            "400".to_string(),
            None,
            None,
            "url(\"inter-400.ttf\")".to_string(),
        );
        set.add(face3);

        let roboto_faces = set.get_by_family("Roboto");
        assert_eq!(roboto_faces.len(), 2);
        assert_eq!(roboto_faces[0].weight, "400");
        assert_eq!(roboto_faces[1].weight, "700");

        let inter_faces = set.get_by_family("Inter");
        assert_eq!(inter_faces.len(), 1);
        assert_eq!(inter_faces[0].weight, "400");

        let missing = set.get_by_family("Helvetica");
        assert_eq!(missing.len(), 0);
    }

    #[test]
    fn font_face_set_clear() {
        let mut set = FontFaceSet::new();

        let face = FontFace::new(
            "Roboto".to_string(),
            "normal".to_string(),
            "400".to_string(),
            None,
            None,
            "url(\"roboto.ttf\")".to_string(),
        );
        set.add(face);
        assert_eq!(set.size(), 1);

        set.clear();
        assert_eq!(set.size(), 0);
        assert!(!set.has_family("Roboto"));
    }

    #[test]
    fn document_fonts_collection() {
        let mut doc = Document::new();
        assert_eq!(doc.fonts().size(), 0);

        let face = FontFace::new(
            "CustomFont".to_string(),
            "normal".to_string(),
            "400".to_string(),
            None,
            None,
            "url(\"custom.ttf\")".to_string(),
        );
        doc.fonts_mut().add(face);

        assert_eq!(doc.fonts().size(), 1);
        assert!(doc.fonts().has_family("CustomFont"));
    }

    // ── Performance Timeline tests (W3C Performance Timeline §3) ──────────────

    #[test]
    fn performance_entry_creation() {
        let entry = PerformanceEntry::new(
            PerformanceEntryType::Mark,
            "myMark".to_string(),
            10.5,
            0.0,
        );
        assert_eq!(entry.entry_type, PerformanceEntryType::Mark);
        assert_eq!(entry.name, "myMark");
        assert_eq!(entry.start_time, 10.5);
        assert_eq!(entry.duration, 0.0);
        assert_eq!(entry.end_time(), 10.5);
    }

    #[test]
    fn performance_entry_end_time() {
        let entry = PerformanceEntry::new(
            PerformanceEntryType::Measure,
            "myMeasure".to_string(),
            100.0,
            50.0,
        );
        assert_eq!(entry.end_time(), 150.0);
    }

    #[test]
    fn performance_entries_add_and_retrieve() {
        let mut entries = PerformanceEntries::new();
        assert!(entries.is_empty());

        let mark = PerformanceEntry::new(
            PerformanceEntryType::Mark,
            "mark1".to_string(),
            10.0,
            0.0,
        );
        entries.add_entry(mark);
        assert_eq!(entries.len(), 1);
        assert!(!entries.is_empty());

        let all = entries.all();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].name, "mark1");
    }

    #[test]
    fn performance_entries_filter_by_type() {
        let mut entries = PerformanceEntries::new();

        let mark = PerformanceEntry::new(
            PerformanceEntryType::Mark,
            "mark1".to_string(),
            10.0,
            0.0,
        );
        entries.add_entry(mark);

        let measure = PerformanceEntry::new(
            PerformanceEntryType::Measure,
            "measure1".to_string(),
            10.0,
            25.0,
        );
        entries.add_entry(measure);

        let marks = entries.get_by_type(PerformanceEntryType::Mark);
        assert_eq!(marks.len(), 1);
        assert_eq!(marks[0].name, "mark1");

        let measures = entries.get_by_type(PerformanceEntryType::Measure);
        assert_eq!(measures.len(), 1);
        assert_eq!(measures[0].name, "measure1");

        let resources = entries.get_by_type(PerformanceEntryType::Resource);
        assert_eq!(resources.len(), 0);
    }

    #[test]
    fn performance_entries_filter_by_name() {
        let mut entries = PerformanceEntries::new();

        for i in 0..3 {
            let entry = PerformanceEntry::new(
                PerformanceEntryType::Mark,
                "mark1".to_string(),
                (i * 10) as f64,
                0.0,
            );
            entries.add_entry(entry);
        }

        let mark2 = PerformanceEntry::new(
            PerformanceEntryType::Mark,
            "mark2".to_string(),
            100.0,
            0.0,
        );
        entries.add_entry(mark2);

        let mark1s = entries.get_by_name("mark1");
        assert_eq!(mark1s.len(), 3);

        let mark2s = entries.get_by_name("mark2");
        assert_eq!(mark2s.len(), 1);

        let mark3s = entries.get_by_name("mark3");
        assert_eq!(mark3s.len(), 0);
    }

    #[test]
    fn performance_entries_first_by_name() {
        let mut entries = PerformanceEntries::new();

        let mark1 = PerformanceEntry::new(
            PerformanceEntryType::Mark,
            "myMark".to_string(),
            50.0,
            0.0,
        );
        entries.add_entry(mark1);

        let first = entries.get_first_by_name("myMark");
        assert!(first.is_some());
        assert_eq!(first.unwrap().start_time, 50.0);

        let missing = entries.get_first_by_name("missing");
        assert!(missing.is_none());
    }

    #[test]
    fn performance_entries_clear() {
        let mut entries = PerformanceEntries::new();

        let entry = PerformanceEntry::new(
            PerformanceEntryType::Mark,
            "mark1".to_string(),
            10.0,
            0.0,
        );
        entries.add_entry(entry);
        assert_eq!(entries.len(), 1);

        entries.clear();
        assert_eq!(entries.len(), 0);
        assert!(entries.is_empty());
    }

    #[test]
    fn document_mark() {
        let mut doc = Document::new();
        assert_eq!(doc.performance_entries().len(), 0);

        doc.mark("myMark".to_string(), Some(25.0));
        assert_eq!(doc.performance_entries().len(), 1);

        let entries = doc.performance_entries_by_type(PerformanceEntryType::Mark);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "myMark");
        assert_eq!(entries[0].start_time, 25.0);
        assert_eq!(entries[0].duration, 0.0);
    }

    #[test]
    fn document_measure_between_marks() {
        let mut doc = Document::new();

        // Create two marks
        doc.mark("start".to_string(), Some(100.0));
        doc.mark("end".to_string(), Some(150.0));

        // Measure between them
        let duration = doc.measure("myMeasure".to_string(), "start", "end");
        assert_eq!(duration, Some(50.0));

        let measures = doc.performance_entries_by_type(PerformanceEntryType::Measure);
        assert_eq!(measures.len(), 1);
        assert_eq!(measures[0].name, "myMeasure");
        assert_eq!(measures[0].duration, 50.0);
    }

    #[test]
    fn document_measure_missing_marks() {
        let mut doc = Document::new();

        doc.mark("start".to_string(), Some(100.0));

        // Try to measure with missing mark
        let duration = doc.measure("myMeasure".to_string(), "start", "missing");
        assert_eq!(duration, None);

        // No measure should be created
        let measures = doc.performance_entries_by_type(PerformanceEntryType::Measure);
        assert_eq!(measures.len(), 0);
    }

    #[test]
    fn document_performance_entries_by_name() {
        let mut doc = Document::new();

        doc.mark("mark1".to_string(), Some(10.0));
        doc.mark("mark1".to_string(), Some(20.0));
        doc.mark("mark2".to_string(), Some(30.0));

        let mark1s = doc.performance_entries_by_name("mark1");
        assert_eq!(mark1s.len(), 2);

        let mark2s = doc.performance_entries_by_name("mark2");
        assert_eq!(mark2s.len(), 1);

        let mark3s = doc.performance_entries_by_name("mark3");
        assert_eq!(mark3s.len(), 0);
    }

    #[test]
    fn document_clear_performance_entries() {
        let mut doc = Document::new();

        doc.mark("mark1".to_string(), Some(10.0));
        doc.mark("mark2".to_string(), Some(20.0));
        assert_eq!(doc.performance_entries().len(), 2);

        doc.clear_performance_entries();
        assert_eq!(doc.performance_entries().len(), 0);
    }

    #[test]
    fn performance_observer_creation() {
        let observer = PerformanceObserver::new();
        assert!(observer.handle().is_none());
        assert_eq!(observer.observed_types().len(), 0);
    }

    #[test]
    fn performance_observer_observe() {
        let mut observer = PerformanceObserver::new();
        observer.observe(vec![PerformanceEntryType::Mark, PerformanceEntryType::Measure]);

        assert_eq!(observer.observed_types().len(), 2);
        assert!(observer.is_observing(PerformanceEntryType::Mark));
        assert!(observer.is_observing(PerformanceEntryType::Measure));
        assert!(!observer.is_observing(PerformanceEntryType::Resource));
    }

    #[test]
    fn performance_observer_disconnect() {
        let mut observer = PerformanceObserver::new();
        observer.observe(vec![PerformanceEntryType::Mark]);
        observer.set_handle(123);

        assert!(observer.handle().is_some());
        assert_eq!(observer.handle(), Some(123));

        observer.disconnect();
        assert!(observer.handle().is_none());
        assert_eq!(observer.observed_types().len(), 0);
    }

    #[test]
    fn performance_entry_type_display() {
        assert_eq!(PerformanceEntryType::Mark.to_string(), "mark");
        assert_eq!(PerformanceEntryType::Measure.to_string(), "measure");
        assert_eq!(PerformanceEntryType::Navigation.to_string(), "navigation");
        assert_eq!(PerformanceEntryType::Resource.to_string(), "resource");
        assert_eq!(PerformanceEntryType::Paint.to_string(), "paint");
        assert_eq!(PerformanceEntryType::Layout.to_string(), "layout");
    }

    #[test]
    fn input_mode_parse_basic() {
        assert_eq!(InputMode::parse("text"), InputMode::Text);
        assert_eq!(InputMode::parse("none"), InputMode::None);
        assert_eq!(InputMode::parse("decimal"), InputMode::Decimal);
        assert_eq!(InputMode::parse("numeric"), InputMode::Numeric);
        assert_eq!(InputMode::parse("tel"), InputMode::Tel);
        assert_eq!(InputMode::parse("search"), InputMode::Search);
        assert_eq!(InputMode::parse("email"), InputMode::Email);
        assert_eq!(InputMode::parse("url"), InputMode::Url);
    }

    #[test]
    fn input_mode_parse_case_insensitive() {
        assert_eq!(InputMode::parse("TEXT"), InputMode::Text);
        assert_eq!(InputMode::parse("NONE"), InputMode::None);
        assert_eq!(InputMode::parse("DeCiMaL"), InputMode::Decimal);
        assert_eq!(InputMode::parse("NumErIc"), InputMode::Numeric);
    }

    #[test]
    fn input_mode_parse_whitespace_trim() {
        assert_eq!(InputMode::parse("  text  "), InputMode::Text);
        assert_eq!(InputMode::parse("\n  email\t"), InputMode::Email);
    }

    #[test]
    fn input_mode_parse_unknown_default_to_text() {
        assert_eq!(InputMode::parse("unknown"), InputMode::Text);
        assert_eq!(InputMode::parse("random"), InputMode::Text);
        assert_eq!(InputMode::parse(""), InputMode::Text);
    }

    #[test]
    fn input_mode_as_str() {
        assert_eq!(InputMode::Text.as_str(), "text");
        assert_eq!(InputMode::None.as_str(), "none");
        assert_eq!(InputMode::Decimal.as_str(), "decimal");
        assert_eq!(InputMode::Numeric.as_str(), "numeric");
        assert_eq!(InputMode::Tel.as_str(), "tel");
        assert_eq!(InputMode::Search.as_str(), "search");
        assert_eq!(InputMode::Email.as_str(), "email");
        assert_eq!(InputMode::Url.as_str(), "url");
    }

    #[test]
    fn node_input_mode_for_input_element() {
        let mut doc = Document::new();

        // <input inputmode="email">
        let input_email = doc.create_element(QualName::html("input"));
        doc.get_mut(input_email).data = NodeData::Element {
            name: QualName::html("input"),
            attrs: vec![Attribute {
                name: QualName::html("inputmode"),
                value: "email".to_string(),
            }],
        };

        assert_eq!(doc.get(input_email).input_mode(), Some(InputMode::Email));

        // <input inputmode="numeric">
        let input_numeric = doc.create_element(QualName::html("input"));
        doc.get_mut(input_numeric).data = NodeData::Element {
            name: QualName::html("input"),
            attrs: vec![Attribute {
                name: QualName::html("inputmode"),
                value: "numeric".to_string(),
            }],
        };

        assert_eq!(
            doc.get(input_numeric).input_mode(),
            Some(InputMode::Numeric)
        );
    }

    #[test]
    fn node_input_mode_for_textarea_element() {
        let mut doc = Document::new();

        // <textarea inputmode="url">
        let textarea = doc.create_element(QualName::html("textarea"));
        doc.get_mut(textarea).data = NodeData::Element {
            name: QualName::html("textarea"),
            attrs: vec![Attribute {
                name: QualName::html("inputmode"),
                value: "url".to_string(),
            }],
        };

        assert_eq!(doc.get(textarea).input_mode(), Some(InputMode::Url));
    }

    #[test]
    fn node_input_mode_default_to_text_when_absent() {
        let mut doc = Document::new();

        // <input> without inputmode attribute
        let input = doc.create_element(QualName::html("input"));
        doc.get_mut(input).data = NodeData::Element {
            name: QualName::html("input"),
            attrs: vec![],
        };

        assert_eq!(doc.get(input).input_mode(), Some(InputMode::Text));
    }

    #[test]
    fn node_input_mode_none_for_other_elements() {
        let mut doc = Document::new();

        // <div inputmode="email"> should return None
        let div = doc.create_element(QualName::html("div"));
        doc.get_mut(div).data = NodeData::Element {
            name: QualName::html("div"),
            attrs: vec![Attribute {
                name: QualName::html("inputmode"),
                value: "email".to_string(),
            }],
        };

        assert_eq!(doc.get(div).input_mode(), None);
    }

    // ── GC integration tests ───────────────────────────────────────────────────

    #[test]
    fn gc_acquire_increments_ref_count() {
        let mut doc = Document::new();
        let div = doc.create_element(QualName::html("div"));

        assert_eq!(doc.js_ref_count(div), 0);
        assert_eq!(doc.acquire_js_ref(div), 1);
        assert_eq!(doc.js_ref_count(div), 1);
        assert_eq!(doc.acquire_js_ref(div), 2);
        assert_eq!(doc.js_ref_count(div), 2);
    }

    #[test]
    fn gc_release_decrements_ref_count() {
        let mut doc = Document::new();
        let div = doc.create_element(QualName::html("div"));

        doc.acquire_js_ref(div);
        doc.acquire_js_ref(div);
        assert_eq!(doc.release_js_ref(div), 1);
        assert_eq!(doc.release_js_ref(div), 0);
        assert_eq!(doc.js_ref_count(div), 0);
    }

    #[test]
    fn gc_release_on_zero_ref_is_noop() {
        let mut doc = Document::new();
        let div = doc.create_element(QualName::html("div"));

        // release without prior acquire must not panic
        assert_eq!(doc.release_js_ref(div), 0);
        assert_eq!(doc.js_ref_count(div), 0);
    }

    #[test]
    fn gc_node_in_tree_is_not_detached() {
        let mut doc = Document::new();
        let div = doc.create_element(QualName::html("div"));
        doc.append_child(doc.root(), div);

        assert!(!doc.is_detached(div));
    }

    #[test]
    fn gc_orphan_node_is_detached() {
        let mut doc = Document::new();
        // Created but never appended
        let div = doc.create_element(QualName::html("div"));
        assert!(doc.is_detached(div));
    }

    #[test]
    fn gc_detached_node_with_zero_refs_is_dead() {
        let mut doc = Document::new();
        let div = doc.create_element(QualName::html("div"));
        // Never appended, zero JS refs → dead
        let dead = doc.dead_node_ids();
        assert!(dead.contains(&div));
    }

    #[test]
    fn gc_detached_node_with_live_ref_is_not_dead() {
        let mut doc = Document::new();
        let div = doc.create_element(QualName::html("div"));
        doc.acquire_js_ref(div);

        let dead = doc.dead_node_ids();
        assert!(!dead.contains(&div));
    }

    #[test]
    fn gc_node_in_tree_is_not_dead_even_with_zero_refs() {
        let mut doc = Document::new();
        let div = doc.create_element(QualName::html("div"));
        doc.append_child(doc.root(), div);

        let dead = doc.dead_node_ids();
        assert!(!dead.contains(&div));
    }

    #[test]
    fn gc_remove_from_tree_then_release_ref_makes_dead() {
        let mut doc = Document::new();
        let div = doc.create_element(QualName::html("div"));
        doc.append_child(doc.root(), div);
        doc.acquire_js_ref(div);

        // Remove from tree but JS still holds a ref
        doc.detach(div);
        assert!(!doc.dead_node_ids().contains(&div), "still has a JS ref");

        // JS finalizer fires
        doc.release_js_ref(div);
        assert!(doc.dead_node_ids().contains(&div), "now collectable");
    }

    #[test]
    fn gc_root_is_never_dead() {
        let doc = Document::new();
        let dead = doc.dead_node_ids();
        assert!(!dead.contains(&doc.root()));
    }

    #[test]
    fn gc_shadow_root_is_not_dead() {
        let mut doc = Document::new();
        let host = doc.create_element(QualName::html("div"));
        doc.append_child(doc.root(), host);
        let shadow_root = doc.attach_shadow(host, ShadowRootMode::Open);

        // Even with zero JS refs, shadow root must not be collected
        let dead = doc.dead_node_ids();
        assert!(!dead.contains(&shadow_root));
    }

    // ── DOM node count / limit tests ──────────────────────────────────────────

    #[test]
    fn node_count_returns_arena_length() {
        let mut doc = Document::new();
        let before = doc.node_count();
        doc.create_element(QualName::html("div"));
        assert_eq!(doc.node_count(), before + 1);
    }

    #[test]
    fn try_create_element_ok_below_limit() {
        let mut doc = Document::new();
        let result = doc.try_create_element(QualName::html("span"));
        assert!(result.is_ok());
    }

    #[test]
    fn try_create_element_err_at_limit() {
        let mut doc = Document::new();
        // Fill arena to exactly MAX_DOM_NODES nodes.
        while doc.node_count() < MAX_DOM_NODES {
            doc.create_element(QualName::html("div"));
        }
        assert_eq!(doc.node_count(), MAX_DOM_NODES);
        let result = doc.try_create_element(QualName::html("p"));
        assert_eq!(result, Err(NodeLimitExceeded));
    }

    #[test]
    fn try_create_text_ok_below_limit() {
        let mut doc = Document::new();
        assert!(doc.try_create_text("hi").is_ok());
    }

    #[test]
    fn try_create_text_err_at_limit() {
        let mut doc = Document::new();
        while doc.node_count() < MAX_DOM_NODES {
            doc.create_element(QualName::html("div"));
        }
        assert_eq!(doc.try_create_text("hi"), Err(NodeLimitExceeded));
    }

    #[test]
    fn try_create_comment_ok_below_limit() {
        let mut doc = Document::new();
        assert!(doc.try_create_comment("hi").is_ok());
    }

    #[test]
    fn try_create_comment_err_at_limit() {
        let mut doc = Document::new();
        while doc.node_count() < MAX_DOM_NODES {
            doc.create_element(QualName::html("div"));
        }
        assert_eq!(doc.try_create_comment("hi"), Err(NodeLimitExceeded));
    }

    #[test]
    fn node_limit_exceeded_display() {
        let msg = NodeLimitExceeded.to_string();
        assert!(msg.contains("50000"), "display should mention MAX_DOM_NODES");
    }

    #[test]
    fn warn_threshold_less_than_max() {
        const { assert!(WARN_DOM_NODES < MAX_DOM_NODES) };
    }

    // ── is_element_draggable ──────────────────────────────────────────────────

    /// Create a one-element document and return (doc, element_id).
    fn make_elem(tag: &str, attrs: &[(&str, &str)]) -> (Document, NodeId) {
        let mut doc = Document::new();
        let elem = doc.create_element(QualName::html(tag));
        for (name, val) in attrs {
            let node = doc.get_mut(elem);
            if let NodeData::Element { attrs: a, .. } = &mut node.data {
                a.push(Attribute {
                    name: QualName::html(*name),
                    value: val.to_string(),
                });
            }
        }
        doc.append_child(doc.root(), elem);
        (doc, elem)
    }

    #[test]
    fn draggable_true_makes_element_draggable() {
        let (doc, node) = make_elem("div", &[("draggable", "true")]);
        assert!(is_element_draggable(&doc, node));
    }

    #[test]
    fn draggable_false_prevents_drag_on_img() {
        let (doc, node) = make_elem("img", &[("draggable", "false"), ("src", "x.png")]);
        assert!(!is_element_draggable(&doc, node));
    }

    #[test]
    fn img_is_draggable_by_default() {
        let (doc, node) = make_elem("img", &[("src", "photo.jpg")]);
        assert!(is_element_draggable(&doc, node));
    }

    #[test]
    fn anchor_with_href_is_draggable() {
        let (doc, node) = make_elem("a", &[("href", "https://example.com")]);
        assert!(is_element_draggable(&doc, node));
    }

    #[test]
    fn anchor_without_href_is_not_draggable() {
        let (doc, node) = make_elem("a", &[]);
        assert!(!is_element_draggable(&doc, node));
    }

    #[test]
    fn plain_div_is_not_draggable() {
        let (doc, node) = make_elem("div", &[]);
        assert!(!is_element_draggable(&doc, node));
    }

    #[test]
    fn draggable_case_insensitive() {
        let (doc, node) = make_elem("div", &[("DRAGGABLE", "TRUE")]);
        assert!(is_element_draggable(&doc, node));
    }

    // ──────── locate_text_offset_range ────────

    #[test]
    fn locate_text_offset_range_single_child() {
        let mut doc = Document::new();
        let host = doc.create_element(QualName::html("div"));
        let text = doc.create_text("hello wrold");
        doc.append_child(doc.root(), host);
        doc.append_child(host, text);

        let found = locate_text_offset_range(&doc, host, 6, 11).unwrap();
        assert_eq!(found, (text, 6, 11));
    }

    #[test]
    fn locate_text_offset_range_across_nested_elements() {
        // <div>hello <b>wrold</b> there</div> — textContent = "hello wrold there".
        let mut doc = Document::new();
        let host = doc.create_element(QualName::html("div"));
        let t1 = doc.create_text("hello ");
        let bold = doc.create_element(QualName::html("b"));
        let t2 = doc.create_text("wrold");
        let t3 = doc.create_text(" there");
        doc.append_child(doc.root(), host);
        doc.append_child(host, t1);
        doc.append_child(host, bold);
        doc.append_child(bold, t2);
        doc.append_child(host, t3);

        assert_eq!(node_text_content(&doc, host), "hello wrold there");

        // "wrold" sits at global offset 6..11, inside t2 (nested under <b>).
        let found = locate_text_offset_range(&doc, host, 6, 11).unwrap();
        assert_eq!(found, (t2, 0, 5));
    }

    #[test]
    fn locate_text_offset_range_none_when_crossing_nodes() {
        let mut doc = Document::new();
        let host = doc.create_element(QualName::html("div"));
        let t1 = doc.create_text("ab");
        let t2 = doc.create_text("cd");
        doc.append_child(doc.root(), host);
        doc.append_child(host, t1);
        doc.append_child(host, t2);

        // Range 1..3 spans "b" (end of t1) + "c" (start of t2) — crosses a boundary.
        assert!(locate_text_offset_range(&doc, host, 1, 3).is_none());
    }

    #[test]
    fn locate_text_offset_range_out_of_bounds() {
        let (doc, div, _text) = make_text_doc("abc");
        assert!(locate_text_offset_range(&doc, div, 10, 12).is_none());
    }
}
