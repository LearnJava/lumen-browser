//! S2: регистрация console-нативов (`_lumen_console_*`) для [`V8JsRuntime`].
//!
//! Выделено из `v8_runtime/runtime.rs` без изменений поведения (file-size gate,
//! docs/lint-policy.md §5.1).

use super::*;

// ── S2: console native registration ──────────────────────────────────────────

impl V8JsRuntime {
    /// Register the three console natives (`_lumen_console_log`,
    /// `_lumen_console_warn`, `_lumen_console_error`) as global JS functions.
    ///
    /// This is the S2 proof-of-concept that typed Rust closures can be
    /// registered via the compat layer and called from JS with auto-converted
    /// arguments.  S3 will extend this to all 184 `install_primitives` natives.
    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
    pub fn install_console_natives(
        &self,
        console_messages: Arc<std::sync::Mutex<Vec<(u8, String)>>>,
    ) -> JsResult<()> {
        self.run(move |inner| {
            // Disjoint field borrows: scope borrows isolate, native_fn_store is separate.
            let isolate = &mut inner.isolate;
            let context_global = &inner.context;
            let store = &mut inner.native_fn_store;

            v8::scope!(let scope, isolate);
            let ctx = v8::Local::new(scope, context_global);
            let scope = &mut v8::ContextScope::new(scope, ctx);

            // Local `reg!` macro that mirrors the rquickjs original in dom.rs.
            // Arity 0 and 1 shown as proof; higher arities use into_v8_fn2..7.
            macro_rules! reg {
                ($name:expr, move || $body:expr) => {{
                    let native = into_v8_fn0(move || $body);
                    register_v8_native(scope, ctx, store, $name, native)?;
                }};
                ($name:expr, move |$a:ident: $A:ty| $body:expr) => {{
                    let native = into_v8_fn1(move |$a: $A| $body);
                    register_v8_native(scope, ctx, store, $name, native)?;
                }};
            }

            // ── console ──────────────────────────────────────────────────────
            {
                let buf_log = Arc::clone(&console_messages);
                reg!("_lumen_console_log", move |msg: String| {
                    eprintln!("[JS] {msg}");
                    buf_log.lock().unwrap().push((0, msg));
                });
                let buf_warn = Arc::clone(&console_messages);
                reg!("_lumen_console_warn", move |msg: String| {
                    eprintln!("[JS warn] {msg}");
                    buf_warn.lock().unwrap().push((1, msg));
                });
                let buf_err = Arc::clone(&console_messages);
                reg!("_lumen_console_error", move |msg: String| {
                    eprintln!("[JS error] {msg}");
                    buf_err.lock().unwrap().push((2, msg));
                });
            }

            Ok(())
        })
    }
}
