//! BUG-935 срез 94 — `width`/`height`, которые оболочка дописывает декодированному `<img>`
//! (`lumen_layout::apply_intrinsic_size`), — вход каскада, но не JS-мутация: трекер о них не
//! знал, и флаш с базисом, взятым до записи, оставлял картинке прежний стиль (живая страница
//! lenta.ru: три счётчика-пикселя `<img width=1 height=1>` расходились с полным каскадом).
//! `V8JsRuntime::note_shell_attr_writes` объявляет такую запись как обычную запись атрибута.

use super::v8_bug935_s55_content_journal::{page, runtime};
use super::*;
use crate::v8_runtime::V8JsRuntime;
use lumen_core::geom::Size;

fn eval_string(rt: &V8JsRuntime, script: &str) -> String {
    match rt.eval(script).unwrap_or_else(|e| panic!("{e:?}\n{script}")) {
        lumen_core::JsValue::String(s) => s,
        other => panic!("expected a string, got {other:?}"),
    }
}

/// Страница с одной картинкой и одним прочитанным стилем (базис флаша), затем запись
/// оболочки; возвращает `getComputedStyle(img).width/height` после неё.
fn width_height_after_shell_write(announce: bool) -> String {
    let doc = page();
    let rt = runtime(Arc::clone(&doc));
    let nid: u32 = eval_string(
        &rt,
        "(function() {
            var img = document.createElement('img');
            img.setAttribute('src', 'pixel.gif');
            document.body.appendChild(img);
            getComputedStyle(img).width;
            return String(img.__nid__);
        })()",
    )
    .parse()
    .unwrap();
    let node = lumen_dom::NodeId::from_raw(nid);
    let written = {
        let mut d = doc.lock().unwrap();
        lumen_layout::apply_intrinsic_size(&mut d, node, 3, 2, Size::new(800.0, 600.0))
    };
    assert!(written, "the decoded size must land in the document as attributes");
    if announce {
        rt.note_shell_attr_writes(&[node], &["width", "height"]);
    }
    eval_string(
        &rt,
        "(function() {
            var img = document.body.querySelector('img');
            var cs = getComputedStyle(img);
            return cs.width + ' ' + cs.height;
        })()",
    )
}

#[test]
fn a_size_the_shell_wrote_reaches_the_next_flush() {
    assert_eq!(width_height_after_shell_write(true), "3px 2px");
}

/// The control: the same write left unannounced is the defect — the flush keeps the style it
/// computed before the attributes existed.
#[test]
fn an_unannounced_write_keeps_the_old_style() {
    assert_eq!(width_height_after_shell_write(false), "0px 0px");
}
