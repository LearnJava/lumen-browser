//! Принадлежность членов `HTMLCanvasElement` интерфейсу и контракт аргумента
//! `getContext` (BUG-450).
//!
//! Отдельный файл, а не хвост `canvas_object_model.rs` (BUG-449, объектная
//! модель самих 2D-интерфейсов): здесь предмет — ЭЛЕМЕНТ, а не контекст.

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

/// Шесть членов `HTMLCanvasElement` стояли на КАЖДОМ элементе DOM: фабрика
/// обёрток ставила их в общую таблицу `_LUMEN_WRAPPER_MEMBERS`, а она лежит на
/// прототипе, через который проходит любая обёртка.
#[test]
fn canvas_members_are_absent_from_other_elements() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var d = document.createElement('div');\
         var svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');",
    )
    .unwrap();
    for member in [
        "getContext",
        "toDataURL",
        "toBlob",
        "transferControlToOffscreen",
        "width",
        "height",
    ] {
        assert_eq!(
            s(&rt, &format!("'{member}' in d")),
            "false",
            "<div> must not carry HTMLCanvasElement.{member}"
        );
        assert_eq!(
            s(&rt, &format!("'{member}' in svg")),
            "false",
            "<svg> must not carry HTMLCanvasElement.{member}"
        );
    }
}

/// Заявка называла ровно это: `div.toDataURL()` отдавал валидный data-URL, а
/// `div.width = 42` писал на `<div>` атрибут `width`, которого у него в HTML LS
/// нет. Атрибут — половина дефекта: присваивание обязано остаться обычным
/// expando, как в любом браузере.
#[test]
fn assigning_width_on_a_div_writes_no_attribute() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("var d = document.createElement('div'); d.width = 42;")
        .unwrap();
    assert_eq!(s(&rt, "d.width"), "42", "expando still holds the value");
    assert_eq!(
        s(&rt, "String(d.getAttribute('width'))"),
        "null",
        "no content attribute is created"
    );
    assert_eq!(s(&rt, "typeof d.toDataURL"), "undefined");
}

/// Члены живут на прототипе интерфейса, а не собственными свойствами —
/// это то, что делает возможным патч `HTMLCanvasElement.prototype` со страницы
/// (приём полифилов и самих тестов WPT).
#[test]
fn canvas_members_live_on_the_interface_prototype() {
    let rt = v8_runtime_with_dom(make_doc());
    for member in [
        "getContext",
        "toDataURL",
        "toBlob",
        "transferControlToOffscreen",
        "width",
        "height",
    ] {
        assert_eq!(
            s(&rt, &format!("'{member}' in HTMLCanvasElement.prototype")),
            "true",
            "HTMLCanvasElement.prototype must own {member}"
        );
    }
}

/// Бренд-проверка: метод, снятый с прототипа и позванный на чужом `this`,
/// обязан бросать, а не работать по подсмотренному nid. Без неё перенос на
/// прототип закрыл бы только `in`-детект, оставив саму дыру.
#[test]
fn canvas_methods_reject_a_foreign_receiver() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("var d = document.createElement('div');").unwrap();
    for expr in [
        "HTMLCanvasElement.prototype.toDataURL.call(d)",
        "HTMLCanvasElement.prototype.toBlob.call(d, function(){})",
        "HTMLCanvasElement.prototype.getContext.call(d, '2d')",
        "HTMLCanvasElement.prototype.transferControlToOffscreen.call(d)",
        "HTMLCanvasElement.prototype.getContext.call(null, '2d')",
        "Object.getOwnPropertyDescriptor(HTMLCanvasElement.prototype, 'width').get.call(d)",
        "Object.getOwnPropertyDescriptor(HTMLCanvasElement.prototype, 'width').set.call(d, 5)",
    ] {
        assert_eq!(s(&rt, expr), "THROW:TypeError", "{expr}");
    }
}

