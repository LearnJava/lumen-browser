//! `accesskey` activation (HTML Standard §6.6.2): Alt+key and Alt+Shift+key
//! focus the first element of the page that declares that key, and click it
//! when clicking is its natural activation (link, button, checkbox, ...).
//!
//! Which character a press stands for is decided by
//! [`crate::input::layout_keys`]: the layout's own character first, the Latin
//! letter of the physical position second, so Alt+Ф on a Russian layout
//! reaches `accesskey="a"`. The element search is the pure tree walk
//! [`crate::focus_nav::find_accesskey_target`].

use crate::*;

impl Lumen {
    /// Try the pressed Alt chord as an `accesskey`. `true` iff an element took
    /// it (the press is consumed).
    pub(crate) fn activate_accesskey(&mut self, physical: KeyCode, key_event: &KeyEvent) -> bool {
        if !(self.modifiers == ModifiersState::ALT
            || self.modifiers == (ModifiersState::ALT | ModifiersState::SHIFT))
        {
            return false;
        }
        let chars = accesskey_chars(physical, key_event);
        if chars.is_empty() {
            return false;
        }
        let Some(src) = self.layout_source.as_ref() else {
            return false;
        };
        let (target, focus_only) = {
            let doc = src.document.lock().unwrap_or_else(|e| e.into_inner());
            let flat_tree = lumen_dom::build_flat_tree(&doc);
            let Some(target) = focus_nav::find_accesskey_target(&doc, &flat_tree, doc.root(), &chars)
            else {
                return false;
            };
            let tag_is_field = doc.get(target).element_name().is_some_and(|n| {
                n.local.eq_ignore_ascii_case("textarea") || n.local.eq_ignore_ascii_case("select")
            });
            (target, tag_is_field || super::text_input::typeable_field_in(&doc, target).is_some())
        };
        self.focus_page_node(target, true);
        if !focus_only {
            self.activate_node(target);
        }
        self.request_redraw();
        true
    }
}
