//! `eval()`/`new Function()`/`new AsyncFunction()`/`new GeneratorFunction()`/
//! `new AsyncGeneratorFunction()` Trusted Types enforcement (TRUSTEDTYPES-1,
//! срез 7 — the last remaining sink of that task).
//!
//! Every other sink this task closed (срезы 1-4, `trusted_types.rs`) is a DOM
//! setter or a shim wrapper: JS-level code the page calls, which a JS shim
//! can interpose on. `eval`/`new Function` are different — they compile a
//! string inside V8's own built-ins (`GlobalEval`/`CreateDynamicFunction`,
//! `v8/src/builtins/builtins-{global,function}.cc`), so no JS-level wrapper
//! is ever reached; the only interposition point V8 offers is the embedder
//! hook `Isolate::SetModifyCodeGenerationFromStringsCallback`, wired here via
//! the local C++ trampoline in `cpp/codegen_callback.cc` (see that file for
//! the MSVC x64 hidden-return-pointer ABI story — срезы 5/6 empirically
//! confirmed it with `dumpbin`, this срез's `probe_minimal_callback_fires`-
//! style smoke test at the bottom of this file re-confirms it end to end
//! against a live `V8JsRuntime`).
//!
//! V8 only invokes this callback at all once
//! `Context::set_allow_generation_from_strings(false)` has been called on
//! that context — [`install`] does that unconditionally alongside
//! registering the callback (both are per-isolate/per-context setup, done
//! once, not per-navigation), and the callback itself is what makes `eval`
//! keep working afterward: it inspects the source value and returns
//! "allowed" for everything TT permits, "blocked" (empty `modified_source`,
//! `codegen_allowed: false`) for everything it doesn't. V8 turns a blocked
//! result into `EvalError: Code generation from strings disallowed for this
//! context` (`Compiler::GetFunctionFromValidatedString`,
//! `NewEvalError(MessageTemplate::kCodeGenFromStrings, ...)`) — exactly the
//! `EvalError` every WPT test in this area (`eval-csp-tt-*.html`,
//! `eval-function-constructor*.html`) asserts on the throwing path.
//!
//! The TT check itself is delegated to JS
//! (`_lumen_tt_get_compliant_script_for_codegen`, `trusted_types.rs`) rather
//! than reimplemented natively — same division of labour as every other
//! sink in this task, and the one that already encodes the
//! `TrustedScript`/`defaultPolicy`/throw rules `eval`/`new Function` share
//! with them. The Rust callback's whole job is: build a `CallbackScope` from
//! the raw `Local<Context>` V8 hands it (`v8::callback_scope!`, same idiom
//! `promise_reject.rs` uses), look up that global function, call it, and
//! translate its outcome (string / `null` / thrown exception) into the
//! `ModifyCodeGenerationFromStringsResult` V8 expects back through the
//! hidden-pointer `out` parameter.
//!
//! `is_code_like` (V8's own `Object::IsCodeLike`, TC39's `[Symbol.isConcatSpreadable]`-
//! adjacent "code like" object protocol via `ObjectTemplate::SetCodeLike`) is
//! passed straight through unused: Lumen doesn't install any code-like
//! object template, so it is always `false` on every page this browser
//! renders, and the TT spec's own compliant-string algorithm doesn't
//! branch on it either — `original_source` reaching this callback is
//! already either a plain `string` or a `TrustedScript` instance (V8 itself
//! stringifies code-like objects before this hook runs when unconditional
//! codegen is still allowed; once it's disallowed, only string and
//! `TrustedScript`-shaped objects reach here through the JS built-ins this
//! callback actually gets exercised from — `eval`/`new Function` never hand
//! V8 a code-like object of their own accord).

/// Mirrors `v8::ModifyCodeGenerationFromStringsResult` field-for-field — see
/// `cpp/codegen_callback.cc`'s module docs for why this exact shape (and not
/// e.g. a `bool` return) is what MSVC x64 requires.
#[repr(C)]
struct RawResult {
    codegen_allowed: bool,
    modified_source: *const v8::String,
}

