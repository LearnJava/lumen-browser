//! Who owns the keyboard: the browser chrome or the page (UX-FOCUS-ARBITER).
//!
//! While the user types in the address bar the chrome is the single owner of
//! keyboard focus. A page must not be able to take it back: `autofocus` at
//! DOMContentLoaded and a script's `element.focus()` (a `setTimeout(focus)`
//! loop on a login page, say) are ignored for as long as the flag is set.
//!
//! The flag is process-wide on purpose: the shell has one address bar, while
//! the JS contexts of the page, its frames and the parked tabs come and go on
//! the engine thread, and a context must see the state from its first line —
//! the `autofocus` flush runs during load, before the shell has handled the
//! new document.

use std::sync::atomic::{AtomicBool, Ordering};

static CHROME_OWNS_KEYBOARD: AtomicBool = AtomicBool::new(false);

/// The shell calls this whenever the address bar opens or closes.
pub fn set_chrome_owns_keyboard(on: bool) {
    CHROME_OWNS_KEYBOARD.store(on, Ordering::Relaxed);
}

/// `true` while the chrome (the address bar) holds keyboard focus.
pub fn chrome_owns_keyboard() -> bool {
    CHROME_OWNS_KEYBOARD.load(Ordering::Relaxed)
}
