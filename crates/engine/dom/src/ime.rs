use super::*;

/// Tracks the current IME composition session.
///
/// When an IME begins composing (e.g., CJK text input), this struct stores the
/// target node and interim text until the composition ends and the final text
/// is committed. Maintained by P1 (Document::begin/update/end_composition).
///
/// **P3 integration:** P3 shell receives IME events from the platform (winit) and
/// calls Document methods to maintain this state. When composition changes, P3
/// dispatches corresponding CompositionEvent(s) to the target node's event listeners.
///
/// **Layout integration:** P1's layout engine uses get_composition() to highlight
/// the preedit range with an underline or background, per UI Events §5.2.5.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompositionState {
    /// The editable element (contenteditable/input/textarea) receiving IME input.
    pub node: NodeId,
    /// The interim (preedit) composition text being edited by the IME.
    pub text: String,
    /// BCP 47 language tag (e.g., "ja", "zh-Hans"). `None` if not available.
    pub locale: Option<String>,
    /// Selection range in UTF-16 code units: (offset, length).
    /// Describes which part of the preedit text is highlighted (e.g., current
    /// candidate). Allows JS to highlight the composition range as the user types.
    pub selection: Option<(u32, u32)>,
}

/// Input event type per Input Events Level 2 §4.1.3.
///
/// Each variant corresponds to one `inputType` string value that is dispatched
/// with `beforeinput`/`input` events on a `contenteditable` host. P3 maps
/// keyboard events to these variants; P1 uses them only as data types for the
/// DOM mutation API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditInputType {
    /// A printable character was typed (regular key press in contenteditable).
    InsertText,
    /// Enter key — split the current block into two paragraphs.
    InsertParagraph,
    /// Shift+Enter — insert a `<br>` line break without splitting a block.
    InsertLineBreak,
    /// Backspace — delete one character/grapheme cluster before the caret.
    DeleteContentBackward,
    /// Delete — delete one character/grapheme cluster after the caret.
    DeleteContentForward,
    /// Ctrl+Backspace — delete one word before the caret.
    DeleteWordBackward,
    /// Ctrl+Delete — delete one word after the caret.
    DeleteWordForward,
    /// Ctrl+V / drag-and-drop — insert content from the clipboard.
    InsertFromPaste,
    /// Ctrl+X — delete selected content and copy to clipboard.
    DeleteByCut,
    /// Ctrl+A — select all content (no DOM mutation).
    SelectAll,
    /// Ctrl+Z — undo the previous edit (managed by P3 undo stack).
    HistoryUndo,
    /// Ctrl+Y / Ctrl+Shift+Z — redo (managed by P3 undo stack).
    HistoryRedo,
}

impl EditInputType {
    /// The canonical `inputType` string for the `InputEvent` interface.
    ///
    /// Values match the Input Events Level 2 §4.1.3 enumeration.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InsertText => "insertText",
            Self::InsertParagraph => "insertParagraph",
            Self::InsertLineBreak => "insertLineBreak",
            Self::DeleteContentBackward => "deleteContentBackward",
            Self::DeleteContentForward => "deleteContentForward",
            Self::DeleteWordBackward => "deleteWordBackward",
            Self::DeleteWordForward => "deleteWordForward",
            Self::InsertFromPaste => "insertFromPaste",
            Self::DeleteByCut => "deleteByCut",
            Self::SelectAll => "selectAll",
            Self::HistoryUndo => "historyUndo",
            Self::HistoryRedo => "historyRedo",
        }
    }
}

/// Data for a `beforeinput` or `input` DOM event (Input Events Level 2 §4.1).
///
/// P3 constructs this from keyboard / clipboard events and dispatches it to the
/// JS runtime and the mutation functions below.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputEvent {
    /// The kind of edit operation that caused this event.
    pub input_type: EditInputType,
    /// Text that was inserted, or `None` for deletion/selection operations.
    pub data: Option<String>,
    /// `true` while an IME composition is in progress (not yet committed).
    pub is_composing: bool,
    /// `Event.isTrusted` per DOM §2.10: `true` for events dispatched by the
    /// user agent in response to real user input (native input pipeline from
    /// the shell, automation `BrowserSession::type_text`/`paste`); `false` for
    /// events synthesized by page scripts via `new InputEvent(...)` and
    /// `dispatchEvent`. Used by sites to distinguish bots/script automation
    /// from genuine input (e.g., gating form submission on `isTrusted`).
    pub is_trusted: bool,
}

