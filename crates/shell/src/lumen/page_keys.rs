//! Physical keys on their way to the page (UX-PAGE-KEYDOWN).
//!
//! A real key press reaches the page as a trusted `keydown` *before* the
//! browser acts on it, and the page answers synchronously: `preventDefault()`
//! cancels the browser's own action (scrolling on Space, the `F` hint mode, a
//! character typed into a field). A few chords belong to the browser alone and
//! are never offered to the page ([`is_reserved`], UI Events §3.6 "keyboard
//! events are not dispatched for browser-reserved shortcuts"). The matching
//! `keyup` follows the release of every key whose `keydown` was delivered.
//!
//! The first half is the pure part: the winit → DOM `KeyboardEvent` mapping
//! and the dispatch script. The `Lumen` methods below send it; the call sites
//! are in [`super::keyboard`].

use crate::*;
use winit::keyboard::{Key, KeyLocation, NamedKey};

/// `KeyboardEvent.key` of a winit logical key (UI Events Key Values).
pub(crate) fn dom_key(key: &Key) -> String {
    match key {
        Key::Character(s) => s.to_string(),
        Key::Named(NamedKey::Space) => " ".to_owned(),
        Key::Named(NamedKey::Super) => "Meta".to_owned(),
        Key::Named(named) => format!("{named:?}"),
        Key::Dead(_) => "Dead".to_owned(),
        Key::Unidentified(_) => "Unidentified".to_owned(),
    }
}

/// `KeyboardEvent.code` of a physical key. winit names its key codes after the
/// W3C ones; only the Windows key differs (`Super*` there, `Meta*` in the DOM).
pub(crate) fn dom_code(code: KeyCode) -> String {
    match code {
        KeyCode::SuperLeft => "MetaLeft".to_owned(),
        KeyCode::SuperRight => "MetaRight".to_owned(),
        other => format!("{other:?}"),
    }
}

/// Legacy `KeyboardEvent.keyCode` (the Windows virtual-key value pages still
/// compare against); `0` for keys without one.
pub(crate) fn legacy_key_code(code: KeyCode) -> u32 {
    let name = dom_code(code);
    if let Some(letter) = name.strip_prefix("Key").and_then(|s| s.chars().next()) {
        return u32::from(letter.to_ascii_uppercase());
    }
    if let Some(digit) = name.strip_prefix("Digit").and_then(|s| s.chars().next()) {
        return u32::from(digit);
    }
    if let Some(n) = name.strip_prefix("Numpad").and_then(|s| s.parse::<u32>().ok()) {
        return 96 + n;
    }
    if let Some(n) = name.strip_prefix('F').and_then(|s| s.parse::<u32>().ok())
        && (1..=24).contains(&n)
    {
        return 111 + n;
    }
    match code {
        KeyCode::Backspace => 8,
        KeyCode::Tab => 9,
        KeyCode::Enter | KeyCode::NumpadEnter => 13,
        KeyCode::ShiftLeft | KeyCode::ShiftRight => 16,
        KeyCode::ControlLeft | KeyCode::ControlRight => 17,
        KeyCode::AltLeft | KeyCode::AltRight => 18,
        KeyCode::Pause => 19,
        KeyCode::CapsLock => 20,
        KeyCode::Escape => 27,
        KeyCode::Space => 32,
        KeyCode::PageUp => 33,
        KeyCode::PageDown => 34,
        KeyCode::End => 35,
        KeyCode::Home => 36,
        KeyCode::ArrowLeft => 37,
        KeyCode::ArrowUp => 38,
        KeyCode::ArrowRight => 39,
        KeyCode::ArrowDown => 40,
        KeyCode::PrintScreen => 44,
        KeyCode::Insert => 45,
        KeyCode::Delete => 46,
        KeyCode::SuperLeft | KeyCode::SuperRight => 91,
        KeyCode::ContextMenu => 93,
        KeyCode::NumpadMultiply => 106,
        KeyCode::NumpadAdd => 107,
        KeyCode::NumpadSubtract => 109,
        KeyCode::NumpadDecimal => 110,
        KeyCode::NumpadDivide => 111,
        KeyCode::NumLock => 144,
        KeyCode::ScrollLock => 145,
        KeyCode::Semicolon => 186,
        KeyCode::Equal => 187,
        KeyCode::Comma => 188,
        KeyCode::Minus => 189,
        KeyCode::Period => 190,
        KeyCode::Slash => 191,
        KeyCode::Backquote => 192,
        KeyCode::BracketLeft => 219,
        KeyCode::Backslash => 220,
        KeyCode::BracketRight => 221,
        KeyCode::Quote => 222,
        _ => 0,
    }
}

/// `KeyboardEvent.location`: 0 standard, 1 left, 2 right, 3 numpad.
pub(crate) fn dom_location(location: KeyLocation) -> u32 {
    match location {
        KeyLocation::Standard => 0,
        KeyLocation::Left => 1,
        KeyLocation::Right => 2,
        KeyLocation::Numpad => 3,
    }
}

