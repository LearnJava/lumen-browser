//! Process-global clipboard provider bridging `navigator.clipboard` to the host
//! platform clipboard.
//!
//! The JS shim's `navigator.clipboard.readText()` / `writeText()` delegate to the
//! native bindings `_lumen_clipboard_read` / `_lumen_clipboard_write` (registered
//! in [`crate::dom::install_dom_api`]). Those bindings forward to the provider
//! installed here by the shell via [`set_clipboard_provider`]. When no provider
//! is installed (headless tests, dump modes), reads return `""` and writes are
//! discarded — matching the spec'd "permission denied" no-op behaviour.
//!
//! A process-global [`OnceLock`] (mirroring `broadcast_channel::HUB`) is used
//! instead of threading the provider through every `install_dom_api` call site:
//! the clipboard has no per-runtime state, so a single shared provider suffices.

use lumen_core::ext::ClipboardProvider;
use std::sync::{Arc, OnceLock, RwLock};

/// The process-global clipboard provider, installed once by the shell.
///
/// Wrapped in `RwLock` so the shell may replace it (e.g. after platform init)
/// while the script thread reads it; reads dominate, so `RwLock` over `Mutex`.
static PROVIDER: OnceLock<RwLock<Option<Arc<dyn ClipboardProvider>>>> = OnceLock::new();

/// Lazily-initialised handle to the global provider slot.
fn slot() -> &'static RwLock<Option<Arc<dyn ClipboardProvider>>> {
    PROVIDER.get_or_init(|| RwLock::new(None))
}

/// Install the host clipboard provider backing `navigator.clipboard`.
///
/// Called by the shell during startup. Replaces any previously installed
/// provider. Safe to call from any thread.
pub fn set_clipboard_provider(provider: Arc<dyn ClipboardProvider>) {
    if let Ok(mut guard) = slot().write() {
        *guard = Some(provider);
    }
}

/// Read plain text from the installed clipboard provider.
///
/// Returns `""` when no provider is installed or the lock is poisoned.
pub(crate) fn read_text() -> String {
    slot()
        .read()
        .ok()
        .and_then(|g| g.as_ref().map(|p| p.read_text()))
        .unwrap_or_default()
}

/// Write plain text to the installed clipboard provider.
///
/// No-op when no provider is installed or the lock is poisoned.
pub(crate) fn write_text(text: &str) {
    if let Ok(guard) = slot().read()
        && let Some(p) = guard.as_ref()
    {
        p.write_text(text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// In-memory clipboard double for testing the provider plumbing.
    struct MockClipboard(Mutex<String>);

    impl ClipboardProvider for MockClipboard {
        fn read_text(&self) -> String {
            self.0.lock().unwrap().clone()
        }
        fn write_text(&self, text: &str) {
            *self.0.lock().unwrap() = text.to_owned();
        }
    }

    #[test]
    fn roundtrip_through_installed_provider() {
        set_clipboard_provider(Arc::new(MockClipboard(Mutex::new(String::new()))));
        write_text("hello clipboard");
        assert_eq!(read_text(), "hello clipboard");
        write_text("overwrite ✓");
        assert_eq!(read_text(), "overwrite ✓");
    }
}