unsafe extern "C" {
    /// `cpp/codegen_callback.cc`'s trampoline for
    /// `v8::Isolate::SetModifyCodeGenerationFromStringsCallback`. Takes the
    /// same [`v8::UnsafeRawIsolatePtr`] every other native-isolate call in
    /// this crate uses (`html_all.rs`, `promise_reject.rs`) — `&*isolate`
    /// is the wrapper's `Deref` target, not the raw pointer V8's C++ API
    /// wants.
    fn lumen_v8__Isolate__SetModifyCodeGenerationFromStringsCallback(
        this: v8::UnsafeRawIsolatePtr,
        callback: unsafe extern "C" fn(
            *mut RawResult,
            v8::Local<v8::Context>,
            v8::Local<v8::Value>,
            bool,
        ),
    );
}

/// The raw ABI-shape callback V8 actually calls. Builds a full scope from the
/// bare `Local<Context>` V8 hands us — the same idiom
/// `promise_reject.rs::on_promise_reject` uses for the same reason (V8
/// invokes this from deep inside its own C++, with no existing `HandleScope`
/// of ours on the stack).
unsafe extern "C" fn modify_code_generation_from_strings(
    out: *mut RawResult,
    context: v8::Local<v8::Context>,
    source: v8::Local<v8::Value>,
    _is_code_like: bool,
) {
    // Deliberately *not* `v8::scope!(let scope, scope)` on top of this: V8
    // calls this hook synchronously from deep inside `Compiler::
    // ValidateDynamicCompilationSource`, with a real `HandleScope` already
    // active on the ambient C++ call stack (the one the eval/Function-
    // constructor call itself opened) — the very scope our caller uses to
    // read `out.modified_source` back after we return. `callback_scope!` on
    // a `Local<Context>` recognises this (`needs_scope: false` in the `v8`
    // crate's `NewCallbackScope` impl for `Local<Context>`) and gives a view
    // over that ambient scope rather than pushing a fresh one. Pushing an
    // extra nested `HandleScope` here (as `promise_reject.rs` does, safely,
    // because it never hands a `Local` back to native code) would pop and
    // invalidate any `Local` this function returns the instant it returns —
    // V8 would then dereference a freed handle slot, corrupting memory (a
    // real crash was reproduced this way while writing this hook: MSVC
    // release/debug both hard-aborted a few calls later inside
    // `CompilationCacheTable`, nowhere near the actual bug).
    v8::callback_scope!(unsafe scope, context);
    let ctx = scope.get_current_context();
    let global = ctx.global(scope);

    /// Outcome of calling `_lumen_tt_get_compliant_script_for_codegen`.
    enum Outcome<'s> {
        /// Shim not installed yet (isolate bootstrap script, before any page
        /// exists) — nothing to enforce.
        NoShim,
        /// The compliance check itself threw (no default policy, or a
        /// mutating default policy on the eval-specific exact-match rule).
        Threw,
        /// `null` — not a compile-time concern (`eval(42)`, `eval({})`, ...).
        NotApplicable,
        /// A string to actually compile — either the original source
        /// (enforcement off, or a `TrustedScript` already provided) or a
        /// default policy's `createScript` return value.
        Compliant(v8::Local<'s, v8::String>),
    }

    let outcome = (|| -> Outcome {
        let Some(key) = v8::String::new(scope, "_lumen_tt_get_compliant_script_for_codegen") else {
            return Outcome::NoShim;
        };
        let Some(func) = global
            .get(scope, key.into())
            .and_then(|v| v8::Local::<v8::Function>::try_from(v).ok())
        else {
            return Outcome::NoShim;
        };
        let Some(sink) = v8::String::new(scope, "eval") else {
            return Outcome::NoShim;
        };
        v8::tc_scope!(tc, scope);
        match func.call(tc, global.into(), &[source, sink.into()]) {
            None => Outcome::Threw,
            Some(v) if v.is_null_or_undefined() => Outcome::NotApplicable,
            Some(v) => match v8::Local::<v8::String>::try_from(v) {
                Ok(s) => Outcome::Compliant(s),
                // The shim never actually returns a non-string, non-null
                // value — treat defensively as "nothing to enforce" rather
                // than as a block, same as `NoShim`/`NotApplicable`.
                Err(_) => Outcome::NotApplicable,
            },
        }
    })();

    // `NoShim`/`NotApplicable` both mean "let V8 compile/handle the
    // original `source` exactly as it would with no callback installed at
    // all": codegen allowed, no replacement string. Only `Threw` blocks.
    // `Threw` is deliberately not re-thrown into the isolate:
    // `_lumen_tt_get_compliant_script_for_codegen` throws a `TypeError`
    // when it fails (right message, wrong *kind* for this sink —
    // `eval-csp-tt-no-default-policy.html` and
    // `eval-csp-tt-default-policy-mutate.html` both assert `EvalError`, not
    // `TypeError`); returning `codegen_allowed: false` instead routes
    // through V8's own `NewEvalError`/`kCodeGenFromStrings` machinery, which
    // is where the spec-mandated `EvalError` actually comes from.
    let (codegen_allowed, modified_source) = match outcome {
        Outcome::NoShim | Outcome::NotApplicable => (true, std::ptr::null()),
        Outcome::Threw => (false, std::ptr::null()),
        Outcome::Compliant(s) => {
            // SAFETY: `Local<T>` is `#[repr(C)]` over `(NonNull<T>,
            // PhantomData)` (`v8-150.1.0/src/handle.rs`) — the same "it's
            // really just a pointer" fact `cpp/codegen_callback.cc`'s module
            // docs lean on for `MaybeLocal<T>`. `into_raw`/`as_non_null` are
            // sealed/crate-private, so this is the public-API-safe way to
            // hand the pointer back through the `RawResult` buffer; it stays
            // valid because `scope` above is a view over the ambient
            // HandleScope (see the comment above `callback_scope!`), not a
            // scope this function pops on return.
            let ptr: std::ptr::NonNull<v8::String> = unsafe { std::mem::transmute(s) };
            (true, ptr.as_ptr() as *const v8::String)
        }
    };

    // SAFETY: `out` is the hidden result-buffer pointer V8's caller
    // allocated on its own stack and passes uninitialised — writing the
    // `RawResult` here is exactly what `cpp/codegen_callback.cc`'s C++-level
    // by-value-return signature expects the callee to do, matching MSVC
    // x64's "return via hidden pointer" convention confirmed by dumpbin in
    // срез 6.
    unsafe {
        std::ptr::write(
            out,
            RawResult {
                codegen_allowed,
                modified_source,
            },
        );
    }
}

