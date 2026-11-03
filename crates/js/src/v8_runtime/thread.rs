//! Точка входа выделенного потока V8.
//!
//! Выделено из `v8_runtime.rs` батчем SPLIT-JS7 без изменений поведения.

use super::codegen_hook;
use super::named_access::window_named_properties_template;
use super::promise_reject::{drain_promise_rejections, install_promise_reject_hook};
use super::*;
use std::sync::mpsc::RecvTimeoutError;
use std::time::Duration;

// ── Foreground platform queue (BUG-1752) ─────────────────────────────────────

/// Shortest wait between two dives into V8's platform queue: one millisecond
/// while something in the isolate is still making progress, so an `await` round
/// trip in a spec harness costs a millisecond, not the idle step.
const PLATFORM_POLL_MIN: Duration = Duration::from_millis(1);

/// Longest wait between two dives into V8's platform queue. Reached only after
/// several empty polls — an idle tab then wakes ~30 times a second instead of
/// spinning, which is the difference between a parked event loop and a busy
/// core.
const PLATFORM_POLL_MAX: Duration = Duration::from_millis(32);

/// Runs every foreground task V8 has queued for `inner`'s isolate and the
/// microtask checkpoint that follows each one (BUG-1752).
///
/// V8 delivers part of its work not as a microtask but as a task on the
/// isolate's foreground task runner: settling `Atomics.waitAsync` (notify and
/// timeout alike), `FinalizationRegistry` cleanup callbacks, completion of an
/// asynchronous `WebAssembly.compile`/`instantiate`. Only
/// `v8::Platform::pump_message_loop` drains that queue, and this loop used to
/// block in `recv()` instead — so the task sat there until the shell happened
/// to send a command or a GC pass ran.
///
/// Bracketed by [`lumen_core::script_stall::StallHandle`] and the stall sampler
/// exactly like a [`V8Command::Run`] job: a callback this drain runs is script
/// execution on this thread, and the watchdog, the «страница не отвечает» bar
/// and `any_hung()` must see it (`subsystems/js.md` §Invariants).
///
/// Returns whether any task ran — the caller uses that to decide between the
/// short and the long poll step.
fn run_platform_tasks(
    inner: &mut V8Inner,
    platform: &v8::SharedRef<v8::Platform>,
    stall: &lumen_core::script_stall::StallHandle,
    sampler: Option<&super::stall_sampler::StallSampler>,
) -> bool {
    if let Some(s) = sampler {
        s.job_started();
    }
    stall.job_started();
    let mut ran = false;
    while v8::Platform::pump_message_loop(platform, &inner.isolate, false) {
        ran = true;
        // End of a task = end of its microtask checkpoint (HTML LS §8.1.7.3);
        // a `.then` chained onto the settled promise runs here, not later.
        inner.isolate.perform_microtask_checkpoint();
    }
    stall.job_finished();
    if let Some(s) = sampler {
        s.job_finished();
    }
    if ran {
        // Rejected promises queued by those tasks get the same report a
        // `V8Command::Run` job's do (BUG-918).
        drain_promise_rejections(inner);
    }
    ran
}

// ── Thread entry point ────────────────────────────────────────────────────────

