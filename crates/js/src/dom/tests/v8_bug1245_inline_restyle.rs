//! BUG-1245 — `getComputedStyle` of an inline element (`<u>`, `<span>`) after an incremental
//! flush: the style lives on the segments of the run of its ancestor, and a clean graft used to
//! copy the previous run, old segment styles included, over the fresh one.

use super::v8_bug935_s55_content_journal::{page, runtime};
use super::*;

fn colors(read_between: bool, prop: &str, value: &str) -> String {
    let rt = runtime(page());
    rt.update_stylesheet(Arc::new(lumen_css_parser::parse("body { margin: 0; } .c { width: 200px; } .box { height: 20px; }")));
    let between = if read_between { "getComputedStyle(els[0]).top; els[0].getBoundingClientRect();" } else { "" };
    let script = format!(
        "(function() {{
            var root = document.createElement('div');
            document.body.appendChild(root);
            for (var i = 0; i < 8; i++) {{
                var d = document.createElement('div');
                d.className = 'c';
                d.innerHTML = '<p>item ' + i + '</p><div class=\"box\"><u>q</u></div>';
                root.appendChild(d);
            }}
            var els = Array.prototype.slice.call(root.children);
            {between}
            var u = els[1].querySelector('u');
            u.style.{prop} = '{value}';
            {between}
            return getComputedStyle(u).{prop} + '|' + u.getBoundingClientRect().width;
        }})()"
    );
    match rt.eval(&script).unwrap_or_else(|e| panic!("{e:?}\n{script}")) {
        lumen_core::JsValue::String(s) => s,
        other => panic!("expected a string, got {other:?}"),
    }
}

#[test]
fn inline_element_restyled_after_a_flush_reads_back_the_new_style() {
    for (prop, value) in [("color", "rgb(255, 0, 0)"), ("fontSize", "30px")] {
        let after_flush = colors(true, prop, value);
        let one_layout = colors(false, prop, value);
        assert_eq!(after_flush, one_layout, "{prop}: the flush kept the style from before the change");
    }
}
