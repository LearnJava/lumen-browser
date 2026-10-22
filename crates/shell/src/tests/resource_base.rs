//! `ResourceBase` resolution: URL/file bases, BUG-440 (query/fragment and
//! scheme-carrying hrefs on a file base) and BUG-752 (`<base href>` adjusts
//! subresource resolution without moving the page origin).
//!
//! Moved verbatim out of `tests/page_pipeline.rs` (file-size gate,
//! docs/lint-policy.md §5.1).

use super::*;

fn expect_resolved_url(base: &str, href: &str) -> String {
    match ResourceBase::Url(base.to_owned()).resolve(href) {
        ResolvedResource::Url(u) => u,
        ResolvedResource::File(_) => panic!("expected Url"),
    }
}

#[test]
fn resource_base_url_absolute_path() {
    assert_eq!(
        expect_resolved_url("https://example.com/path/page.html", "/style.css"),
        "https://example.com/style.css",
    );
}

#[test]
fn resource_base_url_relative_same_dir() {
    assert_eq!(
        expect_resolved_url("https://example.com/path/page.html", "style.css"),
        "https://example.com/path/style.css",
    );
}

#[test]
fn resource_base_url_relative_subdirectory() {
    assert_eq!(
        expect_resolved_url("https://example.com/path/page.html", "css/main.css"),
        "https://example.com/path/css/main.css",
    );
}

#[test]
fn resource_base_url_root_base() {
    assert_eq!(
        expect_resolved_url("https://example.com/", "style.css"),
        "https://example.com/style.css",
    );
}

#[test]
fn resource_base_url_http_scheme_with_port() {
    assert_eq!(
        expect_resolved_url("http://localhost:8080/index.html", "/css/app.css"),
        "http://localhost:8080/css/app.css",
    );
}

#[test]
fn resource_base_url_absolute_href_passthrough() {
    // Абсолютный href с http/https-схемой ловится в начале ResourceBase::resolve
    // до Url::resolve — это позволяет href с другим scheme быть видимым как Url,
    // даже если base — File.
    let base = ResourceBase::Url("https://example.com/".to_owned());
    let res = base.resolve("https://cdn.example.com/style.css");
    match res {
        ResolvedResource::Url(u) => assert_eq!(u, "https://cdn.example.com/style.css"),
        ResolvedResource::File(_) => panic!("expected Url"),
    }
}

#[test]
fn resource_base_file_resolves_relative() {
    let base = ResourceBase::File(PathBuf::from("samples/page.html"));
    let res = base.resolve("style.css");
    match res {
        ResolvedResource::File(p) => {
            assert_eq!(p, PathBuf::from("samples/style.css"));
        }
        ResolvedResource::Url(_) => panic!("expected File"),
    }
}

#[test]
fn resource_base_file_absolute_url_passthrough() {
    let base = ResourceBase::File(PathBuf::from("samples/page.html"));
    let res = base.resolve("https://cdn.example.com/style.css");
    match res {
        ResolvedResource::Url(u) => assert_eq!(u, "https://cdn.example.com/style.css"),
        ResolvedResource::File(_) => panic!("expected Url"),
    }
}

#[test]
fn resolve_str_url_base_relative() {
    let base = ResourceBase::Url("https://example.com/path/page.html".to_owned());
    assert_eq!(
        base.resolve_str("style.css"),
        "https://example.com/path/style.css"
    );
}

#[test]
fn resolve_str_url_base_absolute_passthrough() {
    let base = ResourceBase::Url("https://example.com/page.html".to_owned());
    assert_eq!(
        base.resolve_str("https://cdn.example.com/lib.js"),
        "https://cdn.example.com/lib.js"
    );
}

#[test]
fn resolve_str_file_base_yields_path_string() {
    let base = ResourceBase::File(PathBuf::from("/home/user/page.html"));
    let result = base.resolve_str("style.css");
    assert!(result.ends_with("style.css"), "got: {result}");
}

// ── BUG-440: File base must not smuggle a query/fragment into the filename,
// nor join an href that carries its own scheme onto its directory ─────────