/// HTML LS §4.12.5 сравнивает `contextId` ПО ТОЧНОМУ значению. Шим приводил
/// аргумент к нижнему регистру, поэтому `'2D'` выдавал рабочий 2D-контекст, а
/// `'WebGL'` — контекст WebGL.
#[test]
fn get_context_matches_the_context_id_exactly() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("var c = document.createElement('canvas');").unwrap();
    for id in ["2D", "WebGL", "WebGL2", "BitmapRenderer", "WebGPU", "bogus"] {
        assert_eq!(
            s(&rt, &format!("String(c.getContext('{id}'))")),
            "null",
            "'{id}' is not a context id"
        );
    }
    assert_eq!(
        s(&rt, "c.getContext('2d') instanceof CanvasRenderingContext2D"),
        "true",
        "the exact spelling still works"
    );
}

/// `contextId` объявлен `required DOMString`: отсутствующий аргумент — это
/// `TypeError`, а не `null`. Конверсия тоже WebIDL-евская: `|| ''` превращал
/// `getContext(0)` в `getContext('')` вместо строки `'0'`.
#[test]
fn get_context_requires_its_argument_and_converts_it_as_a_string() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("var c = document.createElement('canvas');").unwrap();
    assert_eq!(s(&rt, "c.getContext()"), "THROW:TypeError");
    // Ни одна из конверсий не даёт '2d'/'webgpu'/'bitmaprenderer', поэтому
    // наблюдаемый ответ один и тот же — ассерт держит контракт «аргумент есть»,
    // отличая его от «аргумента нет».
    for arg in ["0", "null", "undefined", "false", "''"] {
        assert_eq!(
            s(&rt, &format!("String(c.getContext({arg}))")),
            "null",
            "getContext({arg}) converts and answers null"
        );
    }
}

/// `width`/`height` отражают атрибуты как `unsigned long` с дефолтами 300×150.
/// По HTML LS §2.6.2 значение вне `[0, 2147483647]` — а именно им становится
/// отрицательный или нечисловой аргумент — пишет в атрибут ДЕФОЛТ; старый
/// `parseInt` писал `width="0"`, а геттер при этом отвечал 300, то есть атрибут
/// и IDL-атрибут расходились.
#[test]
fn canvas_dimensions_reflect_as_unsigned_long() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("var c = document.createElement('canvas');").unwrap();
    assert_eq!(s(&rt, "c.width + 'x' + c.height"), "300x150", "defaults");
    assert_eq!(
        s(&rt, "(c.width = 64, c.getAttribute('width') + '/' + c.width)"),
        "64/64"
    );
    assert_eq!(
        s(&rt, "(c.width = -1, c.getAttribute('width'))"),
        "300",
        "out of range writes the interface default, not 0"
    );
    assert_eq!(
        s(&rt, "(c.height = 4000000000, c.getAttribute('height'))"),
        "150",
        "above 2147483647 is out of range too"
    );
    assert_eq!(
        s(&rt, "(c.removeAttribute('width'), c.width)"),
        "300",
        "an absent attribute is the default"
    );
}

/// Другая половина переноса: интерфейсы, у которых пара `width`/`height` по
/// спеке ЕСТЬ, обязаны её сохранить — раньше их обслуживал тот же общий
/// аксессор, поэтому удаление без замены сломало бы `img.width`.
#[test]
fn interfaces_that_own_width_keep_it_with_their_own_type() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var mk = function(t) { return document.createElement(t); };\
         var im = mk('img'), vd = mk('video'), inp = mk('input'), so = mk('source');\
         var emb = mk('embed'), td = mk('td'), obj = mk('object');",
    )
    .unwrap();
    // unsigned long: значение читается числом, дефолт 0.
    for el in ["im", "vd", "inp", "so"] {
        assert_eq!(s(&rt, &format!("{el}.width")), "0", "{el} default");
        assert_eq!(
            s(&rt, &format!("({el}.width = 7, typeof {el}.width + ':' + {el}.width)")),
            "number:7",
            "{el} reflects as unsigned long"
        );
    }
    // DOMString: `<td width="5">` читается строкой '5', а не числом 5.
    for el in ["emb", "td", "obj"] {
        assert_eq!(
            s(&rt, &format!("({el}.width = 5, typeof {el}.width + ':' + {el}.width)")),
            "string:5",
            "{el} reflects as DOMString"
        );
    }
}
