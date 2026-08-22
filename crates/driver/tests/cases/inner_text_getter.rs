//! BUG-413 slice 2 — `HTMLElement.innerText` / `outerText` must report *rendered*
//! text, over the layout the engine actually produced.
//!
//! The shim-level unit tests in `lumen-js` stand a snapshot up by hand, so they
//! prove the collection steps and nothing about the bridge underneath. This file
//! is the other half: `InProcessSession::navigate_html` runs the real pipeline —
//! style, layout, then `update_layout_rects`/`update_computed_styles` — and the
//! getter is read back through `eval`, so a `display: none` subtree is missing
//! from the snapshot because layout produced no box for it, not because a test
//! helper decided to omit it.
//!
//! Each assertion is chosen to differ from `textContent`, which is what the
//! property returned before the slice and what it still returns for a node with
//! no box.
#![cfg(feature = "v8")]

use lumen_driver::{BrowserSession, InProcessSession};

/// Strip the JSON quoting `BrowserSession::eval` applies to string results, then
/// undo the escaping it added — `\n` and `\t` arrive as two characters each.
fn unquote(s: &str) -> String {
    s.trim_matches('"')
        .replace("\\n", "\n")
        .replace("\\t", "\t")
        .replace("\\\"", "\"")
}

fn rendered_text(session: &mut InProcessSession, selector: &str) -> String {
    unquote(
        &session
            .eval(&format!("document.querySelector('{selector}').innerText"))
            .expect("eval innerText"),
    )
}

const PAGE: &str = r#"<html><head><style>
  #gone  { display: none }
  #ghost { visibility: hidden }
  #raw   { white-space: pre }
  #shout { text-transform: uppercase }
</style></head><body>
<div id="blocks"><p>hello <b>world</b></p><p>second</p></div>
<div id="hiding">A<span id="gone">X</span><span id="ghost">Y</span>B</div>
<div id="spaces">  a  <span>  b  </span>
  c  </div>
<div id="breaks">a<br>b <div>c</div></div>
<pre id="raw">  x  y  </pre>
<div id="shout">go</div>
<table id="grid"><tbody><tr><td>a</td><td>b</td></tr><tr><td>c</td></tr></tbody></table>
</body></html>"#;

#[test]
fn inner_text_reports_rendered_text_over_real_layout() {
    let mut session = InProcessSession::new();
    session.navigate_html(PAGE).expect("navigate_html");

    // Two `<p>`s: a required line break count of 2 at each end, stripped at the
    // outer edges and merged into exactly two line feeds in between.
    assert_eq!(rendered_text(&mut session, "#blocks"), "hello world\n\nsecond");

    // `display: none` never reaches the layout tree, `visibility: hidden` does
    // but must not contribute text. `textContent` here is "AXYB".
    assert_eq!(rendered_text(&mut session, "#hiding"), "AB");

    // Collapsing spans inline boundaries and drops the spaces at both ends.
    assert_eq!(rendered_text(&mut session, "#spaces"), "a b c");

    // `<br>` contributes a literal line feed, the nested `<div>` a break count.
    assert_eq!(rendered_text(&mut session, "#breaks"), "a\nb\nc");

    // `white-space: pre` and `text-transform` are read off the computed style
    // the element was laid out with.
    assert_eq!(rendered_text(&mut session, "#raw"), "  x  y  ");
    assert_eq!(rendered_text(&mut session, "#shout"), "GO");

    // A tab after every cell but the last of its row; the rows break the line.
    assert_eq!(rendered_text(&mut session, "#grid"), "a\tb\nc");
}

#[test]
fn outer_text_getter_matches_inner_text_and_both_are_html_only() {
    let mut session = InProcessSession::new();
    session.navigate_html(PAGE).expect("navigate_html");

    let same = session
        .eval(
            "(function () { var e = document.getElementById('blocks'); \
               return e.outerText === e.innerText && e.innerText !== e.textContent; })()",
        )
        .expect("eval outerText");
    assert_eq!(same, "true", "outerText must run the same getter steps as innerText");

    // Both are `HTMLElement` members, and the wrapper factory is shared with
    // foreign content — so an SVG element must read as `undefined`.
    let absent = session
        .eval(
            "(function () { \
               var s = document.createElementNS('http://www.w3.org/2000/svg', 'svg'); \
               s.textContent = 'abc'; document.body.appendChild(s); \
               return s.innerText === undefined && s.outerText === undefined; })()",
        )
        .expect("eval svg");
    assert_eq!(absent, "true", "innerText/outerText must not exist outside HTML");
}

#[test]
fn detached_element_falls_back_to_text_content() {
    let mut session = InProcessSession::new();
    session.navigate_html(PAGE).expect("navigate_html");

    // Step 1 of the getter: a node the layout engine produced no box for answers
    // `textContent`, whitespace intact — the double space would have collapsed
    // on the rendered path.
    let ok = session
        .eval(
            "(function () { var d = document.createElement('div'); \
               d.innerHTML = 'a  <span>b</span>'; \
               return d.innerText === d.textContent && d.innerText.indexOf('a  b') >= 0; })()",
        )
        .expect("eval detached");
    assert_eq!(ok, "true", "a box-less element must answer textContent");
}

/// Regression guard for the bridge itself: an inline element owns no box, so the
/// engine publishes no computed style for it — the style that governs its text
/// lives on the **text node**. Getting this backwards is how slice 2's first
/// attempt lost `<b>world</b>` from `#blocks` entirely.
#[test]
fn inline_element_has_no_snapshot_entry_but_its_text_node_does() {
    let mut session = InProcessSession::new();
    session.navigate_html(PAGE).expect("navigate_html");

    let inline_element = session
        .eval("_lumen_get_computed_style(document.querySelector('#blocks b').__nid__, 'visibility')")
        .expect("eval inline element style");
    assert_eq!(inline_element, "\"\"", "an inline element must have no entry");

    let inline_text = session
        .eval(
            "_lumen_get_computed_style(\
               document.querySelector('#blocks b').firstChild.__nid__, 'visibility')",
        )
        .expect("eval inline text style");
    assert_eq!(inline_text, "\"visible\"", "its text node must carry the style");

    // And the block that does own a box keeps its full ~55-property entry.
    let block = session
        .eval("_lumen_get_computed_style(document.getElementById('blocks').__nid__, 'display')")
        .expect("eval block style");
    assert_eq!(block, "\"block\"");
}
