// Lumen's local binding for `v8::ObjectTemplate::MarkAsUndetectable()`.
//
// GAP-DOCALLDDA / BUG-1057. `document.all` needs the `[[IsHTMLDDA]]` internal
// slot, whose only V8-level expression is `MarkAsUndetectable()`. That method
// is declared in V8's public headers and is **already compiled into the
// prebuilt `rusty_v8.lib` every build downloads** (verified: the archive
// exports `?MarkAsUndetectable@ObjectTemplate@v8@@QEAAXXZ`), but the `v8`
// crate's Rust surface exposes no wrapper for it, and the crate's own
// `binding.cc` is compiled only on the `V8_FROM_SOURCE` path — patching it is
// therefore useless to a consumer using the prebuilt library.
//
// So the wrapper lives here instead: one translation unit whose only job is to
// let the C++ compiler emit a call to an already-linked symbol. Upstream PR
// denoland/rusty_v8#2078 adds the same wrapper to the crate; once it lands in a
// published release this file, `build.rs` and the `cc` build-dependency all go
// away in favour of `ObjectTemplate::mark_as_undetectable()`.
//
// V8's headers are deliberately NOT included. The declaration below is a
// minimal stand-in whose sole purpose is to make the compiler mangle the name
// the same way V8 did — mangling depends only on the namespace, class name,
// function name and (empty) parameter list, none of which this file could get
// wrong without failing to link. Avoiding the include keeps the build free of
// any dependency on the `v8` crate's source layout (it declares no `links` key,
// so there is no supported way to ask cargo for its include path) and on V8's
// build-time defines (`V8_COMPRESS_POINTERS`, `V8_ENABLE_SANDBOX`, …), which
// would otherwise have to match the prebuilt library exactly.
//
// Only a pointer crosses this boundary and the callee is a non-virtual,
// non-inline member function, so no type layout and no vtable participates.

// Two methods are bound, not one: V8 refuses to instantiate an undetectable
// template that has no call handler — `Check failed:
// !IsUndefined(obj->GetInstanceCallHandler())` — because `[[IsHTMLDDA]]` is
// defined for callable objects (`document.all(name)` is part of the legacy
// interface). `SetCallAsFunctionHandler` has no `rusty_v8` binding either, so
// it comes from here as well.
//
// A third, unrelated method rides along in this file rather than a new
// translation unit: `v8::Function::GetScriptStartPosition()`
// (LONGTASK-1 срез 5, `script_attribution.rs`) — same situation (declared in
// V8's headers, already linked into the prebuilt `rusty_v8.lib`, no `v8`
// crate wrapper), same fix (a stand-in declaration that mangles identically),
// so it is grouped with the other local V8 bindings this crate maintains
// rather than duplicating the file-level rationale in a third `.cc`.
namespace v8 {
class Value;
template <typename T>
class FunctionCallbackInfo;

// Stand-in for `v8::Local<T>`: one pointer, trivially copyable, so it is
// passed in a register exactly like the real one (`rusty_v8`'s `support.h`
// static-asserts `sizeof(Local<T>) == sizeof(T*)`). Only ever constructed
// empty here — V8's own default for the handler's `data` argument.
template <typename T>
class Local {
 public:
  Local() : val_(nullptr) {}
  T* val_;
};

class ObjectTemplate {
 public:
  void MarkAsUndetectable();
  void SetCallAsFunctionHandler(
      void (*callback)(const FunctionCallbackInfo<Value>&), Local<Value> data);
};

// Only the one member this file needs — matches `rusty_v8`'s own
// `binding.cc` convention of declaring narrow stand-ins per translation unit,
// not the whole class.
class Function {
 public:
  int GetScriptStartPosition() const;
};
}  // namespace v8

extern "C" void lumen_v8__ObjectTemplate__MarkAsUndetectable(
    v8::ObjectTemplate* self) {
  // `rusty_v8` represents `Local<T>` as a plain `T*` (`support.h`'s
  // `ptr_to_local` static-asserts exactly that and that `*local == ptr`), so
  // the pointer Rust hands over is the same `this` the crate's own
  // `binding.cc` wrappers call through.
  self->MarkAsUndetectable();
}

extern "C" void lumen_v8__ObjectTemplate__SetCallAsFunctionHandler(
    v8::ObjectTemplate* self,
    void (*callback)(const v8::FunctionCallbackInfo<v8::Value>&)) {
  self->SetCallAsFunctionHandler(callback, v8::Local<v8::Value>());
}

// Character offset (0-indexed, per V8's own convention for the sibling
// `GetScriptColumnNumber`/`GetScriptLineNumber` this crate already wraps) of
// `self`'s definition within its source script. Negative return means
// "unavailable" (bound/native functions), same convention `rusty_v8` already
// applies to the two methods above.
extern "C" int lumen_v8__Function__GetScriptStartPosition(
    const v8::Function* self) {
  return self->GetScriptStartPosition();
}
