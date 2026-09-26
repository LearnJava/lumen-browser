//! BUG-1127: относительные `url()` внешнего листа резолвятся от URL самого
//! листа, а не документа (CSS Values §4.3).

use super::*;
use crate::css_url_rebase::rebase_css_urls;
use super::page_resources::null_sink;

fn sheet_base() -> ResourceBase {
    ResourceBase::Url("https://example.com/css/s.css".to_owned())
}

#[test]
fn relative_urls_resolve_against_sheet_url() {
    let css = r#"@font-face { font-family: F; src: url("fonts/f.ttf") format("truetype"); }
#b { background-image: url(bg.svg); cursor: URL( '../c.cur' ), auto; }"#;
    let out = rebase_css_urls(css, &sheet_base());
    assert!(out.contains(r#"url("https://example.com/css/fonts/f.ttf") format("truetype")"#), "{out}");
    assert!(out.contains(r#"background-image: url("https://example.com/css/bg.svg");"#), "{out}");
    assert!(out.contains(r#"cursor: url("https://example.com/c.cur"), auto;"#), "{out}");
}

#[test]
fn absolute_fragment_and_empty_urls_stay_verbatim() {
    let css = r#"a { mask: url(#m); b: url(data:image/png;base64,AAAA); c: url("https://cdn.test/x.png"); d: url(""); e: url(//cdn.test/y.png); }"#;
    let out = rebase_css_urls(css, &sheet_base());
    for kept in ["url(#m)", "url(data:image/png;base64,AAAA)", r#"url("https://cdn.test/x.png")"#, r#"url("")"#] {
        assert!(out.contains(kept), "{kept} must stay verbatim: {out}");
    }
    // Протокол-относительный адрес без схемы — тоже относительный URL.
    assert!(out.contains(r#"url("https://cdn.test/y.png")"#), "{out}");
}

#[test]
fn url_lookalikes_in_comments_strings_and_idents_are_untouched() {
    let css = r#"/* url(a.png) */ a::before { content: "url(b.png)"; x: my-url(c.png); }"#;
    assert_eq!(rebase_css_urls(css, &sheet_base()), css);
}

#[test]
fn file_base_rebases_to_file_url_that_resolves_back_to_path() {
    let dir = std::env::temp_dir().join("lumen_bug1127_rebase");
    let base = ResourceBase::File(dir.join("css").join("s.css"));
    let out = rebase_css_urls("a { background: url(img/x.png) }", &base);
    let url = out.split('"').nth(1).expect("quoted rebased url");
    assert!(url.starts_with("file:"), "{out}");
    // Документ в корне `dir` резолвит переписанный адрес в файл под `css/`.
    let doc_base = ResourceBase::File(dir.join("page.html"));
    match doc_base.resolve(url) {
        ResolvedResource::File(p) => assert_eq!(p, dir.join("css").join("img").join("x.png")),
        other => panic!("expected a file path, got {other:?}"),
    }
}

/// Сквозной путь каскада: `<link href="css/s.css">` из документа в корне —
/// склеенный текст несёт адреса под `css/`, а не от документа.
#[test]
fn linked_sheet_urls_resolve_against_sheet_directory() {
    let dir = std::env::temp_dir().join("lumen_bug1127_linked");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("css")).unwrap();
    std::fs::write(
        dir.join("css").join("s.css"),
        r#"@import "sub/i.css"; @font-face { font-family: G5; src: url("fonts/f.ttf"); }"#,
    )
    .unwrap();
    std::fs::create_dir_all(dir.join("css").join("sub")).unwrap();
    std::fs::write(dir.join("css").join("sub").join("i.css"), "#b { background-image: url(bg.svg) }").unwrap();
    let doc = lumen_html_parser::parse(r#"<html><head><link rel="stylesheet" href="css/s.css"></head><body></body></html>"#);
    let base = ResourceBase::File(dir.join("page.html"));
    let ctx = screen_media_context(Size::new(1024.0, 720.0), false);
    let (css, _outcomes, _blocked) = load_linked_stylesheets(&doc, &base, &null_sink(), None, &ctx);
    let resolved: Vec<_> = css
        .split('"')
        .filter(|s| s.starts_with("file:"))
        .map(|u| match base.resolve(u) {
            ResolvedResource::File(p) => p,
            other => panic!("expected a file path, got {other:?}"),
        })
        .collect();
    assert!(resolved.contains(&dir.join("css").join("fonts").join("f.ttf")), "{css}");
    assert!(resolved.contains(&dir.join("css").join("sub").join("bg.svg")), "{css}");
}
