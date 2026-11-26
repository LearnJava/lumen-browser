//! Значение поля, записанное в документ самой оболочкой (`Lumen::write_field_value`: набранный
//! или стёртый символ, вставка, undo), — не JS-мутация, а JS-эхо `_lumen_set_field_value`
//! застает значение уже на месте и ничего не записывает в трекер. Вне-потоковое задание
//! раскладки на инкрементальной дороге спрашивает трекер, какие блоки перестраивать, — и без
//! уведомления поле оставалось со старым текстом (после первой буквы набранное не рисовалось,
//! Backspace двигал каретку, но не стирал). `V8JsRuntime::shell_touch_notifier` объявляет такую
//! запись структурным касанием.

use super::v8_bug935_s55_content_journal::{page, runtime};
use super::*;
use crate::v8_runtime::V8JsRuntime;
use lumen_layout::style::OwnedNodeChange;

fn control_nid(rt: &V8JsRuntime) -> u32 {
    let script = "(function() {
            var input = document.createElement('input');
            document.body.appendChild(input);
            getComputedStyle(input).color;
            return String(input.__nid__);
        })()";
    match rt.eval(script).unwrap_or_else(|e| panic!("{e:?}\n{script}")) {
        lumen_core::JsValue::String(s) => s.parse().unwrap(),
        other => panic!("expected the control's node id, got {other:?}"),
    }
}

/// Страница с одним текстовым полем; читатель с подтверждением берёт базис, затем оболочка
/// записывает полю значение и посылает JS-эхо. Возвращает узел и то, что читатель сообщает
/// после базиса.
fn changes_after_shell_write(
    announce: bool,
) -> (lumen_dom::NodeId, Vec<(lumen_dom::NodeId, OwnedNodeChange)>) {
    let doc = page();
    let rt = runtime(Arc::clone(&doc));
    let node = lumen_dom::NodeId::from_raw(control_nid(&rt));
    let (read, ack) = rt.dom_changes_reader_acked();
    ack(read().epoch);
    assert!(!read().unattributed, "у читателя есть базис");
    {
        let mut d = doc.lock().unwrap();
        d.set_control_value(node, "typed");
    }
    if announce {
        rt.shell_touch_notifier()(node);
    }
    rt.eval(&format!("_lumen_set_field_value({}, 'typed')", node.raw())).unwrap();
    let t = read();
    assert!(!t.unattributed);
    (node, t.changes)
}

#[test]
fn a_value_the_shell_wrote_reaches_the_next_relayout() {
    let (node, changes) = changes_after_shell_write(true);
    assert_eq!(changes, vec![(node, OwnedNodeChange::Unattributed)]);
}

/// Контроль: необъявленная запись — и есть дефект. JS-эхо видит значение уже на месте и
/// ничего не записывает, так что трекер об этом узле не знает.
#[test]
fn an_unannounced_value_write_stays_invisible() {
    let (_, changes) = changes_after_shell_write(false);
    assert!(changes.is_empty(), "{changes:?}");
}
