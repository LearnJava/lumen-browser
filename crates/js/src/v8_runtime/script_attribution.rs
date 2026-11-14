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

/// Reads a V8 string through `String::WriteV2` (`i::String::WriteToFlat` — a
/// cons-tree walk), not through `to_rust_string_lossy` (`v8::String::ValueView`).
///
/// Why: a cons string that V8 itself flags flat (`second == ""`) but whose
/// `first` is still a cons reaches `SlowGetFlatContent`'s unguarded
/// `TryGetFlatContentFromDirectString(...).value()` — the empty optional
/// throws `std::bad_optional_access`, and V8 is built with `-fno-exceptions`,
/// so its terminate handler aborts the whole process (BUG-1783: ea.com died
/// there reading a function name). `WriteToFlat` walks any cons shape
/// correctly and never touches the flat-content machinery; the price is one
/// UTF-16 → UTF-8 conversion per read, which for the handful of LoAF
/// attribution strings is noise.
fn string_lossy_via_write(scope: &mut v8::PinScope, s: v8::Local<v8::String>) -> String {
    let mut buf = vec![0u16; s.length()];
    s.write_v2(scope, 0, &mut buf, v8::WriteFlags::empty());
    String::from_utf16_lossy(&buf)
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
        .and_then(|v| v.try_cast::<v8::String>().ok())
        .map(|s| string_lossy_via_write(scope, s))
        .unwrap_or_default();
    // `rusty_v8` documents both as zero-indexed; the spec-adjacent field this
    // slice exposes (and `loaf-source-location.html` asserts) is 1-based, the
    // same convention as thrown-error stack frames.
    let source_line = func.get_script_line_number().map(|n| n + 1).unwrap_or(0);
    let source_column = func.get_script_column_number().map(|n| n + 1).unwrap_or(0);
    let name = string_lossy_via_write(scope, func.get_name(scope));
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

#[cfg(test)]
mod tests {
    use std::sync::RwLock;

    /// Guards concurrent test access — parallel reads are fine, init is
    /// exclusive (same contract as `tests/cases/v8_smoke.rs`).
    static PROCESS_LOCK: RwLock<()> = RwLock::new(());

    use super::string_lossy_via_write;

    /// BUG-1783: `to_rust_string_lossy` (`v8::String::ValueView`) aborts the
    /// process on a V8 string shape that reaches the unguarded
    /// `SlowGetFlatContent().value()`. The replacement reader must return the
    /// correct content for every string shape constructable from the API
    /// (sequential, cons, sliced, thin/internalized) without touching the
    /// flat-content machinery at all — the process surviving the test is part
    /// of the assertion.
    #[test]
    fn string_read_via_write_matches_content_for_every_shape() {
        crate::v8_runtime::ensure_v8_platform();
        let _guard = PROCESS_LOCK.read().unwrap();

        let isolate = &mut v8::Isolate::new(Default::default());
        v8::scope!(let scope, isolate);

        let context = v8::Context::new(scope, Default::default());
        let scope = &mut v8::ContextScope::new(scope, context);

        // Sequential ("abc…") and the mixed-script string with a two-byte char.
        let plain = v8::String::new(scope, "abcdefghij").unwrap();
        let wide = v8::String::new(scope, "abcΩdef").unwrap();
        assert_eq!(string_lossy_via_write(scope, plain), "abcdefghij");
        assert_eq!(string_lossy_via_write(scope, wide), "abcΩdef");

        // Cons, non-flat: two halves concatenated via the API (`String::Concat`
        // → `NewConsString` — V8 keeps a real ConsString here).
        let left = v8::String::new(scope, "abcdefghij").unwrap();
        let right = v8::String::new(scope, "0123456789Ω").unwrap();
        let cons = v8::String::concat(scope, left, right).unwrap();
        assert_eq!(
            string_lossy_via_write(scope, cons),
            "abcdefghij0123456789Ω"
        );

        // Sliced + thin, produced by JS: `substring` yields a SlicedString over
        // the flat parent; a property-key lookup internalizes (thin) a cons.
        let source = v8::String::new(
            scope,
            r#"
            (function () {
                var s = "abcdefghij0123456789";
                var sliced = s.substring(2, 18);
                var key = sliced + "«tail»";
                var obj = {};
                obj[key] = 1;              // internalizes `key` → thin/flat
                var probe = function inner() {};
                probe;                     // inferred function name via V8
                return [sliced, key, Object.keys(obj)[0]];
            })()
            "#,
        )
        .unwrap();
        let script = v8::Script::compile(scope, source, None).unwrap();
        let result = script.run(scope).expect("probe script must run");
        let arr: v8::Local<v8::Array> = result.try_into().unwrap();
        let mut read = |i: i32| {
            let v = arr.get_index(scope, i as u32).unwrap();
            string_lossy_via_write(scope, v.try_cast::<v8::String>().unwrap())
        };
        assert_eq!(read(0), "cdefghij01234567");
        assert_eq!(read(1), "cdefghij01234567«tail»");
        assert_eq!(read(2), "cdefghij01234567«tail»");
    }
}
