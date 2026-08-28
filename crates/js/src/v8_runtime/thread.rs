//! Точка входа выделенного потока V8.
//!
//! Выделено из `v8_runtime.rs` батчем SPLIT-JS7 без изменений поведения.

use super::*;
use super::named_access::window_named_properties_template;
use super::promise_reject::{drain_promise_rejections, install_promise_reject_hook};

// ── Thread entry point ────────────────────────────────────────────────────────

/// Entry point of the dedicated V8 thread.
///
/// Initialises the V8 platform (idempotent), creates the isolate and context,
/// signals the caller via `init_tx`, then services [`V8Command`]s until the
/// channel closes or [`V8Command::Shutdown`] arrives.
pub(super) fn v8_thread_main(
    cmd_rx: std::sync::mpsc::Receiver<V8Command>,
    init_tx: Sender<Result<(), JsError>>,
) {
    ensure_v8_platform();

    let mut isolate = v8::Isolate::new(Default::default());
    // S12b-23: dynamic `import()` is resolved by an isolate-wide host hook
    // (static imports go through the callback passed to `instantiate_module`).
    crate::v8_esm::install_dynamic_import_hook(&mut isolate);
    // BUG-716: unhandledrejection/rejectionhandled dispatch, also isolate-wide.
    install_promise_reject_hook(&mut isolate);
    // Create the context inside a short-lived HandleScope so the scope's borrow
    // of `isolate` ends before we move `isolate` into `V8Inner`.
    let (context, baseline_globals) = {
        // scope! pins the HandleScope and gives scope: &mut PinnedRef<HandleScope<'_, ()>>
        v8::scope!(let scope, &mut isolate);
        let ctx = v8::Context::new(
            scope,
            v8::ContextOptions {
                global_template: Some(window_named_properties_template(scope)),
                ..Default::default()
            },
        );
        // Snapshot the bare context's own global keys (S11) before entering it
        // for anything else — this is the baseline `suspend()` diffs against.
        let baseline = {
            let ctx_scope = &mut v8::ContextScope::new(scope, ctx);
            let global = ctx.global(ctx_scope);
            let mut names = std::collections::HashSet::new();
            if let Some(own_props) = global.get_own_property_names(ctx_scope, Default::default())
            {
                for i in 0..own_props.length() {
                    if let Some(key) = own_props.get_index(ctx_scope, i)
                        && let Some(s) = key.to_string(ctx_scope)
                    {
                        names.insert(s.to_rust_string_lossy(ctx_scope));
                    }
                }
            }
            names
        };
        // scope deref-coerces to &Isolate via PinnedRef<HandleScope<'_,()>> → Isolate
        (v8::Global::new(scope, ctx), baseline)
    };

    let mut inner = V8Inner {
        isolate,
        context,
        native_fn_store: Vec::new(),
        native_fn_store_scoped: Vec::new(),
        baseline_globals,
    };
    let _ = init_tx.send(Ok(()));

    while let Ok(cmd) = cmd_rx.recv() {
        match cmd {
            V8Command::Run(job) => {
                job(&mut inner);
                // BUG-918: end of the job = end of the microtask checkpoint,
                // which is where HTML LS §8.1.7.3 step 4 notifies about
                // rejected promises. Every JS entry point on this runtime
                // funnels through `V8Command::Run`, including the ones with
                // no event loop behind them (`--dump-*`, SVG rasterization,
                // unit tests), so the report stays visible there too.
                drain_promise_rejections(&mut inner);
            }
            V8Command::Shutdown => break,
        }
    }
    // Free WASM import `v8::Global` GC roots on this thread while the isolate
    // is still alive (mirrors QuickJS's `wasm::clear_registry()` discipline at
    // `lib.rs:447` — see BUG-222). `Global::drop` no-ops safely on an already
    // disposed isolate, but releasing the persistent handle here is the
    // correct, leak-free order.
    crate::wasm::v8_bridge::clear_registry();
    // Same discipline for the ESM module map's `v8::Global<v8::Module>` roots
    // (S12b-23): release them here, while the isolate is still alive.
    crate::v8_esm::reset();
    // `inner` (OwnedIsolate + Global<Context>) drops here, on its owning thread.
}
