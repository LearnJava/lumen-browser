//! Конвертеры значений V8 ↔ [`lumen_core::JsValue`] (`from_v8`/`to_v8`),
//! разбор исключения в [`lumen_core::JsError`] (`v8_err`) и пара
//! сериализаторов, на которых стоит `suspend()`/`resume()` кучи.
//!
//! Вынесено из `v8_runtime.rs` батчем SPLIT-JS5.

use super::*;

/// [`v8::ValueSerializerImpl`] with no custom host-object support: any
/// non-cloneable value (a `Function`, mainly — F1 in the migration brief)
/// throws `DataCloneError` via V8's default clone algorithm, which the
/// `suspend()` loop catches (`TryCatch::has_caught`) and skips per candidate
/// value.
pub(super) struct LumenValueSerializerImpl;

impl v8::ValueSerializerImpl for LumenValueSerializerImpl {
    fn throw_data_clone_error<'s>(
        &self,
        scope: &mut v8::PinScope<'s, '_>,
        message: v8::Local<'s, v8::String>,
    ) {
        let exc = v8::Exception::error(scope, message);
        scope.throw_exception(exc);
    }
}

/// [`v8::ValueDeserializerImpl`] with all defaults — `resume()` only ever
/// reads plain data (no host objects), so `read_host_object` is never
/// actually invoked.
pub(super) struct LumenValueDeserializerImpl;

impl v8::ValueDeserializerImpl for LumenValueDeserializerImpl {}

// ── Value converters ──────────────────────────────────────────────────────────

/// Max object/array nesting depth `from_v8` will walk before giving up on a
/// branch (BUG-633). JS-controlled values can be arbitrarily deep or cyclic —
/// e.g. WPT's `testharness.js` `Test` objects embed
/// `test.eventExpectations_.test_ === test` — and an unbounded recursive walk
/// can drive the Rust call stack (and V8 allocations happening on it) deep
/// enough that a GC triggered mid-walk fails V8's own
/// `isolate_->IsOnCentralStack()` invariant and takes down the whole process
/// via `V8_Fatal`, instead of surfacing as a catchable `JsError`.
pub(super) const FROM_V8_MAX_DEPTH: usize = 64;

/// Convert a V8 `Local<Value>` to a `JsValue`.
///
/// `scope` must be a `&PinScope<'s, '_>` (= `PinnedRef<HandleScope<'_, Context>>`).
/// Any scope that deref-coerces to one is accepted (e.g. `&mut PinnedRef<TryCatch<…>>`).
pub(super) fn from_v8<'s>(scope: &v8::PinScope<'s, '_>, val: v8::Local<'s, v8::Value>) -> JsResult<JsValue> {
    let mut ancestors = Vec::new();
    from_v8_bounded(scope, val, &mut ancestors)
}

