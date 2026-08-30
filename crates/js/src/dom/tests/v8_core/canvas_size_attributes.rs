//! Атрибуты размера `<canvas>` как отражаемый `unsigned long` (BUG-452), и
//! общий разбор целого, через который они читаются.
//!
//! Отдельный файл, а не хвост `canvas_interface_membership.rs` (BUG-450, кому
//! принадлежат члены): здесь предмет — ЗНАЧЕНИЕ пары `width`/`height`, а не её
//! место в цепочке прототипов.
//!
//! Таблица ниже — дословно ожидания вендоренных WPT
//! `2d.canvas.host.size.attributes.*`; Rust-зеркало того же правила проверяется
//! в `lumen_dom::attr_int`, и расхождение этих двух наборов и было половиной
//! бага (бокс в layout и `canvas.width` из скрипта отвечали по-разному).

use super::*;

/// Вычисляет выражение и возвращает его как строку; исключение — тоже строка,
/// иначе `TypeError` и `null` неразличимы в одном ассерте.
fn s(rt: &crate::v8_runtime::V8JsRuntime, expr: &str) -> String {
    let wrapped = format!(
        "String((function(){{ try {{ return {expr}; }} \
         catch (e) {{ return 'THROW:' + e.name; }} }})())"
    );
    match rt.eval(&wrapped) {
        Ok(lumen_core::JsValue::String(v)) => v,
        other => format!("{other:?}"),
    }
}

/// Заявка называла ровно `width="0"`: `parseInt('0', 10) || 300` отдавал 300.
/// Ноль — валидный размер, у такого canvas просто нет пикселей.
#[test]
fn zero_is_a_valid_size_not_the_default() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var c = document.createElement('canvas');\
         c.setAttribute('width', '0'); c.setAttribute('height', '0');",
    )
    .unwrap();
    assert_eq!(s(&rt, "c.width + 'x' + c.height"), "0x0");
    // Контекст обязан выдаться и на нулевом холсте (HTML LS §4.12.5).
    assert_eq!(s(&rt, "typeof c.getContext('2d')"), "object");
}

/// Тот же `||` ронял и `'0x100'` (разбор целого обязан остановиться на `x` и
/// дать 0), а `'-100'` проходил его насквозь как truthy −100 и упирался в клампы
/// `< 1`, поэтому отвечал **1** вместо дефолта.
#[test]
fn wpt_parse_table() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("var c = document.createElement('canvas');").unwrap();
    for (attr, want) in [
        ("0", "0"),
        ("100.999", "100"),
        ("100em", "100"),
        ("", "300"),
        ("100e1", "100"),
        ("0x100", "0"),
        ("#!?", "300"),
        ("-100", "300"),
        ("0100", "100"),
        ("  ", "300"),
        ("100%", "100"),
        ("+100", "100"),
        ("  100", "100"),
        ("100#!?", "100"),
        ("\\r\\n\\t\\x0c100", "100"),
    ] {
        assert_eq!(
            s(&rt, &format!("(c.setAttribute('width','{attr}'), c.width)")),
            want,
            "width={attr:?}"
        );
    }
}

/// Верхняя граница §2.6.2 — отражение дефолтом, а не насыщение. Ни один WPT из
/// вендоренного набора её не трогает, но без неё разбор без верхней границы
/// возвращал `4294967291` дословно.
#[test]
fn out_of_range_attribute_reflects_the_default() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("var c = document.createElement('canvas');").unwrap();
    for (attr, want) in [("2147483647", "2147483647"), ("2147483648", "300"), ("4294967291", "300")] {
        assert_eq!(
            s(&rt, &format!("(c.setAttribute('width','{attr}'), c.width)")),
            want,
            "width={attr:?}"
        );
    }
}

/// Сеттер (он уже был спек-корректен) и геттер обязаны сойтись на нуле: до
/// правки атрибут честно получал `'0'`, а чтение отвечало 300 — то есть IDL- и
/// content-атрибут расходились в одном присваивании
/// (`2d.canvas.host.size.attributes.reflect.setidlzero`).
#[test]
fn idl_setter_and_getter_agree() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("var c = document.createElement('canvas');").unwrap();
    assert_eq!(s(&rt, "(c.width = 0, c.width)"), "0");
    assert_eq!(s(&rt, "c.getAttribute('width')"), "0");
    assert_eq!(s(&rt, "(c.width = 120, c.getAttribute('width'))"), "120");
    // WebIDL-приведение аргумента: `'+1.5e2'` → 150, `'0x96'` → 150 (hex через
    // ToNumber), дробь усекается (`2d.canvas.host.size.attributes.idl`).
    assert_eq!(s(&rt, "(c.width = 301.999, c.width)"), "301");
    assert_eq!(s(&rt, "(c.width = '+1.5e2', c.width)"), "150");
    assert_eq!(s(&rt, "(c.height = '0x96', c.height)"), "150");
    // Снятие атрибута возвращает дефолт (`…size.attributes.removed`).
    assert_eq!(s(&rt, "(c.removeAttribute('width'), c.width)"), "300");
}

/// Второй экземпляр того же дефекта, в соседнем файле шима: общий
/// `_lumen_parse_integer` обрывался на первом не-цифре, хотя §2.4.4.1 хвост
/// ИГНОРИРУЕТ. Через него читается вся таблица отражения (`<img>`/`<video>`/
/// `<input>`/`<source>`), поэтому `<img width="100px">` отвечал 0.
#[test]
fn shared_integer_parser_ignores_the_trailing_tail() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("var i = document.createElement('img');").unwrap();
    for (attr, want) in [
        ("0", "0"),
        ("100px", "100"),
        ("-5", "0"),
        ("+7", "7"),
        ("0x100", "0"),
        ("2147483648", "0"),
    ] {
        assert_eq!(
            s(&rt, &format!("(i.setAttribute('width','{attr}'), i.width)")),
            want,
            "img width={attr:?}"
        );
    }
    // `tabindex` идёт через тот же разбор, но через ЗНАКОВЫЙ его уровень.
    assert_eq!(s(&rt, "(i.setAttribute('tabindex','3zzz'), i.tabIndex)"), "3");
    assert_eq!(s(&rt, "(i.setAttribute('tabindex','-1'), i.tabIndex)"), "-1");
}