/// Modifier bit mask of `_lumen_dispatch_key_event`: ctrl 1, shift 2, alt 4,
/// meta 8.
pub(crate) fn modifier_mask(mods: ModifiersState) -> u32 {
    u32::from(mods.control_key())
        | u32::from(mods.shift_key()) << 1
        | u32::from(mods.alt_key()) << 2
        | u32::from(mods.super_key()) << 3
}

/// Chords the browser keeps for itself: tab and window management. The page
/// sees neither their `keydown` nor their `keyup`, so it can neither block
/// closing a tab nor learn that the user did it.
pub(crate) fn is_reserved(code: KeyCode, mods: ModifiersState) -> bool {
    let ctrl = mods.control_key();
    let shift = mods.shift_key();
    let alt = mods.alt_key();
    match code {
        KeyCode::KeyW | KeyCode::KeyT | KeyCode::KeyN => ctrl && !alt,
        KeyCode::Tab | KeyCode::PageUp | KeyCode::PageDown => ctrl && !alt,
        KeyCode::F4 => alt || ctrl,
        KeyCode::Digit1
        | KeyCode::Digit2
        | KeyCode::Digit3
        | KeyCode::Digit4
        | KeyCode::Digit5
        | KeyCode::Digit6
        | KeyCode::Digit7
        | KeyCode::Digit8
        | KeyCode::Digit9 => ctrl && !alt && !shift,
        _ => false,
    }
}

/// The page-side script that dispatches a trusted keyboard event at node
/// `nid`; it evaluates to `false` when a handler called `preventDefault()`.
pub(crate) fn dispatch_script(nid: usize, event_type: &str, event: &KeyEvent, mods: ModifiersState) -> String {
    let PhysicalKeyParts { code, key_code } = physical_parts(event);
    format!(
        "_lumen_dispatch_key_event({nid}, '{event_type}', '{}', '{}', {key_code}, {}, {}, {}, false)",
        crate::js_escape::escape_js_string(&dom_key(&event.logical_key)),
        crate::js_escape::escape_js_string(&code),
        dom_location(event.location),
        modifier_mask(mods),
        event.repeat,
    )
}

/// `code` string and legacy `keyCode` of the physical key of an event.
struct PhysicalKeyParts {
    code: String,
    key_code: u32,
}

fn physical_parts(event: &KeyEvent) -> PhysicalKeyParts {
    match event.physical_key {
        winit::keyboard::PhysicalKey::Code(code) => PhysicalKeyParts {
            code: dom_code(code),
            key_code: legacy_key_code(code),
        },
        winit::keyboard::PhysicalKey::Unidentified(_) => PhysicalKeyParts {
            code: String::new(),
            key_code: 0,
        },
    }
}

impl Lumen {
    /// Offer a key press to the page as a trusted `keydown` and report whether
    /// a handler cancelled it (`true` = the browser must skip its own action).
    ///
    /// Not offered: browser-reserved chords ([`is_reserved`]), keys while a
    /// frame field has focus (the frame has its own JS context) and the keys a
    /// focused text field already dispatches itself in `inject_char` /
    /// `inject_backspace` / `inject_delete_forward`. The press is remembered
    /// so that its release can send the `keyup`.
    pub(crate) fn page_keydown_cancelled(&mut self, code: KeyCode, key_event: &KeyEvent) -> bool {
        if !self.js_present
            || self.focused_frame.is_some()
            || is_reserved(code, self.modifiers)
            || self.field_dispatches_key_itself(code, key_event)
        {
            return false;
        }
        if !self.page_keys_down.contains(&code) {
            self.page_keys_down.push(code);
        }
        let nid = self.focused_node.map_or(0, |n| n.index());
        let script = dispatch_script(nid, "keydown", key_event, self.modifiers);
        !self.eval_key_script(script)
    }

    /// The release of a key whose `keydown` reached the page: send `keyup`.
    pub(crate) fn page_keyup(&mut self, code: KeyCode, key_event: &KeyEvent) {
        let Some(at) = self.page_keys_down.iter().position(|k| *k == code) else {
            return;
        };
        self.page_keys_down.swap_remove(at);
        if !self.js_present || self.focused_frame.is_some() {
            return;
        }
        let nid = self.focused_node.map_or(0, |n| n.index());
        let script = dispatch_script(nid, "keyup", key_event, self.modifiers);
        self.eval_key_script(script);
    }