/// Depth/cycle-guarded worker behind [`from_v8`]. `ancestors` holds the
/// identity hashes of every object/array currently being walked on the
/// current path (push on entry, pop on exit) so a self-reference anywhere in
/// the chain is caught instead of recursed into forever.
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub(super) fn from_v8_bounded<'s>(
    scope: &v8::PinScope<'s, '_>,
    val: v8::Local<'s, v8::Value>,
    ancestors: &mut Vec<std::num::NonZeroI32>,
) -> JsResult<JsValue> {
    if val.is_null() || val.is_undefined() {
        return Ok(JsValue::Null);
    }
    if val.is_boolean() {
        return Ok(JsValue::Bool(val.boolean_value(scope)));
    }
    if val.is_number() {
        return Ok(JsValue::Number(val.number_value(scope).unwrap_or(f64::NAN)));
    }
    if val.is_string() {
        let s = val
            .to_string(scope)
            .ok_or_else(|| JsError::Runtime("string conversion failed".into()))?;
        return Ok(JsValue::String(s.to_rust_string_lossy(scope)));
    }
    if val.is_array() {
        let arr: v8::Local<v8::Array> = val.try_into().unwrap();
        let hash = arr.get_identity_hash();
        if ancestors.len() >= FROM_V8_MAX_DEPTH {
            return Ok(JsValue::String("[Max Depth Exceeded]".into()));
        }
        if ancestors.contains(&hash) {
            return Ok(JsValue::String("[Circular]".into()));
        }
        ancestors.push(hash);
        let len = arr.length();
        let mut items = Vec::with_capacity(len as usize);
        for i in 0..len {
            let elem = arr
                .get_index(scope, i)
                .ok_or_else(|| JsError::Runtime(format!("array[{i}] is missing")))?;
            items.push(from_v8_bounded(scope, elem, ancestors)?);
        }
        ancestors.pop();
        return Ok(JsValue::Array(items));
    }
    if val.is_object() {
        let obj: v8::Local<v8::Object> = val.try_into().unwrap();
        let hash = obj.get_identity_hash();
        if ancestors.len() >= FROM_V8_MAX_DEPTH {
            return Ok(JsValue::String("[Max Depth Exceeded]".into()));
        }
        if ancestors.contains(&hash) {
            return Ok(JsValue::String("[Circular]".into()));
        }
        ancestors.push(hash);
        let own_props = obj
            .get_own_property_names(scope, Default::default())
            .ok_or_else(|| JsError::Runtime("get_own_property_names failed".into()))?;
        let mut entries: Vec<(String, JsValue)> = Vec::new();
        for i in 0..own_props.length() {
            let key = own_props.get_index(scope, i).unwrap();
            let key_str = key
                .to_string(scope)
                .ok_or_else(|| JsError::Runtime("property key to_string failed".into()))?
                .to_rust_string_lossy(scope);
            let prop_val = obj
                .get(scope, key)
                .ok_or_else(|| JsError::Runtime(format!("get '{key_str}' failed")))?;
            entries.push((key_str, from_v8_bounded(scope, prop_val, ancestors)?));
        }
        ancestors.pop();
        return Ok(JsValue::object(entries));
    }
    Ok(JsValue::Undefined)
}

/// Convert a `JsValue` to a V8 `Local<Value>`.
pub(super) fn to_v8<'s>(scope: &v8::PinScope<'s, '_>, val: JsValue) -> JsResult<v8::Local<'s, v8::Value>> {
    Ok(match val {
        // BUG-442: keep Null and Undefined distinct (see the sibling
        // `jsvalue_to_v8` in `v8_compat.rs` for the full rationale).
        JsValue::Null => v8::null(scope).into(),
        JsValue::Undefined => v8::undefined(scope).into(),
        JsValue::Bool(b) => v8::Boolean::new(scope, b).into(),
        JsValue::Number(n) => v8::Number::new(scope, n).into(),
        JsValue::String(s) => v8::String::new(scope, &s)
            .ok_or_else(|| JsError::Runtime("OOM: string allocation".into()))?
            .into(),
        JsValue::Array(items) => {
            let arr = v8::Array::new(scope, items.len() as i32);
            for (i, item) in items.into_iter().enumerate() {
                let v8_item = to_v8(scope, item)?;
                arr.set_index(scope, i as u32, v8_item);
            }
            arr.into()
        }
        JsValue::Object(entries) => {
            let obj = v8::Object::new(scope);
            for (k, v) in entries {
                let key = v8::String::new(scope, &k)
                    .ok_or_else(|| JsError::Runtime("OOM: key allocation".into()))?;
                let v8_val = to_v8(scope, v)?;
                obj.set(scope, key.into(), v8_val);
            }
            obj.into()
        }
    })
}

/// Extract an error message from a V8 exception value.
pub(super) fn v8_err<'s>(scope: &v8::PinScope<'s, '_>, exc: v8::Local<'s, v8::Value>) -> JsError {
    // Try obj.message first (covers Error instances), fall back to string coercion.
    if let Ok(obj) = v8::Local::<v8::Object>::try_from(exc)
        && let Some(msg_key) = v8::String::new(scope, "message")
        && let Some(msg_val) = obj.get(scope, msg_key.into())
        && msg_val.is_string()
        && let Some(s) = msg_val.to_string(scope)
    {
        return JsError::Runtime(s.to_rust_string_lossy(scope));
    }
    let msg = exc
        .to_string(scope)
        .map(|s| s.to_rust_string_lossy(scope))
        .unwrap_or_else(|| "JS exception".into());
    JsError::Runtime(msg)
}
