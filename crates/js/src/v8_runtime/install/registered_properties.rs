//! BUG-1505: `CSS.registerProperty()` / `CSS.unregisterProperty()` write to the same
//! registry the cascade reads for `@property` rules — the document's script
//! registrations ([`lumen_dom::Document::register_custom_property`]) — so a registered
//! property has its initial value, `inherits` and computed form in `getComputedStyle`.
//!
//! A registration changes the cascade of every element, not of one node: it is recorded
//! as an untracked mutation, which sends the next flush (and the shell's relayout) down
//! the full path instead of restyling only touched subtrees.

use super::reg;
#[allow(unused_imports)]
use super::super::*;

/// Natives backing the registration half of the CSS Properties and Values API shim.
#[allow(clippy::unwrap_used)] // унаследовано: тот же `d.lock().unwrap()`, что в соседних `install_*`
pub(crate) fn install_registered_properties(
    scope: &mut v8::PinScope<'_, '_>,
    ctx: v8::Local<'_, v8::Context>,
    store: &mut Vec<OwnedNativeFn>,
    doc: Arc<Mutex<lumen_dom::Document>>,
    dom_dirty: Arc<AtomicBool>,
    flush_stale: Arc<AtomicBool>,
    dom_touched: Arc<Mutex<DomTouched>>,
) -> JsResult<()> {
    // `(name, syntax, inherits, initial_value, has_initial_value) -> registered`: `false` when a
    // script already registered the name (the shim throws `InvalidModificationError`).
    {
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        let stale = Arc::clone(&flush_stale);
        let touched = Arc::clone(&dom_touched);
        reg!(
            scope, ctx, store, "_lumen_register_css_property",
            move |name: String, syntax: String, inherits: bool, initial: String, has_initial: bool| -> bool {
                let def = lumen_dom::RegisteredCustomProperty {
                    name,
                    syntax,
                    inherits,
                    initial_value: has_initial.then_some(initial),
                };
                let added = d.lock().unwrap().register_custom_property(def);
                if added {
                    record_dom_touch_unattributed(&touched);
                    dirty.store(true, Ordering::Relaxed);
                    stale.store(true, Ordering::Relaxed);
                }
                added
            }
        );
    }
    // `name -> removed`.
    {
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        let stale = Arc::clone(&flush_stale);
        let touched = Arc::clone(&dom_touched);
        reg!(scope, ctx, store, "_lumen_unregister_css_property", move |name: String| -> bool {
            let removed = d.lock().unwrap().unregister_custom_property(&name);
            if removed {
                record_dom_touch_unattributed(&touched);
                dirty.store(true, Ordering::Relaxed);
                stale.store(true, Ordering::Relaxed);
            }
            removed
        });
    }
    Ok(())
}
