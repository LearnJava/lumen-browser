//! Compiles the one C++ translation unit `lumen-js` owns: the local binding for
//! `v8::ObjectTemplate::MarkAsUndetectable()` (`cpp/undetectable.cc`), needed by
//! `document.all`'s `[[IsHTMLDDA]]` semantics (GAP-DOCALLDDA, BUG-1057).
//!
//! Only runs on the `v8-backend` feature — without it the crate links no V8 at
//! all and the symbol the wrapper calls would not exist. The rest of the crate
//! is pure Rust; this build script must stay that small.

fn main() {
    println!("cargo:rerun-if-changed=cpp/undetectable.cc");
    println!("cargo:rerun-if-changed=build.rs");

    // `CARGO_FEATURE_<NAME>` is set by cargo for every enabled feature.
    if std::env::var_os("CARGO_FEATURE_V8_BACKEND").is_none() {
        return;
    }

    cc::Build::new()
        .cpp(true)
        .file("cpp/undetectable.cc")
        .compile("lumen_v8_undetectable");
}