/// Entry point of the dedicated V8 thread.
///
/// Initialises the V8 platform (idempotent), creates the isolate and context,
/// signals the caller via `init_tx`, then services [`V8Command`]s until the
/// channel closes or [`V8Command::Shutdown`] arrives.
pub(super) fn v8_thread_main(
    cmd_rx: std::sync::mpsc::Receiver<V8Command>,
    init_tx: Sender<Result<(), JsError>>,
    stall: lumen_core::script_stall::StallHandle,
) {
    ensure_v8_platform();

    let mut isolate = v8::Isolate::new(Default::default());
    // S12b-23: dynamic `import()` is resolved by an isolate-wide host hook
    // (static imports go through the callback passed to `instantiate_module`).
    crate::v8_esm::install_dynamic_import_hook(&mut isolate);
    // BUG-753 срез 3: `import.meta` (url/resolve/env) is filled by a host hook.
    crate::v8_esm::install_import_meta_hook(&mut isolate);
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
            // BUG-753 срез 2: every context owns an internal container from birth.
            let _ = crate::internal_globals::install_container(ctx_scope, ctx);
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
    // TRUSTEDTYPES-1 срез 7: `eval`/`new Function`-family codegen-from-
    // strings interception, isolate/context-wide like the hooks above —
    // gating on `require-trusted-types-for 'script'` happens inside the
    // callback itself (`_lumen_tt_get_compliant_script_for_codegen`), same
    // division as every other sink this task closed.
    codegen_hook::install(&mut inner.isolate, &inner.context);
    // BUG-935 срез 53: `LUMEN_JS_STALL_SAMPLE_MS` — стек долгой JS-задачи в stderr.
    let stall_sampler = super::stall_sampler::StallSampler::start(&mut inner.isolate, &inner.context);
    // COMPAT-HANG: `TerminateExecution` зовёт оболочка с UI-потока (кнопка
    // «Остановить скрипт», закрытие вкладки), поэтому замыкание держит
    // потокобезопасную ручку изолята, а не сам изолят.
    let terminate_handle = inner.isolate.thread_safe_handle();
    stall.set_terminator({
        let h = terminate_handle.clone();
        move || {
            h.terminate_execution();
        }
    });
    let _ = init_tx.send(Ok(()));

    // BUG-1752: wait with a step instead of `recv()`, and drain V8's foreground
    // task queue on every wake-up — including the ones nobody asked for. The
    // step stays at `PLATFORM_POLL_MIN` while work keeps showing up and backs
    // off to `PLATFORM_POLL_MAX` while the isolate is idle; a command resets it.
    let platform = v8::V8::get_current_platform();
    let mut poll_step = PLATFORM_POLL_MIN;
    loop {
        let cmd = match cmd_rx.recv_timeout(poll_step) {
            Ok(cmd) => cmd,
            Err(RecvTimeoutError::Timeout) => {
                let pumped =
                    run_platform_tasks(&mut inner, &platform, &stall, stall_sampler.as_ref());
                poll_step = if pumped {
                    PLATFORM_POLL_MIN
                } else {
                    poll_step.saturating_mul(2).min(PLATFORM_POLL_MAX)
                };
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => break,
        };
        match cmd {
            V8Command::Run(job) => {
                if let Some(s) = &stall_sampler {
                    s.job_started();
                }
                // Просьба об остановке, пришедшая между задачами, не должна
                // убить следующую.
                terminate_handle.cancel_terminate_execution();
                stall.job_started();
                job(&mut inner);
                stall.job_finished();
                // После `TerminateExecution` изолят остаётся «прерываемым» до
                // возврата на верхний уровень — снимаем, чтобы следующая задача
                // (таймер, обработчик события) исполнялась.
                terminate_handle.cancel_terminate_execution();
                if let Some(s) = &stall_sampler {
                    s.job_finished();
                }
                // BUG-918: end of the job = end of the microtask checkpoint,
                // which is where HTML LS §8.1.7.3 step 4 notifies about
                // rejected promises. Every JS entry point on this runtime
                // funnels through `V8Command::Run`, including the ones with
                // no event loop behind them (`--dump-*`, SVG rasterization,
                // unit tests), so the report stays visible there too.
                drain_promise_rejections(&mut inner);
                // BUG-1752: the job may have *queued* a platform task — an
                // async wasm compilation started by the script finishes on a
                // V8 thread and lands here — so drain before going back to the
                // wait, not after the next poll step.
                run_platform_tasks(&mut inner, &platform, &stall, stall_sampler.as_ref());
                poll_step = PLATFORM_POLL_MIN;
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
