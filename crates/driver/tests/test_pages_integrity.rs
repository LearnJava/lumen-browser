//! Integrity guard for `graphic_tests/*.html` source files.
//!
//! BUG-119 regression: a bulk title-tag edit (88cdb9e1) accidentally left a
//! raw U+0001 control byte inside `<head>` of 17 test pages. Per the HTML
//! parsing spec a non-whitespace character token closes `<head>`, so the
//! byte became renderable body text — Lumen drew a 19.2px text line at the
//! top of each page, shifting all content down and failing 6 borderline
//! run.py tests (TEST-27/28/29/40/41/68). Test pages are ground truth and
//! must stay byte-clean.

use std::fs;
use std::path::{Path, PathBuf};

/// Workspace root (two parents up from the driver crate manifest).
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

/// No graphic test page may contain C0 control bytes other than
/// tab (0x09), LF (0x0A), and CR (0x0D). Any other control byte inside
/// `<head>` silently closes the head per the HTML spec and renders as body
/// text, corrupting the page's layout ground truth.
#[test]
fn graphic_test_pages_have_no_stray_control_bytes() {
    let dir = workspace_root().join("graphic_tests");
    let mut offenders = Vec::new();
    for entry in fs::read_dir(&dir).expect("read graphic_tests dir") {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("html") {
            continue;
        }
        let bytes = fs::read(&path).expect("read test page");
        if let Some(pos) = bytes
            .iter()
            .position(|&b| b < 0x20 && b != 0x09 && b != 0x0A && b != 0x0D)
        {
            offenders.push(format!(
                "{}: byte 0x{:02X} at offset {pos}",
                path.file_name().unwrap().to_string_lossy(),
                bytes[pos]
            ));
        }
    }
    assert!(
        offenders.is_empty(),
        "graphic test pages contain stray control bytes (corrupted ground truth):\n{}",
        offenders.join("\n")
    );
}
