//! Keyboard state winit does not carry: the Caps Lock toggle and the layout in
//! use (UX-CAPSLOCK).
//!
//! `winit::keyboard::ModifiersState` has no Caps Lock, and nothing in the
//! shell reads the layout. Both are read from the OS on demand — a frame that
//! paints the password-field indicator, a key press that builds a
//! `KeyboardEvent`. Only the Windows reading exists today; elsewhere the
//! answers are "off" and "no label", so no indicator is drawn.
//!
//! Must be called on the thread that owns the window: the layout is a
//! per-thread property and the browser runs on that thread (ADR-032).

/// Bit of the `_lumen_dispatch_key_event` modifier mask that carries Caps Lock
/// (`ctrl` 1, `shift` 2, `alt` 4, `meta` 8 live in `page_keys::modifier_mask`).
pub const CAPS_LOCK_MASK_BIT: u32 = 16;

/// `true` while Caps Lock is toggled on.
#[must_use]
pub fn caps_lock_on() -> bool {
    imp::caps_lock_on()
}

/// [`CAPS_LOCK_MASK_BIT`] when Caps Lock is on, else 0.
#[must_use]
pub fn caps_lock_mask() -> u32 {
    if caps_lock_on() { CAPS_LOCK_MASK_BIT } else { 0 }
}

/// Short tag (`RU`) of the active keyboard layout when it is not Latin; `None`
/// for a Latin layout and where the layout cannot be read.
#[must_use]
pub fn non_latin_layout_tag() -> Option<&'static str> {
    imp::layout_langid().and_then(tag_for_langid)
}

/// Tag for a Windows `LANGID` (low word of the `HKL`): the primary language
/// (low 10 bits) of the scripts that cannot type a Latin password.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn tag_for_langid(langid: u16) -> Option<&'static str> {
    Some(match langid & 0x3ff {
        0x01 => "AR",
        0x02 => "BG",
        0x04 => "ZH",
        0x08 => "EL",
        0x0d => "HE",
        0x11 => "JA",
        0x12 => "KO",
        0x19 => "RU",
        0x1e => "TH",
        0x22 => "UK",
        0x23 => "BE",
        _ => return None,
    })
}

#[cfg(target_os = "windows")]
mod imp {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, GetKeyboardLayout, VK_CAPITAL};

    pub(super) fn caps_lock_on() -> bool {
        // SAFETY: no pointer arguments; the low bit of the result is the toggle.
        unsafe { GetKeyState(i32::from(VK_CAPITAL)) & 1 != 0 }
    }

    pub(super) fn layout_langid() -> Option<u16> {
        // SAFETY: thread id 0 = the calling thread; no pointer arguments.
        let hkl = unsafe { GetKeyboardLayout(0) };
        (hkl != 0).then_some((hkl as usize & 0xffff) as u16)
    }
}

#[cfg(not(target_os = "windows"))]
mod imp {
    pub(super) fn caps_lock_on() -> bool {
        false
    }

    pub(super) fn layout_langid() -> Option<u16> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cyrillic_layouts_are_tagged() {
        // 0x0419 Russian, 0x0422 Ukrainian, 0x0423 Belarusian, 0x0402 Bulgarian.
        assert_eq!(tag_for_langid(0x0419), Some("RU"));
        assert_eq!(tag_for_langid(0x0422), Some("UK"));
        assert_eq!(tag_for_langid(0x0423), Some("BE"));
        assert_eq!(tag_for_langid(0x0402), Some("BG"));
    }

    #[test]
    fn latin_layouts_are_not_tagged() {
        // 0x0409 en-US, 0x0407 de-DE, 0x040c fr-FR, 0x0809 en-GB.
        for id in [0x0409, 0x0407, 0x040c, 0x0809] {
            assert_eq!(tag_for_langid(id), None, "{id:#x}");
        }
    }

    #[test]
    fn mask_is_a_free_bit() {
        // ctrl 1, shift 2, alt 4, meta 8 are taken by `page_keys::modifier_mask`.
        assert_eq!(CAPS_LOCK_MASK_BIT & 0xf, 0);
    }
}