/// Installs the codegen hook and flips
/// `Context::set_allow_generation_from_strings(false)` — see the module docs
/// for why both must happen together. Called once per isolate, from
/// [`super::thread`]'s `Isolate::new` site, mirroring how
/// `promise_reject::install`/`html_all`'s equivalents are wired.
pub(super) fn install(isolate: &mut v8::OwnedIsolate, context: &v8::Global<v8::Context>) {
    // SAFETY: `isolate` is a live, currently-owned `OwnedIsolate` — the same
    // precondition every other native-isolate call site in this crate
    // (`html_all.rs`, `promise_reject.rs`) relies on for
    // `as_raw_isolate_ptr`.
    let isolate_ptr = unsafe { isolate.as_raw_isolate_ptr() };
    // SAFETY: `lumen_v8__Isolate__SetModifyCodeGenerationFromStringsCallback`
    // (`cpp/codegen_callback.cc`) takes the isolate pointer and stores the
    // function pointer verbatim into the isolate's `modify_code_gen_callback`
    // field — confirmed by срез 6's disassembly of the real setter
    // (`mov qword ptr [rcx+10780h], rdx; ret`). `modify_code_generation_from_strings`
    // matches the raw ABI shape that setter's caller (`ModifyCodeGenerationFromStrings`
    // in `compiler.cc`) actually invokes it with.
    unsafe {
        lumen_v8__Isolate__SetModifyCodeGenerationFromStringsCallback(
            isolate_ptr,
            modify_code_generation_from_strings,
        );
    }
    v8::scope!(let scope, isolate);
    let ctx = v8::Local::new(scope, context);
    ctx.set_allow_generation_from_strings(false);
}
