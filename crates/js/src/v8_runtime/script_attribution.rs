//! LONGTASK-1 срез 4: culprit source-location attribution for
//! `PerformanceScriptTiming` (Long Animation Frames API §4,
//! `long_animation_frames.rs`).
//!
//! `_lumen_capture_call_site(fn)` — a scoped native (raw `v8::Function`
//! access; `JsValue` cannot carry a function argument, see
//! `v8_compat.rs`'s "Scoped native functions" section) that reads the three
//! pieces of `rusty_v8`'s existing `v8::Function` introspection surface
//! (`get_script_origin`, `get_script_line_number`, `get_script_column_number`,
//! `get_name`) and returns `{sourceURL, sourceFunctionName, sourceLine,
//! sourceColumn}`, or `undefined` when the argument is not a function
//! (anonymous top-level code, a value coerced from a string handler, etc. —
//! callers treat `undefined` as "no attribution available", matching the
//! class defaults `PerformanceScriptTiming` already falls back to).
//!
//! `event_target_shim.js`'s `_lumen_record_script_timing` calls this once per
//! recorded callback invocation and merges the result into the buffered
//! entry, so `PerformanceScriptTiming.sourceURL`/`sourceFunctionName` are now
//! real per-callback values instead of the always-empty-string class default.
//!
//! Срез 4 exposed `sourceLine`/`sourceColumn` (1-based, from
//! `get_script_line_number`/`get_script_column_number`, both documented
//! 0-indexed by `rusty_v8`) but left `sourceCharPosition` at the class
//! default: V8's C++ `v8::Function::GetScriptStartPosition()` (character
//! offset) exists in the headers and is already linked into the prebuilt
//! `rusty_v8.lib` (confirmed by scanning the archive's symbol table for
//! `?GetScriptStartPosition@Function@v8@@QEBAHXZ` — present, same mangling
//! shape as the already-wrapped `GetScriptColumnNumber`), but, like
//! `ObjectTemplate::MarkAsUndetectable` before GAP-DOCALLDDA's local binding,
//! is not exposed by the `v8` crate's Rust surface.
//!
//! **Срез 5 (this slice): `sourceCharPosition` closed.** Rather than a fourth
//! `.cc` translation unit, the wrapper
//! `lumen_v8__Function__GetScriptStartPosition` was added to the existing
//! `cpp/undetectable.cc` (same stand-in-declaration technique GAP-DOCALLDDA
//! established: no V8 headers included, a minimal `class Function { int
//! GetScriptStartPosition() const; };` stand-in whose only job is to make the
//! C++ compiler mangle the name identically to V8's real declaration).
//! `-1` (V8's own "unavailable" convention, matching the sibling
//! `get_script_line_number`/`get_script_column_number` `rusty_v8` already
//! wraps) maps to the class default `0`, same as the other source fields.
//! Bound functions (`Function.prototype.bind`) are not
//! unwrapped to their target either — V8 does not expose the wrapped
//! function through the embedder API — so `sourceFunctionName` for
//! `my_bound_function.bind(obj)` follows whatever V8's own `GetName()`
//! reports for the bound wrapper (empirically the bind target's name is
//! preserved by V8 itself, but this slice does not special-case it further).

// `_lumen_capture_call_site(fn)` — see module docs.
unsafe extern "C" {
    /// Lumen's local C++ binding for `v8::Function::GetScriptStartPosition()`
    /// (`cpp/undetectable.cc`, compiled by this crate's `build.rs`). Same
    /// convention as `rusty_v8`'s own `get_script_line_number`/
    /// `get_script_column_number`: a negative return means "unavailable".
    fn lumen_v8__Function__GetScriptStartPosition(this: *const v8::Function) -> i32;
}

pub(crate) fn capture_call_site(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
    rv: &mut v8::ReturnValue,
) {
    let Ok(func) = v8::Local::<v8::Function>::try_from(args.get(0)) else {
        return;
    };

    let source_url = func
        .get_script_origin(scope)
        .resource_name()
        .map(|v| v.to_rust_string_lossy(scope))
        .unwrap_or_default();
    // `rusty_v8` documents both as zero-indexed; the spec-adjacent field this
    // slice exposes (and `loaf-source-location.html` asserts) is 1-based, the
    // same convention as thrown-error stack frames.
    let source_line = func.get_script_line_number().map(|n| n + 1).unwrap_or(0);
    let source_column = func.get_script_column_number().map(|n| n + 1).unwrap_or(0);
    let name = func.get_name(scope).to_rust_string_lossy(scope);
    // SAFETY: the callee only forwards the pointer to a non-virtual,
    // non-inline V8 member function that takes no arguments and touches no
    // Rust-owned memory. `&*func` is a live `v8::Function*` for as long as
    // the `Local` is in scope, which covers this call.
    let raw_char_position = unsafe { lumen_v8__Function__GetScriptStartPosition(&*func) };
    let source_char_position = raw_char_position.max(0);

    let obj: v8::Local<v8::Object> = v8::Object::new(scope);
    let set = |scope: &mut v8::PinScope,
               obj: v8::Local<v8::Object>,
               key: &str,
               val: v8::Local<v8::Value>| {
        if let Some(k) = v8::String::new(scope, key) {
            obj.set(scope, k.into(), val);
        }
    };
    if let Some(s) = v8::String::new(scope, &source_url) {
        set(scope, obj, "sourceURL", s.into());
    }
    if let Some(s) = v8::String::new(scope, &name) {
        set(scope, obj, "sourceFunctionName", s.into());
    }
    let line_val: v8::Local<v8::Value> = v8::Integer::new(scope, source_line as i32).into();
    set(scope, obj, "sourceLine", line_val);
    let col_val: v8::Local<v8::Value> = v8::Integer::new(scope, source_column as i32).into();
    set(scope, obj, "sourceColumn", col_val);
    let char_pos_val: v8::Local<v8::Value> = v8::Integer::new(scope, source_char_position).into();
    set(scope, obj, "sourceCharPosition", char_pos_val);

    rv.set(obj.into());
}
