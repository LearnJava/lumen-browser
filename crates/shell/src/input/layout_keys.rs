//! Keyboard-layout independence of shortcuts and `accesskey` (UX-HOTKEYS-LAYOUT).
//!
//! winit hands the shell two views of a key press: the *physical* key
//! (`KeyCode`, a position on a US keyboard) and the character the active layout
//! prints for it with no modifiers (`key_without_modifiers`). Shortcut tables
//! are written against the Latin letter ("Ctrl+T"), so the rule here is the one
//! browsers use: **the Latin character of the layout first, the physical
//! position as the fallback**.
//!
//! - AZERTY: the key printing `z` sits on the physical `W`; Ctrl+Z must be undo,
//!   not "close tab".
//! - Dvorak: the key printing `t` is the physical `F`; Ctrl+T must open a tab.
//! - ЙЦУКЕН / Ukrainian / Belarusian / Kazakh / phonetic Cyrillic: the key
//!   prints no Latin letter, so the physical position stands and Ctrl+Е is
//!   Ctrl+T as it always was.
//!
//! Only the typewriter block takes part: numpad, arrows, F-keys, Space and
//! friends keep their physical identity.

use winit::event::KeyEvent;
use winit::keyboard::{Key, KeyCode};
use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;

/// The key a Latin character sits on in the US layout, `None` for anything
/// that is not a single typewriter-block character.
fn code_for_latin(c: char) -> Option<KeyCode> {
    Some(match c.to_ascii_lowercase() {
        'a' => KeyCode::KeyA,
        'b' => KeyCode::KeyB,
        'c' => KeyCode::KeyC,
        'd' => KeyCode::KeyD,
        'e' => KeyCode::KeyE,
        'f' => KeyCode::KeyF,
        'g' => KeyCode::KeyG,
        'h' => KeyCode::KeyH,
        'i' => KeyCode::KeyI,
        'j' => KeyCode::KeyJ,
        'k' => KeyCode::KeyK,
        'l' => KeyCode::KeyL,
        'm' => KeyCode::KeyM,
        'n' => KeyCode::KeyN,
        'o' => KeyCode::KeyO,
        'p' => KeyCode::KeyP,
        'q' => KeyCode::KeyQ,
        'r' => KeyCode::KeyR,
        's' => KeyCode::KeyS,
        't' => KeyCode::KeyT,
        'u' => KeyCode::KeyU,
        'v' => KeyCode::KeyV,
        'w' => KeyCode::KeyW,
        'x' => KeyCode::KeyX,
        'y' => KeyCode::KeyY,
        'z' => KeyCode::KeyZ,
        '0' => KeyCode::Digit0,
        '1' => KeyCode::Digit1,
        '2' => KeyCode::Digit2,
        '3' => KeyCode::Digit3,
        '4' => KeyCode::Digit4,
        '5' => KeyCode::Digit5,
        '6' => KeyCode::Digit6,
        '7' => KeyCode::Digit7,
        '8' => KeyCode::Digit8,
        '9' => KeyCode::Digit9,
        '-' => KeyCode::Minus,
        // A layout that prints `+` unshifted (German) still means "zoom in".
        '=' | '+' => KeyCode::Equal,
        '[' => KeyCode::BracketLeft,
        ']' => KeyCode::BracketRight,
        '\\' => KeyCode::Backslash,
        ';' => KeyCode::Semicolon,
        '\'' => KeyCode::Quote,
        ',' => KeyCode::Comma,
        '.' => KeyCode::Period,
        '/' => KeyCode::Slash,
        '`' => KeyCode::Backquote,
        _ => return None,
    })
}

