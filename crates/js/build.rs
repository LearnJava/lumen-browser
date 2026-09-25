//! Compiles the local C++ translation units `lumen-js` owns: bindings for V8
//! methods the `v8` crate's Rust surface doesn't wrap.
//!
//! * `cpp/undetectable.cc` — `v8::ObjectTemplate::MarkAsUndetectable()`, needed
//!   by `document.all`'s `[[IsHTMLDDA]]` semantics (GAP-DOCALLDDA, BUG-1057).
//! * `cpp/codegen_callback.cc` — `v8::Isolate::SetModifyCodeGenerationFromStringsCallback()`,
//!   needed to intercept `eval`/`new Function` for Trusted Types enforcement
//!   (TRUSTEDTYPES-1).
//!
//! Only runs on the `v8-backend` feature — without it the crate links no V8 at
//! all and the symbols the wrappers call would not exist. The rest of the
//! crate is pure Rust; this build script must stay that small.

fn main() {
    println!("cargo:rerun-if-changed=cpp/undetectable.cc");
    println!("cargo:rerun-if-changed=cpp/codegen_callback.cc");
    println!("cargo:rerun-if-changed=build.rs");

    // `CARGO_FEATURE_<NAME>` is set by cargo for every enabled feature.
    if std::env::var_os("CARGO_FEATURE_V8_BACKEND").is_none() {
        return;
    }

    cc::Build::new()
        .cpp(true)
        .file("cpp/undetectable.cc")
        .file("cpp/codegen_callback.cc")
        .compile("lumen_v8_undetectable");
}
