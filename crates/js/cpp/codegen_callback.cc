// Lumen's local binding for `v8::Isolate::SetModifyCodeGenerationFromStringsCallback()`.
//
// TRUSTEDTYPES-1 (срезы 5-7). `eval`/`new Function`/`new AsyncFunction`/
// `new GeneratorFunction`/`new AsyncGeneratorFunction` compile a JS string
// natively — no JS-shim wrapper can intercept them, because the page calls
// V8's own built-ins directly. The only embedder hook V8 offers for "should
// code generation from this string be allowed, and what string should
// actually be compiled" is this callback (V8 internal name:
// `ModifyCodeGenerationFromStrings`, invoked from
// `Compiler::ValidateDynamicCompilationSource`,
// `v8/src/codegen/compiler.cc`) — the TT L2 §4.1.1 "can compile strings"
// check has nowhere else to attach.
//
// `SetModifyCodeGenerationFromStringsCallback` is declared in V8's public
// headers and is already compiled into the prebuilt `rusty_v8.lib` every
// build downloads (verified срез 6: `dumpbin /symbols` on
// `target/*/gn_out/obj/rusty_v8.lib` finds it `External` — defined, not just
// referenced — in `obj/v8/v8_base_without_compiler/api.obj`), but the `v8`
// crate exposes no Rust wrapper for it. Same situation and same fix as
// `cpp/undetectable.cc`: one translation unit whose only job is to let the
// C++ compiler emit a call to an already-linked symbol, by declaring a
// minimal stand-in that mangles the same way V8's real declaration does.
//
// ── The ABI wrinkle (срез 5 theory, срез 6 confirmed by dumpbin disassembly) ──
//
// V8 declares the callback's C++ type as returning
// `ModifyCodeGenerationFromStringsResult` *by value*:
//
//   struct ModifyCodeGenerationFromStringsResult {
//     bool codegen_allowed = false;
//     MaybeLocal<String> modified_source;
//   };
//   using ModifyCodeGenerationFromStringsCallback2 =
//       ModifyCodeGenerationFromStringsResult (*)(Local<Context>,
//                                                  Local<Value>, bool);
//
// The struct has an in-class default member initializer (`= false`), which
// makes its default constructor non-trivial — MSVC x64's calling convention
// then classifies it as "not simple enough to return in registers" and
// passes the caller's storage as a HIDDEN FIRST PARAMETER instead (the
// classic MSVC x64 "return via hidden pointer, shifted into the first
// register" rule for non-trivial aggregates). Срез 6 confirmed this by
// disassembling the real call site: `compiler.obj`'s
// `v8::internal::ModifyCodeGenerationFromStrings` does
// `lea rcx,[rsp+28h]` (the hidden result-buffer pointer) BEFORE loading
// `r8`/the rest of the arguments, then an indirect call through
// `__guard_dispatch_icall_fptr` — exactly the shape below.
//
// `MaybeLocal<T>` itself is layout-identical to `Local<T>` (V8's own
// `v8-local-handle.h`: `MaybeLocal<T>` wraps exactly one `Local<T> local_`
// member) — one pointer, null meaning empty — which is what `Local<T>` below
// models, same stand-in strategy as `cpp/undetectable.cc`.
//
// So: the callback the Rust side implements has the RAW ABI shape (hidden
// result pointer first, everything else shifted right by one), while the
// C++-level TYPE the setter's mangled name expects is the by-value-return
// shape above. `reinterpret_cast` between the two function pointer types
// below bridges that — legal because both spellings compile to the exact
// same machine calling convention; only the C++ type system's view differs.
//
// V8's headers are deliberately NOT included, for the same reason
// `cpp/undetectable.cc` doesn't include them: no `links` key means no
// supported way to ask cargo for V8's include path, and skipping the include
// avoids depending on V8's build-time defines
// (`V8_COMPRESS_POINTERS`/`V8_ENABLE_SANDBOX`/…), which would otherwise have
// to match the prebuilt library exactly. Only pointers cross this boundary,
// the setter itself is a non-virtual, non-inline member function, and the
// stored callback is a bare function pointer — no type layout and no vtable
// participates beyond what's declared here.

namespace v8 {
class Context;
class Value;
class String;

// Stand-in for `v8::Local<T>`/`v8::MaybeLocal<T>`: one pointer, trivially
// copyable (`rusty_v8`'s `support.h` static-asserts `sizeof(Local<T>) ==
// sizeof(T*)`, and `MaybeLocal<T>` is `Local<T>` plus no other state — see
// the module docs above).
template <typename T>
class Local {
 public:
  Local() : val_(nullptr) {}
  T* val_;
};

// Mirrors `v8::ModifyCodeGenerationFromStringsResult` field-for-field. The
// default member initializer on `codegen_allowed` is what makes MSVC return
// this via hidden pointer — kept here so the type's shape (not just its
// size) matches what a caller compiled against the real header expects.
struct ModifyCodeGenerationFromStringsResult {
  bool codegen_allowed = false;
  Local<String> modified_source;
};

class Isolate {
 public:
  void SetModifyCodeGenerationFromStringsCallback(
      ModifyCodeGenerationFromStringsResult (*callback)(Local<Context>,
                                                          Local<Value>,
                                                          bool));
};
}  // namespace v8

// The raw ABI shape MSVC x64 actually generates for the C++-declared
// signature above (see the module docs' "ABI wrinkle" section): the hidden
// result-buffer pointer is the callback's first parameter, and every other
// argument shifts one slot right.
using LumenRawCodegenCallback = void (*)(
    v8::ModifyCodeGenerationFromStringsResult* out, v8::Local<v8::Context>,
    v8::Local<v8::Value>, bool);
// The C++-level type V8's setter actually declares (return-by-value) — only
// used here to get the mangled name and the `reinterpret_cast` target right;
// never called through this type from this translation unit.
using LumenCxxCodegenCallback = v8::ModifyCodeGenerationFromStringsResult (*)(
    v8::Local<v8::Context>, v8::Local<v8::Value>, bool);

extern "C" void
lumen_v8__Isolate__SetModifyCodeGenerationFromStringsCallback(
    v8::Isolate* self, LumenRawCodegenCallback callback) {
  self->SetModifyCodeGenerationFromStringsCallback(
      reinterpret_cast<LumenCxxCodegenCallback>(callback));
}
