//! V8 build spike smoke test (S0).
//!
//! Verifies that the `v8` crate compiles on Windows MSVC, the platform
//! initialises, an isolate can be created, and a trivial expression evaluates
//! correctly. This is the go/no-go gate before any porting work (slices S1+).
//!
//! # Windows build note
//! v8's build.rs creates a `gn_root` symlink when the cargo target dir and
//! the cargo registry are on different drives. This requires
//! `SeCreateSymbolicLinkPrivilege` (Windows Developer Mode or admin rights).
//! Workaround: set `CARGO_TARGET_DIR` to a path on the same drive as the
//! cargo registry (default `C:\Users\…\.cargo`), e.g.
//!   `CARGO_TARGET_DIR=C:\tmp\lumen-v8-target cargo test -p lumen-js --features v8-backend`
#![cfg(feature = "v8-backend")]

use std::sync::RwLock;

/// Guards concurrent test access — parallel reads are fine, init is exclusive.
static PROCESS_LOCK: RwLock<()> = RwLock::new(());

/// Delegate platform init to the shared path in `v8_runtime` so there is
/// exactly one `V8::initialize_platform` call per process (S1 invariant).
fn init_v8() {
    lumen_js::v8_runtime::ensure_v8_platform();
}

/// Smoke test: eval `1+1` → 2.0 inside a fresh isolate.
#[test]
fn v8_eval_one_plus_one() {
    init_v8();
    let _guard = PROCESS_LOCK.read().unwrap();

    let isolate = &mut v8::Isolate::new(Default::default());
    v8::scope!(let scope, isolate);

    let context = v8::Context::new(scope, Default::default());
    let scope = &mut v8::ContextScope::new(scope, context);

    let source = v8::String::new(scope, "1+1").unwrap();
    let script = v8::Script::compile(scope, source, None)
        .expect("v8: script compile failed");
    let result = script.run(scope).expect("v8: script run failed");

    let number: v8::Local<v8::Number> = result
        .try_into()
        .expect("v8: result is not a Number");
    assert_eq!(number.value(), 2.0, "1+1 must equal 2 in V8");
}

/// Verify that string round-trips through a V8 isolate work.
#[test]
fn v8_string_round_trip() {
    init_v8();
    let _guard = PROCESS_LOCK.read().unwrap();

    let isolate = &mut v8::Isolate::new(Default::default());
    v8::scope!(let scope, isolate);

    let context = v8::Context::new(scope, Default::default());
    let scope = &mut v8::ContextScope::new(scope, context);

    let source = v8::String::new(scope, r#""hello from V8""#).unwrap();
    let script = v8::Script::compile(scope, source, None).unwrap();
    let result = script.run(scope).unwrap();
    let s = result.to_string(scope).unwrap();
    assert_eq!(s.to_rust_string_lossy(scope), "hello from V8");
}
