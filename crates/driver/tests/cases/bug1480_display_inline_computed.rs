//! BUG-1480 — после `el.style.display = 'inline'` на `div` `'display' in
//! getComputedStyle(el)` был `false` до следующего кадра: инкрементальный путь
//! не публиковал запись стиля элемента, ставшего бокс-less (пустой inline).

#![cfg(feature = "v8")]

use lumen_driver::{BrowserSession, InProcessSession};

const PAGE: &str = r#"<html><body>
<div id="a"></div><div id="b"></div><div id="c"></div>
</body></html>"#;

fn ev(s: &mut InProcessSession, js: &str) -> String {
    s.eval(js).expect("eval").trim_matches('"').to_owned()
}

#[test]
fn display_inline_is_visible_in_computed_style_immediately() {
    let mut s = InProcessSession::new();
    s.navigate_html(PAGE).expect("navigate_html");
    let js = "['a','b','c'].map(id => { const e = document.getElementById(id); e.style.display = 'inline'; \
              const cs = getComputedStyle(e); return ('display' in cs) + ':' + cs.display; }).join(',')";
    assert_eq!(ev(&mut s, js), "true:inline,true:inline,true:inline");
}

#[test]
fn display_switch_block_inline_keeps_property() {
    let mut s = InProcessSession::new();
    s.navigate_html(PAGE).expect("navigate_html");
    let js = "const e = document.getElementById('b'); const r = []; \
              for (const d of ['block','inline','inline','inline-block','inline']) { e.style.display = d; \
              const cs = getComputedStyle(e); r.push(('display' in cs) + ':' + cs.display); } r.join(',')";
    assert_eq!(
        ev(&mut s, js),
        "true:block,true:inline,true:inline,true:inline-block,true:inline"
    );
}
