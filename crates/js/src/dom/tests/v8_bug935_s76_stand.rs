//! BUG-935 срез 76 — стенд устойчивого флаша: ~2 200 элементов, цикл детектора
//! шрифтов (`span` с `style` в `body`, чтение геометрии, удаление, чтение).
//!
//! `#[ignore]`: это не проверка, а источник суммы `[profile]`-областей —
//! `LUMEN_PROFILE_TREE=1 cargo test -p lumen-js --profile dev-release --features v8-backend
//! --lib bug935_s76_stand -- --ignored --nocapture --test-threads=1`.

use super::v8_bug935_s55_content_journal::{page, runtime};
use super::*;

#[test]
#[ignore = "стенд профилирования, не проверка"]
fn bug935_s76_stand() {
    let rt = runtime(page());
    let script = "(function() {
        var root = document.createElement('main');
        document.body.appendChild(root);
        for (var i = 0; i < 200; i++) {
            var c = document.createElement('article');
            c.className = 'card';
            c.innerHTML = '<h3>t</h3><p><a href=\"#\">x</a> <b>y</b></p><ul><li>1</li><li>2</li><li>3</li></ul><div><span>a</span><em>b</em></div>';
            root.appendChild(c);
        }
        document.body.getBoundingClientRect();
        var t0 = Date.now();
        for (var k = 0; k < 300; k++) {
            var s = document.createElement('span');
            s.style.fontFamily = 'monospace';
            s.textContent = 'mmmmmmmmmmlli';
            document.body.appendChild(s);
            s.getBoundingClientRect();
            document.body.removeChild(s);
            document.body.getBoundingClientRect();
        }
        return String(Date.now() - t0);
    })()";
    match rt.eval(script).unwrap() {
        lumen_core::JsValue::String(s) => eprintln!("[stand] 300 flushes: {s} ms"),
        other => panic!("{other:?}"),
    }
}