#[test]
fn resolve_file_base_get_query_is_not_part_of_filename() {
    let base = ResourceBase::File(PathBuf::from("D:/tmp/form.html"));
    match base.resolve("target.html?q=hello") {
        ResolvedResource::File(p) => assert_eq!(p, PathBuf::from("D:/tmp/target.html")),
        other => panic!("expected File, got {other:?}"),
    }
}

#[test]
fn resolve_file_base_strips_fragment() {
    let base = ResourceBase::File(PathBuf::from("D:/tmp/form.html"));
    match base.resolve("target.html#sec") {
        ResolvedResource::File(p) => assert_eq!(p, PathBuf::from("D:/tmp/target.html")),
        other => panic!("expected File, got {other:?}"),
    }
}

#[test]
fn resolve_file_base_query_only_href_reloads_same_file() {
    let base = ResourceBase::File(PathBuf::from("D:/tmp/form.html"));
    match base.resolve("?q=hello") {
        ResolvedResource::File(p) => assert_eq!(p, PathBuf::from("D:/tmp/form.html")),
        other => panic!("expected File, got {other:?}"),
    }
}

#[test]
fn resolve_file_base_fragment_only_href_reloads_same_file() {
    let base = ResourceBase::File(PathBuf::from("D:/tmp/form.html"));
    match base.resolve("#sec") {
        ResolvedResource::File(p) => assert_eq!(p, PathBuf::from("D:/tmp/form.html")),
        other => panic!("expected File, got {other:?}"),
    }
}

#[test]
fn resolve_file_base_about_blank_is_a_url_not_a_path() {
    let base = ResourceBase::File(PathBuf::from("D:/tmp/form.html"));
    match base.resolve("about:blank") {
        ResolvedResource::Url(u) => assert_eq!(u, "about:blank"),
        other => panic!("expected Url, got {other:?}"),
    }
}

#[test]
fn resolve_file_base_data_url_is_a_url_not_a_path() {
    let base = ResourceBase::File(PathBuf::from("D:/tmp/form.html"));
    match base.resolve("data:text/html,x") {
        ResolvedResource::Url(u) => assert_eq!(u, "data:text/html,x"),
        other => panic!("expected Url, got {other:?}"),
    }
}

#[test]
fn resolve_file_base_plain_relative_href_unaffected() {
    let base = ResourceBase::File(PathBuf::from("D:/tmp/form.html"));
    match base.resolve("sub/../target.html") {
        ResolvedResource::File(p) => assert_eq!(p, PathBuf::from("D:/tmp/sub/../target.html")),
        other => panic!("expected File, got {other:?}"),
    }
}

/// GET form submission end-to-end through `make_get_url` + `resolve_str` —
/// the exact sequence BUG-440 was filed against (`action="target.html"`,
/// field `q=hello`, page opened as `file://`).
#[test]
fn bug440_get_form_submission_resolves_to_the_target_file() {
    let base = ResourceBase::File(PathBuf::from("D:/tmp/form437target.html"));
    let get_url = forms::make_get_url("target.html", "q=hello");
    assert_eq!(base.resolve_str(&get_url), "D:/tmp\\target.html");
}

/// The other BUG-440 symptom: `action="about:blank"` must resolve to a
/// literal `"about:blank"` string so `PageSource::from_arg` recognizes it,
/// not a path like `"D:/tmp\about:blank"`.
#[test]
fn bug440_get_form_submission_with_scheme_action_resolves_to_url() {
    let base = ResourceBase::File(PathBuf::from("D:/tmp/form437target.html"));
    let get_url = forms::make_get_url("about:blank", "");
    assert_eq!(base.resolve_str(&get_url), "about:blank");
}

/// A `file:` href on a local page is still a local file: resolving it to a
/// `Url` would only move the defect one caller along, where `PathBuf` gets
/// the whole `file://...` string verbatim (the BUG-651 shape).
#[test]
fn resolve_file_base_file_scheme_href_yields_a_path() {
    let base = ResourceBase::File(PathBuf::from("D:/tmp/form.html"));
    match base.resolve("file:///D:/other/x.html") {
        ResolvedResource::File(p) => assert_eq!(p, PathBuf::from("D:/other/x.html")),
        other => panic!("expected File, got {other:?}"),
    }
}

