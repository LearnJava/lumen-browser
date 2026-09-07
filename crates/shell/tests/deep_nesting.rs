//! BUG-1027 regression: a deeply nested page must not kill the process.
//!
//! Box-tree build, style passes and the DOM import recurse on DOM depth at
//! ~10.4 KB of stack per level, so a thread left on a platform default dies
//! on ordinary pages — and dies *silently*: the runtime's stack-overflow
//! handler calls `abort()`, so there is no panic, no backtrace and no exit
//! code beyond `SIGABRT`. Every entry point below therefore has to run its
//! traversals on a thread carrying `lumen_core::DEEP_TREE_STACK_BYTES`.
//!
//! `NESTING` is deliberately past both defaults that used to bite (2 MiB ≈ 190
//! levels for a spawned thread, 8 MiB ≈ 790 for the Unix main thread) while
//! staying clear of the O(N²) margin-collapsing cost BUG-1026 removed on
//! 2026-09-07 — this test guards the stack, not the clock.
//!
//! **Not covered here:** the live window. Its final pipeline runs on
//! `lumen-pipeline` (`app/user_event.rs`) and needs a real event loop and a
//! GPU surface, so it cannot run in `cargo test`. Verify it by hand after
//! touching thread setup: `lumen <page with NESTING nested divs>` must open a
//! window instead of printing `has overflowed its stack`.

use std::path::PathBuf;
use std::process::Command;

/// Nesting depth of the generated page. Above every historical stack limit.
const NESTING: usize = 2000;

/// Writes a page into cargo's per-test temp dir and returns its path.
///
/// Errors are returned rather than raised: `panic!` outside a `#[test]` body
/// is production code as far as `clippy::panic` is concerned, and the callers
/// below are test bodies, where unwrapping is allowed.
fn write_nested_page(name: &str, body: &str) -> std::io::Result<PathBuf> {
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let html = format!("<!doctype html><meta charset=utf-8><title>deep</title>{body}");
    std::fs::write(&path, html)?;
    Ok(path)
}

/// Static nesting: `<div>`×`NESTING`, no scripts.
fn static_nesting() -> String {
    format!("{}x{}", "<div>".repeat(NESTING), "</div>".repeat(NESTING))
}

/// Runs the browser binary and returns (exit code, stderr).
fn run_lumen(args: &[&str]) -> std::io::Result<(Option<i32>, String)> {
    let out = Command::new(env!("CARGO_BIN_EXE_lumen")).args(args).output()?;
    Ok((out.status.code(), String::from_utf8_lossy(&out.stderr).into_owned()))
}

/// A crash here is a stack overflow until proven otherwise — say so, and show
/// the two lines the runtime does print.
fn assert_survived(label: &str, code: Option<i32>, stderr: &str) {
    assert!(
        !stderr.contains("overflowed its stack"),
        "{label}: стек переполнен на {NESTING} уровнях вложенности\n{stderr}"
    );
    assert_eq!(code, Some(0), "{label}: выход с кодом {code:?}\n{stderr}");
}

#[test]
fn dump_layout_survives_deep_nesting() {
    let page = write_nested_page("deep-static-layout.html", &static_nesting()).unwrap();
    let (code, stderr) = run_lumen(&["--dump-layout", &page.to_string_lossy()]).unwrap();
    assert_survived("--dump-layout", code, &stderr);
}

#[test]
fn screenshot_survives_deep_nesting() {
    let page = write_nested_page("deep-static-shot.html", &static_nesting()).unwrap();
    let png = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("deep-static.png");
    let (code, stderr) =
        run_lumen(&["--screenshot", &png.to_string_lossy(), &page.to_string_lossy()]).unwrap();
    assert_survived("--screenshot", code, &stderr);
}

/// The same depth reached through `innerHTML`, i.e. through
/// `lumen_js::v8_runtime::dom_helpers::import_node` on the `lumen-v8` thread —
/// a second recursion, with its own stack, that BUG-1027 also had to cover.
#[cfg(feature = "v8")]
#[test]
fn inner_html_import_survives_deep_nesting() {
    let script = format!(
        "<div id=host></div><script>\
         document.getElementById('host').innerHTML = \
         '<div>'.repeat({NESTING}) + 'x' + '</div>'.repeat({NESTING});\
         </script>"
    );
    let page = write_nested_page("deep-innerhtml.html", &script).unwrap();
    let png = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("deep-innerhtml.png");
    let (code, stderr) =
        run_lumen(&["--screenshot", &png.to_string_lossy(), &page.to_string_lossy()]).unwrap();
    assert_survived("innerHTML + --screenshot", code, &stderr);
}
