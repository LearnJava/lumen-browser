//! BUG-1553: `styleSheet.media` — живой `MediaList` (`appendMedium`/`deleteMedium`/
//! `mediaText =`), записывающий результат в атрибут `media` владельца.

use super::*;
use crate::v8_runtime::V8JsRuntime;
use lumen_css_parser::StylesheetNodeEntry;

fn eval_string(rt: &V8JsRuntime, js: &str) -> String {
    match rt.eval(js).unwrap() {
        lumen_core::JsValue::String(s) => s,
        other => panic!("ожидалась строка, получено {other:?}"),
    }
}

#[test]
fn sheet_media_list_is_live_and_writes_owner_attribute() {
    let doc_arc = make_doc();
    let style_nid = {
        let mut doc = doc_arc.lock().unwrap();
        let head = super::super::find_element_by_tag(&doc, "head").unwrap();
        let style = doc.create_element(QualName::html("style"));
        doc.append_child(head, style);
        style.index() as u32
    };
    let rt = V8JsRuntime::new().unwrap();
    rt.install_dom(doc_arc, "", None, None, None, None, None, None, None, None, None, false, None).unwrap();
    rt.update_stylesheet_nodes(vec![StylesheetNodeEntry {
        node: style_nid,
        sheet: Arc::new(lumen_css_parser::parse("p { color: red }")),
        disabled: false,
    }]);
    let out = eval_string(
        &rt,
        "(function() {
            var st = document.getElementsByTagName('style')[0];
            var m = st.sheet.media, o = [];
            o.push(m.length + ':' + m.mediaText);
            m.appendMedium('screen'); m.appendMedium('screen'); m.appendMedium('screen, print');
            o.push(m.length + ':' + m.mediaText + ':' + m[0] + ':' + st.getAttribute('media'));
            m.mediaText = 'all, tty'; m.deleteMedium('tty');
            o.push(m.length + ':' + m.mediaText);
            try { m.deleteMedium('tty'); } catch (e) { o.push(e.name); }
            try { m.deleteMedium(); } catch (e) { o.push(e.name); }
            m.mediaText = 'screen and (min-width:480px), print, screen';
            m.deleteMedium('screen and (min-width: 480px)');
            o.push(m.mediaText);
            o.push(Object.prototype.toString.call(m) + ':' + (st.sheet.media === m));
            return o.join('|');
        })()",
    );
    assert_eq!(
        out,
        "0:|1:screen:screen:screen|1:all|NotFoundError|TypeError|print, screen|[object MediaList]:true"
    );
}