impl InputEvent {
    /// Construct a trusted input event (native input pipeline or automation
    /// `BrowserSession`). Equivalent to setting `is_trusted = true`.
    pub fn trusted(input_type: EditInputType, data: Option<String>, is_composing: bool) -> Self {
        Self {
            input_type,
            data,
            is_composing,
            is_trusted: true,
        }
    }

    /// Construct an untrusted input event (synthesized by page script via
    /// `dispatchEvent`). Equivalent to setting `is_trusted = false`.
    pub fn untrusted(input_type: EditInputType, data: Option<String>, is_composing: bool) -> Self {
        Self {
            input_type,
            data,
            is_composing,
            is_trusted: false,
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// IME Composition Events (UI Events §5.2.5 — for CJK/Cyrillic input)
// ──────────────────────────────────────────────────────────────────────────────

/// Type of IME composition event (UI Events §5.2.5).
///
/// While an IME is composing (e.g., Japanese input), the sequence is:
/// 1. `compositionstart` — IME began capturing input
/// 2. Zero or more `compositionupdate` — interim composition text shown to user
/// 3. `compositionend` — IME committed the final text (may differ from last update)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompositionEventType {
    /// IME composition began.
    Start,
    /// Interim composition text changed.
    Update,
    /// IME composition finished and text was committed.
    End,
}

impl CompositionEventType {
    /// The canonical DOM event name per UI Events §5.2.5.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Start => "compositionstart",
            Self::Update => "compositionupdate",
            Self::End => "compositionend",
        }
    }
}

/// Data for a `compositionstart` / `compositionupdate` / `compositionend` event.
///
/// P3 constructs this from winit IME callbacks (Ime::Preedit, Ime::Commit) and
/// dispatches to the JS runtime. P1 maintains composition state and ranges.
#[derive(Debug, Clone)]
pub struct CompositionData {
    /// The interim (preedit) or final (committed) composition text.
    /// Empty string for events where only metadata changed.
    pub data: String,
    /// BCP 47 language tag (e.g., "ja", "zh-Hans", "ru"). `None` if unknown.
    pub locale: Option<String>,
    /// Composition range in the contenteditable host (UTF-16 code units).
    /// `None` if the range cannot be determined.
    /// `(offset, length)` where offset is the start position and length is the number
    /// of characters in the composition range.
    pub range: Option<(u32, u32)>,
}

/// An IME composition event (compositionstart / update / end).
///
/// **P3 lifecycle:** P3 receives IME callbacks from winit (Ime::Preedit, Ime::Commit)
/// and constructs CompositionEvent(s) to dispatch to the target node's listeners.
/// - `compositionstart` → when IME begins (Document::begin_composition called)
/// - `compositionupdate` → interim preedit changes (Document::update_composition)
/// - `compositionend` → final commit (Document::end_composition returns state)
///
/// **JS exposure:** P3 serializes these events to the JS runtime, where JS can
/// listen via `element.addEventListener("compositionstart", ...)`.
///
/// **Range semantics:** (offset, length) in UTF-16 code units, matching platform
/// IME conventions and JS TextEvent / UIEvent.
#[derive(Debug, Clone)]
pub struct CompositionEvent {
    /// The kind of composition event.
    pub event_type: CompositionEventType,
    /// Composition data (text, language, selection range).
    pub data: CompositionData,
    /// `Event.isTrusted` per DOM §2.10: `true` for composition events produced
    /// by the platform IME (winit `Ime::Preedit`/`Ime::Commit` callbacks routed
    /// through the native input pipeline) or by automation
    /// `BrowserSession::compose_text`; `false` for events synthesized by page
    /// scripts via `new CompositionEvent(...)` and `dispatchEvent`. Sites that
    /// gate on `isTrusted` to reject script-driven IME use this flag.
    pub is_trusted: bool,
}

impl CompositionEvent {
    /// Create a new trusted composition event (native IME pipeline).
    ///
    /// Sets `is_trusted = true`. For events synthesized from page script use
    /// [`CompositionEvent::untrusted`] instead.
    pub fn new(event_type: CompositionEventType, data: CompositionData) -> Self {
        Self {
            event_type,
            data,
            is_trusted: true,
        }
    }

