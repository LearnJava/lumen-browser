//! User-activation gate for popups, the clipboard and permission prompts
//! (UX-USER-ACTIVATION).
//!
//! `navigator.userActivation` (GAP-USERACT) only *recorded* the user's last
//! gesture; the APIs that abuse a page's freedom — `window.open`,
//! `navigator.clipboard.writeText`, `Notification.requestPermission` and the
//! other permission prompts — never looked at it. With the gate on, each of
//! them needs a live activation (HTML LS §6.4), unless the user has allowed it
//! for the site in the permission popover.
//!
//! The gate is **off by default**: the engine is also embedded by the WPT
//! runner, the dump mode and the unit tests, which call these APIs from a
//! bare script. The interactive shell turns it on at startup
//! ([`set_gate_enabled`]), and `LUMEN_ALLOW_POPUPS=1` turns it off again for
//! automation that drives pages with `eval`.
//!
//! The flag and the site resolver are process-wide on purpose, like
//! [`crate::chrome_input`]: the shell owns one permission store, while the JS
//! contexts of tabs, frames and parked tabs come and go on the engine thread.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

/// `(origin, permission kind) → allowed for the site`; the kind is the
/// `PermissionKind` name of the store (`popups`, `clipboard`).
type SiteResolver = Arc<dyn Fn(&str, &str) -> bool + Send + Sync>;

static GATE_ENABLED: AtomicBool = AtomicBool::new(false);
static SITE_RESOLVER: RwLock<Option<SiteResolver>> = RwLock::new(None);

/// Turns the activation gate on or off for the whole process.
pub fn set_gate_enabled(on: bool) {
    GATE_ENABLED.store(on, Ordering::Relaxed);
}

/// `true` while popups, clipboard writes and permission prompts need a gesture.
pub fn gate_enabled() -> bool {
    GATE_ENABLED.load(Ordering::Relaxed)
}

/// The shell installs the lookup into its permission store; `None` removes it.
pub fn set_site_resolver(resolver: Option<SiteResolver>) {
    if let Ok(mut slot) = SITE_RESOLVER.write() {
        *slot = resolver;
    }
}

/// `true` when the user allowed `kind` (`popups`, `clipboard`) for `origin`
/// regardless of gestures. Without a resolver nothing is allowed.
pub fn site_allows(origin: &str, kind: &str) -> bool {
    SITE_RESOLVER
        .read()
        .ok()
        .and_then(|slot| slot.as_ref().map(|f| f(origin, kind)))
        .unwrap_or(false)
}

/// Does `kind` need a gesture for a document at `origin` right now? `false`
/// while the gate is off or the user allowed the site.
pub fn gesture_required(origin: &str, kind: &str) -> bool {
    gate_enabled() && !site_allows(origin, kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    // The gate is process-global, so the whole contract lives in ONE test:
    // parallel tests would race on the flag.
    #[test]
    fn gate_follows_flag_and_site_resolver() {
        set_gate_enabled(false);
        set_site_resolver(None);
        assert!(!gesture_required("https://a.test", "popups"), "gate off: nothing is required");

        set_gate_enabled(true);
        assert!(gesture_required("https://a.test", "popups"));

        set_site_resolver(Some(Arc::new(|origin, kind| origin == "https://a.test" && kind == "popups")));
        assert!(!gesture_required("https://a.test", "popups"), "allowed site");
        assert!(gesture_required("https://a.test", "clipboard"), "other kind stays gated");
        assert!(gesture_required("https://b.test", "popups"), "other site stays gated");

        set_gate_enabled(false);
        set_site_resolver(None);
    }
}