/// The path component of a URL reference is percent-encoded, so a file whose
/// name holds a space arrives as `%20` and has to be decoded before the
/// filesystem is asked for it.
#[test]
fn resolve_file_base_decodes_percent_escapes() {
    let base = ResourceBase::File(PathBuf::from("D:/tmp/form.html"));
    match base.resolve("my%20file.html") {
        ResolvedResource::File(p) => assert_eq!(p, PathBuf::from("D:/tmp/my file.html")),
        other => panic!("expected File, got {other:?}"),
    }
}

/// A `%` that is not a valid escape is a legal filename character and must
/// survive verbatim — decoding it away would name a different file.
#[test]
fn resolve_file_base_keeps_a_lone_percent() {
    let base = ResourceBase::File(PathBuf::from("D:/tmp/form.html"));
    match base.resolve("100%done.html") {
        ResolvedResource::File(p) => assert_eq!(p, PathBuf::from("D:/tmp/100%done.html")),
        other => panic!("expected File, got {other:?}"),
    }
}

/// A Windows drive letter is not a URI scheme: one character cannot be one,
/// and reading `D:/docs/x.html` as a `d:` URL would send a perfectly good
/// local href to the network layer instead of to disk.
#[test]
fn resolve_file_base_drive_letter_is_not_a_scheme() {
    let base = ResourceBase::File(PathBuf::from("D:/tmp/form.html"));
    match base.resolve("D:/docs/x.html") {
        ResolvedResource::File(p) => assert_eq!(p, PathBuf::from("D:/docs/x.html")),
        other => panic!("expected File, got {other:?}"),
    }
}

// -- BUG-752: `<base href>` adjusts subresource resolution -------------------

#[test]
fn resolve_as_base_url_relative_href_becomes_new_base() {
    let page = ResourceBase::Url("https://example.com/a/page.html".to_owned());
    let based = page.resolve_as_base("/other/");
    assert_eq!(
        based.resolve_str("pic.png"),
        "https://example.com/other/pic.png",
        "a relative href now resolves against the <base href>, not the page URL"
    );
}

#[test]
fn resolve_as_base_url_absolute_href_switches_origin() {
    let page = ResourceBase::Url("https://example.com/a/page.html".to_owned());
    let based = page.resolve_as_base("https://cdn.example/assets/");
    assert_eq!(based.resolve_str("pic.png"), "https://cdn.example/assets/pic.png");
}

#[test]
fn resolve_as_base_file_relative_href_moves_directory() {
    let page = ResourceBase::File(PathBuf::from("D:/site/a/page.html"));
    let based = page.resolve_as_base("other/");
    match based.resolve("pic.png") {
        ResolvedResource::File(p) => assert_eq!(p, PathBuf::from("D:/site/a/other/pic.png")),
        other => panic!("expected File, got {other:?}"),
    }
}

#[test]
fn effective_base_falls_back_to_page_base_without_a_base_element() {
    let doc = lumen_html_parser::parse("<html><body></body></html>");
    let page = ResourceBase::Url("https://example.com/a/page.html".to_owned());
    let eff = effective_base(&doc, &page);
    assert_eq!(eff.resolve_str("pic.png"), "https://example.com/a/pic.png");
}

#[test]
fn effective_base_reads_base_href_from_the_document() {
    let doc = lumen_html_parser::parse(
        "<html><head><base href=\"/other/\"></head><body></body></html>",
    );
    let page = ResourceBase::Url("https://example.com/a/page.html".to_owned());
    let eff = effective_base(&doc, &page);
    assert_eq!(eff.resolve_str("pic.png"), "https://example.com/other/pic.png");
}

#[test]
fn effective_base_does_not_change_page_origin() {
    // <base href> to a different origin must resolve subresources there, but
    // must not be mistaken for the document's own Origin (CSP / mixed content
    // identity stays with the real page URL, per HTML LS — <base> only
    // affects reference resolution, not the document's origin).
    let doc = lumen_html_parser::parse(
        "<html><head><base href=\"https://cdn.example/\"></head><body></body></html>",
    );
    let page = ResourceBase::Url("https://example.com/a/page.html".to_owned());
    let eff = effective_base(&doc, &page);
    assert_eq!(eff.resolve_str("pic.png"), "https://cdn.example/pic.png");
    assert_ne!(
        page.origin(), eff.origin(),
        "the un-adjusted base's Origin must stay example.com, not follow <base> to cdn.example"
    );
}
