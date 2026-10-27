//! BUG-1554 — which `<style>`/`<link rel=stylesheet>` owners take part in the cascade: the preferred
//! style sheet set (`title`/`alternate`), `HTMLLinkElement.disabled`, `CSSStyleSheet.disabled`,
//! `HTMLStyleElement.disabled`, and a `<link>` sheet that arrives after the shell's cascade pass.

use super::v8_bug935_s55_content_journal::{page, runtime};
use super::*;

/// Run `script` (a function body returning a string) on a page whose shell sheet is `shell_css`.
fn run(shell_css: &str, script: &str) -> String {
    let rt = runtime(page());
    rt.update_stylesheet(Arc::new(lumen_css_parser::parse(shell_css)));
    let src = format!(
        "(function() {{
            var p = document.createElement('p'); p.id = 'p'; document.body.appendChild(p);
            function mk(tag, attrs, text) {{
                var e = document.createElement(tag);
                for (var k in attrs) e.setAttribute(k, attrs[k]);
                if (text) e.appendChild(document.createTextNode(text));
                document.body.appendChild(e);
                return e;
            }}
            function color() {{ return getComputedStyle(p).color; }}
            {script}
        }})()"
    );
    match rt.eval(&src).unwrap_or_else(|e| panic!("{e:?}\n{src}")) {
        lumen_core::JsValue::String(s) => s,
        other => panic!("expected a string, got {other:?}"),
    }
}

const GREEN: &str = "rgb(0, 128, 0)";

/// The first titled sheet made is the preferred set (a titled `<link rel=author>` is not a sheet); a
/// later titled one is an alternate.
#[test]
fn only_the_preferred_titled_style_applies() {
    let out = run(
        "",
        "mk('link', {rel: 'author', title: 'Someone', href: 'mailto:a@b.c'});
         mk('style', {}, '');
         mk('style', {title: ''}, '');
         mk('style', {title: 'Preferred'}, '#p { color: green }');
         mk('style', {title: 'Not preferred'}, '#p { color: red }');
         return color();",
    );
    assert_eq!(out, GREEN);
}

/// Creation order, not tree order, picks the set (WPT `preferred-stylesheet-reversed-order`).
#[test]
fn preferred_set_follows_creation_order_not_tree_order() {
    let out = run(
        "",
        "var a = document.createElement('style'); a.setAttribute('title', 'preferred');
         a.appendChild(document.createTextNode('#p { color: green }'));
         var b = document.createElement('style'); b.setAttribute('title', 'notpreferred');
         b.appendChild(document.createTextNode('#p { color: red }'));
         document.body.insertBefore(a, document.body.firstChild);
         document.body.insertBefore(b, document.body.firstChild);
         return color();",
    );
    assert_eq!(out, GREEN);
}

/// `sheet.disabled` / `style.disabled` take a sheet the shell already put into the cascade out and back.
#[test]
fn style_disabled_removes_and_restores_a_cascaded_sheet() {
    let out = run(
        "#p { color: green }\n",
        "var s = mk('style', {}, '#p { color: green }');
         var before = color();
         s.disabled = true;
         var off = color() + '|' + s.sheet.disabled + '|' + s.disabled;
         s.sheet.disabled = false;
         return [before, off, color(), s.disabled].join(';');",
    );
    assert_eq!(out, format!("{GREEN};rgb(0, 0, 0)|true|true;{GREEN};false"));
}

/// An alternate `<link>` is listed but disabled; `link.disabled = false` is only meaningful once the
/// attribute is present, and then marks the sheet explicitly enabled.
#[test]
fn link_disabled_reflects_the_attribute_and_enables_explicitly() {
    let out = run(
        "",
        "var l = document.createElement('link');
         l.setAttribute('rel', 'alternate stylesheet'); l.setAttribute('title', 'alt');
         l.setAttribute('href', 'data:text/css,x');
         l.disabled = false;
         var absent = l.disabled + '/' + l.hasAttribute('disabled');
         l.disabled = true;
         var set = l.disabled + '/' + l.hasAttribute('disabled');
         l.disabled = false;
         return [absent, set, l.disabled, l.hasAttribute('disabled')].join(';');",
    );
    assert_eq!(out, "false/false;true/true;false;false");
}

/// A `<link>` sheet that arrives after the shell's cascade pass joins `document.styleSheets` and the
/// cascade; an alternate one does so only when explicitly enabled; `link.disabled = true` removes it.
#[test]
fn loaded_link_joins_the_cascade_and_leaves_with_disabled() {
    let out = run(
        "",
        "var l = mk('link', {rel: 'stylesheet', href: 'a.css'});
         var nid = _lumen_reflect_nid(l);
         var ok = _lumen_stylesheet_register_link(nid, '#p { color: green }');
         var on = [ok, document.styleSheets.length, l.sheet.ownerNode === l, color()].join();
         l.disabled = true;
         var off = [document.styleSheets.length, l.sheet, color()].join();
         return on + ';' + off;",
    );
    assert_eq!(out, format!("true,1,true,{GREEN};0,,rgb(0, 0, 0)"));
}

#[test]
fn alternate_link_is_listed_but_does_not_apply_until_enabled() {
    let out = run(
        "",
        "var l = mk('link', {rel: 'alternate stylesheet', title: 'alt', href: 'a.css'});
         _lumen_stylesheet_register_link(_lumen_reflect_nid(l), '#p { color: green }');
         var before = [document.styleSheets.length, l.sheet.disabled, color()].join();
         l.sheet.disabled = false;
         return before + ';' + [l.sheet.disabled, color()].join();",
    );
    assert_eq!(out, format!("1,true,rgb(0, 0, 0);false,{GREEN}"));
}

/// WPT `HTMLLinkElement-disabled-001`: the sheet colours the root element, the `<link>` sits in `<head>`.
#[test]
fn disabling_a_loaded_link_in_head_restyles_the_root() {
    let out = run(
        "",
        "var head = document.createElement('head');
         document.documentElement.insertBefore(head, document.body);
         var l = document.createElement('link');
         l.setAttribute('rel', 'stylesheet'); l.setAttribute('href', 'a.css');
         head.appendChild(l);
         function bg() { return getComputedStyle(document.documentElement).backgroundColor; }
         var s0 = bg();
         _lumen_stylesheet_register_link(_lumen_reflect_nid(l), 'html { background: green }');
         var s1 = bg();
         l.disabled = true;
         return [s0, s1, bg()].join(';');",
    );
    assert_eq!(out, "rgba(0, 0, 0, 0);rgb(0, 128, 0);rgba(0, 0, 0, 0)");
}

/// The shell may rebuild its cascade from the DOM while a script-enabled `<link>` is on, baking the
/// sheet's text in; disabling the link afterwards must cut that text out of the flush's sheet.
#[test]
fn disabling_a_link_whose_text_the_shell_already_cascaded_removes_it() {
    let out = run(
        "html { background: green }\n",
        "var head = document.createElement('head');
         document.documentElement.insertBefore(head, document.body);
         var l = document.createElement('link');
         l.setAttribute('rel', 'stylesheet'); l.setAttribute('href', 'a.css');
         head.appendChild(l);
         function bg() { return getComputedStyle(document.documentElement).backgroundColor; }
         _lumen_stylesheet_register_link(_lumen_reflect_nid(l), 'html { background: green }\\n');
         var on = bg();
         l.disabled = true;
         var off = bg();
         l.disabled = false;
         return [on, off, document.styleSheets.length].join(';');",
    );
    assert_eq!(out, "rgb(0, 128, 0);rgba(0, 0, 0, 0);0");
}