    /// Create an untrusted composition event (synthesized by page script).
    ///
    /// Sets `is_trusted = false`, matching `new CompositionEvent(...)` in JS
    /// per DOM §2.10 (events constructed by script are never trusted).
    pub fn untrusted(event_type: CompositionEventType, data: CompositionData) -> Self {
        Self {
            event_type,
            data,
            is_trusted: false,
        }
    }

    /// Create a `compositionstart` event with initial IME text.
    ///
    /// Trusted by default (native IME pipeline).
    pub fn start(data: String, locale: Option<String>) -> Self {
        Self {
            event_type: CompositionEventType::Start,
            data: CompositionData {
                data,
                locale,
                range: None,
            },
            is_trusted: true,
        }
    }

    /// Create a `compositionupdate` event for interim preedit text.
    ///
    /// Trusted by default (native IME pipeline).
    pub fn update(data: String, range: Option<(u32, u32)>) -> Self {
        Self {
            event_type: CompositionEventType::Update,
            data: CompositionData {
                data,
                locale: None,
                range,
            },
            is_trusted: true,
        }
    }

    /// Create a `compositionend` event for final committed text.
    ///
    /// Trusted by default (native IME pipeline).
    pub fn end(data: String) -> Self {
        Self {
            event_type: CompositionEventType::End,
            data: CompositionData {
                data,
                locale: None,
                range: None,
            },
            is_trusted: true,
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// DOM mutation helpers for contenteditable editing
// ──────────────────────────────────────────────────────────────────────────────

impl Document {
    // ── IME Composition state management ──────────────────────────────────────

    /// Begin a new IME composition session in the given editable element.
    ///
    /// Overwrites any existing composition. Called by P3 when `compositionstart`
    /// is received from winit.
    ///
    /// # Arguments
    /// * `node` — the contenteditable/input/textarea receiving IME input
    /// * `text` — initial composition text (may be empty)
    /// * `locale` — BCP 47 language tag, or `None` if not available
    pub fn begin_composition(&mut self, node: NodeId, text: String, locale: Option<String>) {
        self.composition = Some(CompositionState {
            node,
            text,
            locale,
            selection: None,
        });
    }

    /// Update the active composition with new preedit text and selection range.
    ///
    /// Called by P3 when `compositionupdate` is received from winit.
    /// No-op if no composition is active.
    ///
    /// # Arguments
    /// * `text` — new interim composition text
    /// * `selection` — UTF-16 offset and length of the composition range shown to user
    pub fn update_composition(&mut self, text: String, selection: Option<(u32, u32)>) {
        if let Some(comp) = &mut self.composition {
            comp.text = text;
            comp.selection = selection;
        }
    }

    /// End the active composition and return its final state.
    ///
    /// Called by P3 when `compositionend` is received from winit. Returns the
    /// composition state so P3 can construct the final text event (with the
    /// committed text from the IME).
    ///
    /// Returns `None` if no composition is active.
    pub fn end_composition(&mut self) -> Option<CompositionState> {
        self.composition.take()
    }

    /// Get the current composition state without removing it.
    ///
    /// Used by layout and JS to inspect the active composition (e.g., for
    /// drawing selection underlines in the preedit range).
    ///
    /// Returns `None` if no composition is active.
    pub fn get_composition(&self) -> Option<&CompositionState> {
        self.composition.as_ref()
    }

    /// Check if an IME composition is currently active.
    ///
    /// Returns `true` if `begin_composition` has been called and
    /// `end_composition` has not yet been called.
    pub fn is_composing(&self) -> bool {
        self.composition.is_some()
    }

    /// Get the composition range (offset and length) if composition is active.
    ///
    /// Returns `None` if no composition is active or if the selection range
    /// has not been set yet.
    pub fn get_composition_range(&self) -> Option<(u32, u32)> {
        self.composition.as_ref().and_then(|comp| comp.selection)
    }

    /// Get the target node that is receiving composition input.
    ///
    /// Returns `None` if no composition is active. This is the element that
    /// should dispatch composition events (compositionstart/update/end).
    pub fn get_composition_target(&self) -> Option<NodeId> {
        self.composition.as_ref().map(|comp| comp.node)
    }
}
