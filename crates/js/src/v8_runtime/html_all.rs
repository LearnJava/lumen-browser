//! `document.all` and its `[[IsHTMLDDA]]` "unusual behaviors" (HTML LS
//! §obsolete, GAP-DOCALLDDA / BUG-1057).
//!
//! `document.all` is the one web-facing object whose shape no JavaScript can
//! reproduce: `typeof document.all` must report `"undefined"`, the object must
//! be falsy and loosely equal to both `null` and `undefined`, yet stay a real
//! object under `===` and answer property reads normally. A `Proxy` cannot do
//! it — there is no `typeof` trap — so the slot has to come from the engine.
//! V8 expresses it as `ObjectTemplate::MarkAsUndetectable()`, which the `v8`
//! crate compiles into its prebuilt library but exposes no Rust binding for;
//! Lumen's own wrapper for it lives in `cpp/undetectable.cc` (that file
//! explains why it is a local translation unit rather than a crate patch).
//!
//! The collection itself stays in JavaScript. The shim builds an ordinary live
//! `HTMLCollection`-shaped Proxy over "every element in tree order" and hands
//! it to [`make_html_all_collection`] as a *target*; what comes back is an
//! undetectable object whose property and index reads — and `in` checks — are
//! forwarded to that target by the interceptors below. Only the part JS
//! genuinely cannot express, the DDA slot, is native; liveness, named access
//! and `item`/`namedItem` all come from the shim's existing collection.
//!
//! The call handler below is not optional garnish: V8 CHECK-fails while
//! instantiating an undetectable template that has none (`Check failed:
//! !IsUndefined(obj->GetInstanceCallHandler())`), because `[[IsHTMLDDA]]` is
//! defined for callable objects. It also happens to be what HTML requires —
//! `document.all(name)` is a synonym for `namedItem`.
//!
//! Writes are deliberately not forwarded: `document.all` is a read-only
//! collection, so an unforwarded write lands on the wrapper as a plain own
//! property — what a non-interceptor object would do anyway.

/// Index of the internal field holding the JS object every intercepted access
/// is forwarded to.
const TARGET_FIELD: usize = 0;

/// How many internal fields the wrapper template reserves.
const INTERNAL_FIELD_COUNT: usize = 1;

unsafe extern "C" {
    /// Lumen's local C++ binding for `v8::ObjectTemplate::MarkAsUndetectable()`
    /// (`cpp/undetectable.cc`, compiled by this crate's `build.rs`). Takes the
    /// raw `ObjectTemplate` pointer, which is exactly what `rusty_v8`'s own
    /// `binding.cc` wrappers pass for the same class.
    fn lumen_v8__ObjectTemplate__MarkAsUndetectable(this: *const v8::ObjectTemplate);

    /// The same for `v8::ObjectTemplate::SetCallAsFunctionHandler()`, with the
    /// handler's optional `data` argument fixed to V8's own default (empty) on
    /// the C++ side — this call handler derives everything it needs from the
    /// receiver's internal field.
    fn lumen_v8__ObjectTemplate__SetCallAsFunctionHandler(
        this: *const v8::ObjectTemplate,
        callback: v8::FunctionCallback,
    );
}

/// Mark instances of `template` as "undetectable" — the `[[IsHTMLDDA]]` shape.
fn mark_as_undetectable(template: v8::Local<'_, v8::ObjectTemplate>) {
    // SAFETY: the callee only forwards the pointer to a non-virtual, non-inline
    // V8 member function that takes no arguments and touches no Rust-owned
    // memory. `&*template` is a live `v8::ObjectTemplate*` for as long as the
    // `Local` is in scope, which covers the whole call.
    unsafe { lumen_v8__ObjectTemplate__MarkAsUndetectable(&*template) };
}

/// Install the `[[Call]]` behaviour `document.all` is required to have — and
/// that V8 requires any undetectable object to have.
fn set_call_handler(template: v8::Local<'_, v8::ObjectTemplate>) {
    let callback: v8::FunctionCallback = v8::MapFnTo::map_fn_to(html_all_call);
    // SAFETY: as for `mark_as_undetectable` — a live `ObjectTemplate*` and a
    // plain function pointer of the exact shape V8 declares for a call
    // handler (`rusty_v8` builds every ordinary `v8::Function` from the same
    // `FunctionCallback` type).
    unsafe { lumen_v8__ObjectTemplate__SetCallAsFunctionHandler(&*template, callback) };
}

/// `document.all(...)` — HTML LS §obsolete gives the collection a `[[Call]]`:
/// with no argument it answers `undefined`, with one it behaves like
/// `namedItem`. Delegated to the target's own `namedItem` so the lookup rules
/// live in one place (the shim) rather than two.
fn html_all_call(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    mut rv: v8::ReturnValue<v8::Value>,
) {
    rv.set_undefined();
    if args.length() == 0 {
        return;
    }
    let holder = args.this();
    if holder.internal_field_count() != INTERNAL_FIELD_COUNT {
        return;
    }
    let Some(data) = holder.get_internal_field(scope, TARGET_FIELD) else {
        return;
    };
    let Ok(value) = TryInto::<v8::Local<v8::Value>>::try_into(data) else {
        return;
    };
    let Ok(target) = v8::Local::<v8::Object>::try_from(value) else {
        return;
    };
    let Some(key) = v8::String::new(scope, "namedItem") else {
        return;
    };
    let named_item = target
        .get(scope, key.into())
        .and_then(|v| v8::Local::<v8::Function>::try_from(v).ok());
    let Some(named_item) = named_item else {
        return;
    };
    let arg = args.get(0);
    v8::tc_scope!(tc, scope);
    if let Some(result) = named_item.call(tc, target.into(), &[arg])
        && !tc.has_caught()
    {
        rv.set(result);
    }
}