/// `true` for the keys that carry a printable character on any layout.
fn is_character_key(code: KeyCode) -> bool {
    matches!(
        code,
        KeyCode::KeyA | KeyCode::KeyB | KeyCode::KeyC | KeyCode::KeyD | KeyCode::KeyE
            | KeyCode::KeyF | KeyCode::KeyG | KeyCode::KeyH | KeyCode::KeyI | KeyCode::KeyJ
            | KeyCode::KeyK | KeyCode::KeyL | KeyCode::KeyM | KeyCode::KeyN | KeyCode::KeyO
            | KeyCode::KeyP | KeyCode::KeyQ | KeyCode::KeyR | KeyCode::KeyS | KeyCode::KeyT
            | KeyCode::KeyU | KeyCode::KeyV | KeyCode::KeyW | KeyCode::KeyX | KeyCode::KeyY
            | KeyCode::KeyZ
            | KeyCode::Digit0 | KeyCode::Digit1 | KeyCode::Digit2 | KeyCode::Digit3
            | KeyCode::Digit4 | KeyCode::Digit5 | KeyCode::Digit6 | KeyCode::Digit7
            | KeyCode::Digit8 | KeyCode::Digit9
            | KeyCode::Minus | KeyCode::Equal | KeyCode::BracketLeft | KeyCode::BracketRight
            | KeyCode::Backslash | KeyCode::Semicolon | KeyCode::Quote | KeyCode::Comma
            | KeyCode::Period | KeyCode::Slash | KeyCode::Backquote
            | KeyCode::IntlBackslash
    )
}

/// The key a shortcut table should see for a press of `physical` that the
/// active layout prints as `unmodified` (the character with no modifiers).
pub(crate) fn layout_code(physical: KeyCode, unmodified: Option<char>) -> KeyCode {
    if !is_character_key(physical) {
        return physical;
    }
    unmodified.and_then(code_for_latin).unwrap_or(physical)
}

/// The single character a key press prints with no modifiers, if there is one.
fn unmodified_char(event: &KeyEvent) -> Option<char> {
    match event.key_without_modifiers() {
        Key::Character(s) => {
            let mut chars = s.chars();
            let c = chars.next()?;
            chars.next().is_none().then_some(c)
        }
        _ => None,
    }
}

/// [`layout_code`] for a real winit event: what the shell's shortcut tables,
/// panels and vim mode should treat the press as. The page-facing paths
/// (`KeyboardEvent.code`) keep the physical key.
pub(crate) fn shortcut_code(physical: KeyCode, event: &KeyEvent) -> KeyCode {
    layout_code(physical, unmodified_char(event))
}

/// Characters an `accesskey` token may match for this press, best first: what
/// the layout prints (so `accesskey="ф"` works on a Russian layout), then the
/// Latin letter of the physical position (so Alt+Ф activates `accesskey="a"`).
/// Lower-cased; `accesskey` matching is case-insensitive.
pub(crate) fn accesskey_chars(physical: KeyCode, event: &KeyEvent) -> Vec<char> {
    accesskey_chars_for(physical, unmodified_char(event))
}

fn accesskey_chars_for(physical: KeyCode, typed: Option<char>) -> Vec<char> {
    let mut out = Vec::with_capacity(2);
    if let Some(c) = typed.filter(|c| c.is_alphanumeric()) {
        out.extend(c.to_lowercase());
    }
    if let Some(c) = latin_char(layout_code(physical, typed))
        && !out.contains(&c)
    {
        out.push(c);
    }
    out
}

