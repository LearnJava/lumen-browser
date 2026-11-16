//! BUG-935 срез 101 — стенд записи наследуемого custom property на `body` (форма
//! `--scrollbar-compensation`): ~2 200 элементов, 300 флашей, каждый — `style.setProperty` на
//! `body` и чтение геометрии.
//!
//! `#[ignore]`: это не проверка, а источник суммы `[profile]`-областей и времени цикла —
//! `LUMEN_PROFILE_TREE=1 cargo test -p lumen-js --profile dev-release --features v8-backend
//! --lib bug935_s101_stand -- --ignored --nocapture --test-threads=1`.
//! A/B — две сборки (кандидат откачен, см. BUG-935 срез 101): плечи подряд, машина шумит
//! на десятки процентов, поэтому вывод — по строкам `[profile]` одного флаша, а не по сумме.

use super::v8_bug935_s55_content_journal::{page, runtime};
use super::*;

#[test]
#[ignore = "стенд профилирования, не проверка"]
fn bug935_s101_stand() {
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
            document.body.style.setProperty('--scrollbar-compensation', (k % 17) + 'px');
            document.body.getBoundingClientRect();
            root.getBoundingClientRect();
        }
        return String(Date.now() - t0);
    })()";
    match rt.eval(script).unwrap() {
        lumen_core::JsValue::String(s) => eprintln!("[stand] 300 custom-property flushes: {s} ms"),
        other => panic!("{other:?}"),
    }
}