/// Read the forwarding target out of the wrapper an interceptor fired on.
///
/// `None` means "this is not one of our wrappers" — every interceptor then
/// declines, leaving V8's ordinary lookup in charge, rather than guessing.
/// `holder()` rather than `this()`: the wrapper carries the interceptor itself
/// and is never used as a prototype, so the two coincide, and `holder()` is the
/// one that stays correct if the object is ever put on a prototype chain.
fn target_of<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: &v8::PropertyCallbackArguments,
) -> Option<v8::Local<'s, v8::Object>> {
    let holder = args.holder();
    if holder.internal_field_count() != INTERNAL_FIELD_COUNT {
        return None;
    }
    let data = holder.get_internal_field(scope, TARGET_FIELD)?;
    let value: v8::Local<v8::Value> = data.try_into().ok()?;
    v8::Local::<v8::Object>::try_from(value).ok()
}

/// Build the template every `document.all` wrapper is instantiated from.
fn html_all_template<'s>(scope: &mut v8::PinScope<'s, '_>) -> v8::Local<'s, v8::ObjectTemplate> {
    // Named reads: `all.length`, `all.item`, `all.namedItem`, `all.someId`.
    // Symbols are forwarded too — the target's `Symbol.iterator` is what makes
    // `[...document.all]` work, and dropping symbols here would break it while
    // leaving `document.all.item(0)` fine.
    let getter = |scope: &mut v8::PinScope,
                  key: v8::Local<v8::Name>,
                  args: v8::PropertyCallbackArguments,
                  mut rv: v8::ReturnValue<v8::Value>| {
        let Some(target) = target_of(scope, &args) else {
            return v8::Intercepted::kNo;
        };
        match target.get(scope, key.into()) {
            Some(value) => {
                rv.set(value);
                v8::Intercepted::kYes
            }
            None => v8::Intercepted::kNo,
        }
    };
    // `'length' in document.all` / `hasOwnProperty`. Without it V8 would answer
    // existence checks from the (empty) wrapper instead of the target.
    let query = |scope: &mut v8::PinScope,
                 key: v8::Local<v8::Name>,
                 args: v8::PropertyCallbackArguments,
                 mut rv: v8::ReturnValue<v8::Integer>| {
        let Some(target) = target_of(scope, &args) else {
            return v8::Intercepted::kNo;
        };
        if !target.has(scope, key.into()).unwrap_or(false) {
            return v8::Intercepted::kNo;
        }
        // Matching what the target itself reports is not worth a second
        // round-trip: an `HTMLAllCollection`'s members are non-enumerable, and
        // `DONT_ENUM` keeps `for (k in document.all)` as quiet here as it is in
        // other engines.
        rv.set_int32(v8::PropertyAttribute::DONT_ENUM.as_u32() as i32);
        v8::Intercepted::kYes
    };
    // Indexed reads: `document.all[0]`.
    let indexed_getter = |scope: &mut v8::PinScope,
                          index: u32,
                          args: v8::PropertyCallbackArguments,
                          mut rv: v8::ReturnValue<v8::Value>| {
        let Some(target) = target_of(scope, &args) else {
            return v8::Intercepted::kNo;
        };
        let key = v8::Integer::new_from_unsigned(scope, index);
        match target.get(scope, key.into()) {
            Some(value) => {
                rv.set(value);
                v8::Intercepted::kYes
            }
            None => v8::Intercepted::kNo,
        }
    };

    let template = v8::ObjectTemplate::new(scope);
    template.set_internal_field_count(INTERNAL_FIELD_COUNT);
    mark_as_undetectable(template);
    // Order matters only in that both must be set before `new_instance`: V8
    // CHECKs an undetectable template for a call handler at instantiation.
    set_call_handler(template);
    template.set_named_property_handler(
        v8::NamedPropertyHandlerConfiguration::new()
            .getter(getter)
            .query(query),
    );
    template.set_indexed_property_handler(
        v8::IndexedPropertyHandlerConfiguration::new().getter(indexed_getter),
    );
    template
}

/// Wrap `target` — the shim's live all-elements collection — in a fresh
/// undetectable object that forwards reads to it.
///
/// Returns `None` only when V8 declines to instantiate the template (OOM); the
/// caller then leaves `document.all` as the plain collection rather than
/// installing a half-built object.
pub(crate) fn make_html_all_collection<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    target: v8::Local<'_, v8::Object>,
) -> Option<v8::Local<'s, v8::Object>> {
    let template = html_all_template(scope);
    let wrapper = template.new_instance(scope)?;
    let target_value: v8::Local<v8::Value> = target.into();
    wrapper.set_internal_field(TARGET_FIELD, target_value.into());
    Some(wrapper)
}