    /// `true` when the focused page text field dispatches this key's own
    /// `keydown` (typed character, Backspace, forward Delete): the generic
    /// dispatch would send it twice.
    fn field_dispatches_key_itself(&self, code: KeyCode, key_event: &KeyEvent) -> bool {
        let plain = self.modifiers.is_empty();
        if !(plain || self.modifiers == ModifiersState::SHIFT) {
            return false;
        }
        if !self.focused_node.is_some_and(|nid| self.typeable_field(nid).is_some()) {
            return false;
        }
        match code {
            KeyCode::Backspace => true,
            KeyCode::Delete => plain,
            _ => key_event
                .logical_key
                .to_text()
                .is_some_and(|t| !t.is_empty() && t.chars().all(|c| !c.is_control())),
        }
    }

    /// Run a dispatch script synchronously; `false` = a handler cancelled the
    /// event. No script context, or a JS thread stuck in a dialog, never blocks
    /// the key.
    pub(super) fn eval_key_script(&self, script: String) -> bool {
        match route_query_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |js| {
            js.eval_js_value(&script)
        }) {
            Some(Ok(json)) => json.trim() != "false",
            Some(Err(_)) | None => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_follow_the_dom_names() {
        assert_eq!(dom_code(KeyCode::KeyJ), "KeyJ");
        assert_eq!(dom_code(KeyCode::ArrowLeft), "ArrowLeft");
        assert_eq!(dom_code(KeyCode::Escape), "Escape");
        assert_eq!(dom_code(KeyCode::SuperLeft), "MetaLeft");
        assert_eq!(dom_code(KeyCode::Numpad7), "Numpad7");
    }

    #[test]
    fn keys_follow_the_dom_values() {
        assert_eq!(dom_key(&Key::Character("j".into())), "j");
        assert_eq!(dom_key(&Key::Named(NamedKey::Space)), " ");
        assert_eq!(dom_key(&Key::Named(NamedKey::ArrowLeft)), "ArrowLeft");
        assert_eq!(dom_key(&Key::Named(NamedKey::Escape)), "Escape");
        assert_eq!(dom_key(&Key::Named(NamedKey::Super)), "Meta");
        assert_eq!(dom_key(&Key::Named(NamedKey::F5)), "F5");
    }

    #[test]
    fn legacy_key_codes() {
        assert_eq!(legacy_key_code(KeyCode::KeyJ), 74);
        assert_eq!(legacy_key_code(KeyCode::Digit5), 53);
        assert_eq!(legacy_key_code(KeyCode::Numpad3), 99);
        assert_eq!(legacy_key_code(KeyCode::F5), 116);
        assert_eq!(legacy_key_code(KeyCode::Space), 32);
        assert_eq!(legacy_key_code(KeyCode::ArrowLeft), 37);
        assert_eq!(legacy_key_code(KeyCode::Escape), 27);
        assert_eq!(legacy_key_code(KeyCode::Enter), 13);
        assert_eq!(legacy_key_code(KeyCode::Quote), 222);
        assert_eq!(legacy_key_code(KeyCode::Fn), 0);
    }

    #[test]
    fn modifier_bits_match_the_shim() {
        assert_eq!(modifier_mask(ModifiersState::empty()), 0);
        assert_eq!(modifier_mask(ModifiersState::CONTROL), 1);
        assert_eq!(modifier_mask(ModifiersState::SHIFT), 2);
        assert_eq!(modifier_mask(ModifiersState::ALT), 4);
        assert_eq!(modifier_mask(ModifiersState::SUPER), 8);
        assert_eq!(modifier_mask(ModifiersState::CONTROL | ModifiersState::SHIFT), 3);
    }

    #[test]
    fn tab_and_window_chords_are_reserved() {
        let c = ModifiersState::CONTROL;
        let cs = ModifiersState::CONTROL | ModifiersState::SHIFT;
        assert!(is_reserved(KeyCode::KeyW, c));
        assert!(is_reserved(KeyCode::KeyT, c));
        assert!(is_reserved(KeyCode::KeyT, cs));
        assert!(is_reserved(KeyCode::KeyN, c));
        assert!(is_reserved(KeyCode::Tab, c));
        assert!(is_reserved(KeyCode::Tab, cs));
        assert!(is_reserved(KeyCode::Digit3, c));
        assert!(is_reserved(KeyCode::F4, ModifiersState::ALT));
    }

    #[test]
    fn plain_keys_and_page_chords_are_not_reserved() {
        let e = ModifiersState::empty();
        assert!(!is_reserved(KeyCode::KeyW, e));
        assert!(!is_reserved(KeyCode::Tab, e));
        assert!(!is_reserved(KeyCode::Tab, ModifiersState::SHIFT));
        assert!(!is_reserved(KeyCode::Escape, e));
        assert!(!is_reserved(KeyCode::ArrowLeft, e));
        assert!(!is_reserved(KeyCode::KeyS, ModifiersState::CONTROL));
        assert!(!is_reserved(KeyCode::KeyL, ModifiersState::CONTROL));
        assert!(!is_reserved(KeyCode::Digit3, e));
    }
}