/// Inverse of [`code_for_latin`] for letters and digits.
fn latin_char(code: KeyCode) -> Option<char> {
    ('a'..='z').chain('0'..='9').find(|c| code_for_latin(*c) == Some(code))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `physical` is the key position, `typed` what the layout prints on it.
    fn press(physical: KeyCode, typed: char) -> KeyCode {
        layout_code(physical, Some(typed))
    }

    #[test]
    fn qwerty_is_identity() {
        assert_eq!(press(KeyCode::KeyT, 't'), KeyCode::KeyT);
        assert_eq!(press(KeyCode::Digit0, '0'), KeyCode::Digit0);
        assert_eq!(press(KeyCode::Equal, '='), KeyCode::Equal);
    }

    #[test]
    fn cyrillic_layouts_fall_back_to_the_physical_key() {
        // ЙЦУКЕН, Ukrainian, Belarusian, Kazakh, phonetic: no Latin letter.
        for (physical, typed) in [
            (KeyCode::KeyT, 'е'),
            (KeyCode::KeyW, 'ц'),
            (KeyCode::KeyK, 'л'),
            (KeyCode::KeyS, 'і'),
            (KeyCode::KeyF, 'ә'),
            (KeyCode::KeyJ, 'ј'),
            (KeyCode::Comma, 'б'),
        ] {
            assert_eq!(press(physical, typed), physical);
        }
    }

    #[test]
    fn azerty_puts_z_and_w_where_the_letter_is() {
        // AZERTY prints `z` on the physical W and `w` on the physical Z.
        assert_eq!(press(KeyCode::KeyW, 'z'), KeyCode::KeyZ);
        assert_eq!(press(KeyCode::KeyZ, 'w'), KeyCode::KeyW);
        assert_eq!(press(KeyCode::KeyQ, 'a'), KeyCode::KeyA);
        assert_eq!(press(KeyCode::KeyA, 'q'), KeyCode::KeyQ);
        assert_eq!(press(KeyCode::Semicolon, 'm'), KeyCode::KeyM);
    }

    #[test]
    fn azerty_digit_row_keeps_the_physical_digit() {
        // Unshifted AZERTY digit row prints & é " ' ( - è _ ç à.
        assert_eq!(press(KeyCode::Digit1, '&'), KeyCode::Digit1);
        assert_eq!(press(KeyCode::Digit2, 'é'), KeyCode::Digit2);
        assert_eq!(press(KeyCode::Digit0, 'à'), KeyCode::Digit0);
    }

    #[test]
    fn dvorak_follows_the_printed_letter() {
        // Dvorak prints `t` on the physical K and `f` on the physical Y.
        assert_eq!(press(KeyCode::KeyK, 't'), KeyCode::KeyT);
        assert_eq!(press(KeyCode::KeyY, 'f'), KeyCode::KeyF);
        assert_eq!(press(KeyCode::KeyR, 'p'), KeyCode::KeyP);
        assert_eq!(press(KeyCode::KeyW, ','), KeyCode::Comma);
        assert_eq!(press(KeyCode::Minus, '['), KeyCode::BracketLeft);
    }

    #[test]
    fn qwertz_swaps_y_and_z() {
        assert_eq!(press(KeyCode::KeyY, 'z'), KeyCode::KeyZ);
        assert_eq!(press(KeyCode::KeyZ, 'y'), KeyCode::KeyY);
        // German `+` and `-` keys mean zoom in / zoom out.
        assert_eq!(press(KeyCode::BracketRight, '+'), KeyCode::Equal);
        assert_eq!(press(KeyCode::Slash, '-'), KeyCode::Minus);
    }

    #[test]
    fn keys_outside_the_typewriter_block_keep_their_identity() {
        assert_eq!(press(KeyCode::Numpad5, '5'), KeyCode::Numpad5);
        assert_eq!(press(KeyCode::Space, ' '), KeyCode::Space);
        assert_eq!(layout_code(KeyCode::ArrowLeft, None), KeyCode::ArrowLeft);
        assert_eq!(layout_code(KeyCode::KeyT, None), KeyCode::KeyT);
    }

    #[test]
    fn upper_case_and_unknown_characters_are_handled() {
        assert_eq!(press(KeyCode::KeyK, 'T'), KeyCode::KeyT);
        assert_eq!(press(KeyCode::KeyF, '€'), KeyCode::KeyF);
    }

    #[test]
    fn latin_char_round_trips_letters_and_digits() {
        assert_eq!(latin_char(KeyCode::KeyA), Some('a'));
        assert_eq!(latin_char(KeyCode::Digit7), Some('7'));
        assert_eq!(latin_char(KeyCode::Comma), None);
        assert_eq!(latin_char(KeyCode::Tab), None);
    }

    #[test]
    fn accesskey_candidates_are_typed_char_then_physical_latin() {
        // Russian layout: Alt+Ф is the physical A.
        assert_eq!(accesskey_chars_for(KeyCode::KeyA, Some('ф')), vec!['ф', 'a']);
        // Latin layout: one candidate, case-folded.
        assert_eq!(accesskey_chars_for(KeyCode::KeyA, Some('A')), vec!['a']);
        // Dvorak: the printed letter wins over the position.
        assert_eq!(accesskey_chars_for(KeyCode::KeyK, Some('t')), vec!['t']);
        // Punctuation is not an accesskey candidate.
        assert!(accesskey_chars_for(KeyCode::Comma, Some(',')).is_empty());
        assert!(accesskey_chars_for(KeyCode::Tab, None).is_empty());
    }

    use crate::input::keybindings::{KeyCommand, keybinding_for};
    use winit::keyboard::ModifiersState;

    /// Letter keys of a layout: (physical key, character printed on it).
    type Layout = &'static [(KeyCode, char)];

    const DVORAK: Layout = &[
        (KeyCode::KeyQ, '\''), (KeyCode::KeyW, ','), (KeyCode::KeyE, '.'), (KeyCode::KeyR, 'p'),
        (KeyCode::KeyT, 'y'), (KeyCode::KeyY, 'f'), (KeyCode::KeyU, 'g'), (KeyCode::KeyI, 'c'),
        (KeyCode::KeyO, 'r'), (KeyCode::KeyP, 'l'), (KeyCode::KeyA, 'a'), (KeyCode::KeyS, 'o'),
        (KeyCode::KeyD, 'e'), (KeyCode::KeyF, 'u'), (KeyCode::KeyG, 'i'), (KeyCode::KeyH, 'd'),
        (KeyCode::KeyJ, 'h'), (KeyCode::KeyK, 't'), (KeyCode::KeyL, 'n'), (KeyCode::Semicolon, 's'),
        (KeyCode::KeyZ, ';'), (KeyCode::KeyX, 'q'), (KeyCode::KeyC, 'j'), (KeyCode::KeyV, 'k'),
        (KeyCode::KeyB, 'x'), (KeyCode::KeyN, 'b'), (KeyCode::KeyM, 'm'), (KeyCode::Comma, 'w'),
        (KeyCode::Period, 'v'), (KeyCode::Slash, 'z'),
    ];

    const AZERTY: Layout = &[
        (KeyCode::KeyQ, 'a'), (KeyCode::KeyW, 'z'), (KeyCode::KeyE, 'e'), (KeyCode::KeyR, 'r'),
        (KeyCode::KeyT, 't'), (KeyCode::KeyY, 'y'), (KeyCode::KeyU, 'u'), (KeyCode::KeyI, 'i'),
        (KeyCode::KeyO, 'o'), (KeyCode::KeyP, 'p'), (KeyCode::KeyA, 'q'), (KeyCode::KeyS, 's'),
        (KeyCode::KeyD, 'd'), (KeyCode::KeyF, 'f'), (KeyCode::KeyG, 'g'), (KeyCode::KeyH, 'h'),
        (KeyCode::KeyJ, 'j'), (KeyCode::KeyK, 'k'), (KeyCode::KeyL, 'l'), (KeyCode::Semicolon, 'm'),
        (KeyCode::KeyZ, 'w'), (KeyCode::KeyX, 'x'), (KeyCode::KeyC, 'c'), (KeyCode::KeyV, 'v'),
        (KeyCode::KeyB, 'b'), (KeyCode::KeyN, 'n'),
    ];

    /// ЙЦУКЕН; Ukrainian, Belarusian and Kazakh differ from it only in keys
    /// that print further Cyrillic letters, so one table stands for the family.
    const CYRILLIC: Layout = &[
        (KeyCode::KeyQ, 'й'), (KeyCode::KeyW, 'ц'), (KeyCode::KeyE, 'у'), (KeyCode::KeyR, 'к'),
        (KeyCode::KeyT, 'е'), (KeyCode::KeyY, 'н'), (KeyCode::KeyU, 'г'), (KeyCode::KeyI, 'ш'),
        (KeyCode::KeyO, 'щ'), (KeyCode::KeyP, 'з'), (KeyCode::KeyA, 'ф'), (KeyCode::KeyS, 'ы'),
        (KeyCode::KeyD, 'в'), (KeyCode::KeyF, 'а'), (KeyCode::KeyG, 'п'), (KeyCode::KeyH, 'р'),
        (KeyCode::KeyJ, 'о'), (KeyCode::KeyK, 'л'), (KeyCode::KeyL, 'д'), (KeyCode::KeyZ, 'я'),
        (KeyCode::KeyX, 'ч'), (KeyCode::KeyC, 'с'), (KeyCode::KeyV, 'м'), (KeyCode::KeyB, 'и'),
        (KeyCode::KeyN, 'т'), (KeyCode::KeyM, 'ь'),
    ];

    /// Phonetic Cyrillic layouts print a Cyrillic letter on the key that
    /// carries the same Latin letter, so they are the identity on positions.
    const PHONETIC: Layout = &[
        (KeyCode::KeyA, 'а'), (KeyCode::KeyB, 'б'), (KeyCode::KeyC, 'ц'), (KeyCode::KeyD, 'д'),
        (KeyCode::KeyF, 'ф'), (KeyCode::KeyG, 'г'), (KeyCode::KeyK, 'к'), (KeyCode::KeyL, 'л'),
        (KeyCode::KeyM, 'м'), (KeyCode::KeyN, 'н'), (KeyCode::KeyP, 'п'), (KeyCode::KeyR, 'р'),
        (KeyCode::KeyS, 'с'), (KeyCode::KeyT, 'т'), (KeyCode::KeyW, 'в'), (KeyCode::KeyZ, 'з'),
    ];

    /// The key sequence that produces `Ctrl+<latin>` on `layout`: the physical
    /// key printing `latin`, or — on a layout without Latin letters — the key at
    /// `latin`'s own position.
    fn ctrl_chord(layout: Layout, latin: char) -> Option<(KeyCode, char)> {
        if let Some(&(p, c)) = layout.iter().find(|(_, c)| *c == latin) {
            return Some((p, c));
        }
        let position = code_for_latin(latin)?;
        layout.iter().copied().find(|(p, c)| *p == position && !c.is_ascii())
    }

    #[test]
    fn every_shortcut_letter_is_its_own_command_on_every_layout() {
        for (name, layout) in [("dvorak", DVORAK), ("azerty", AZERTY), ("ru/ua/by/kz", CYRILLIC), ("phonetic", PHONETIC)] {
            for latin in 'a'..='z' {
                let Some((physical, printed)) = ctrl_chord(layout, latin) else {
                    continue;
                };
                let expected = keybinding_for(code_for_latin(latin).unwrap(), ModifiersState::CONTROL);
                let got = keybinding_for(layout_code(physical, Some(printed)), ModifiersState::CONTROL);
                assert_eq!(got, expected, "{name}: Ctrl+{latin} (key {physical:?} prints {printed:?})");
            }
        }
    }

    #[test]
    fn named_cases_from_the_ux_report() {
        let ctrl = ModifiersState::CONTROL;
        // Dvorak Ctrl+T opens a tab (not the command palette of the physical T... K).
        assert_eq!(keybinding_for(layout_code(KeyCode::KeyK, Some('t')), ctrl), Some(KeyCommand::NewTab));
        // AZERTY Ctrl+Z (the physical W) is not "close tab".
        assert_ne!(keybinding_for(layout_code(KeyCode::KeyW, Some('z')), ctrl), Some(KeyCommand::CloseTab));
        assert_eq!(keybinding_for(layout_code(KeyCode::KeyZ, Some('w')), ctrl), Some(KeyCommand::CloseTab));
        // ЙЦУКЕН Ctrl+Е still opens a tab.
        assert_eq!(keybinding_for(layout_code(KeyCode::KeyT, Some('е')), ctrl), Some(KeyCommand::NewTab));
        // ЙЦУКЕН Alt+Ф reaches accesskey "a".
        assert!(accesskey_chars_for(KeyCode::KeyA, Some('ф')).contains(&'a'));
    }
}
