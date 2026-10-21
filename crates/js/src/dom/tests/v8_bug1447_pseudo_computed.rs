//! BUG-1447 — `getComputedStyle(el, '::first-letter')` reports the box properties the
//! pseudo-element rule sets (`background-*`, `border-radius`, `box-shadow`, `margin`, `opacity`)
//! instead of the originating element's values. Mirrors WPT
//! `css-pseudo/first-letter-allowed-properties.html`, which writes the properties through the
//! CSSOM of an empty `#target::first-letter {}` rule.

use super::v8_bug935_s55_content_journal::{page, runtime, JOURNAL_SWITCH};
use super::*;

#[test]
fn first_letter_computed_style_reports_box_properties_set_through_cssom() {
    let _journal_on = JOURNAL_SWITCH.lock().unwrap_or_else(|e| e.into_inner());
    let rt = runtime(page());
    let script = "(function() {
        var st = document.createElement('style');
        st.textContent = '#target::first-letter {} #target { visibility: hidden; }';
        document.body.appendChild(st);
        var d = document.createElement('div'); d.id = 'target'; d.textContent = 'text';
        document.body.appendChild(d);
        var style = document.styleSheets[0].cssRules[0].style;
        var props = {
            backgroundColor: 'rgb(10, 20, 30)',
            backgroundImage: 'linear-gradient(rgb(0, 0, 0), rgb(255, 255, 255))',
            backgroundAttachment: 'fixed',
            backgroundPosition: '10px 20px',
            backgroundRepeat: 'no-repeat',
            borderRadius: '10px 20px',
            boxShadow: 'rgb(10, 20, 30) 10px 20px 30px 40px inset',
            margin: '10px 20px 30px 40px',
            padding: '10px 20px 30px 40px',
            opacity: '0.5'
        };
        var bad = [];
        for (var p in props) {
            style[p] = '';
            style[p] = props[p];
            var got = getComputedStyle(d, '::first-letter')[p];
            if (got !== props[p]) bad.push(p + '=' + got);
            style[p] = '';
        }
        return bad.join(' | ');
    })()";
    match rt.eval(script).unwrap_or_else(|e| panic!("{e:?}")) {
        lumen_core::JsValue::String(s) => assert_eq!(s, "", "wrong computed values"),
        other => panic!("expected a string, got {other:?}"),
    }
}
