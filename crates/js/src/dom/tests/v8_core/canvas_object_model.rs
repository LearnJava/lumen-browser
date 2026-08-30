//! Объектная модель Canvas 2D (BUG-449): интерфейсы существуют как глобальные
//! классы, члены живут на прототипах, фабрики отдают экземпляры, а метод,
//! вызванный на чужом `this`, бросает `TypeError`, а не рисует по чужому nid.
//!
//! Отдельный файл, а не хвост `selectors_canvas_window.rs`: тот уже 1 874
//! строки при потолке 2 000 (`scripts/check_file_sizes.py`).

use super::*;

/// Вычисляет JS-выражение и возвращает его как строку.
fn s(rt: &crate::v8_runtime::V8JsRuntime, expr: &str) -> String {
    let prelude = "var c = document.createElement('canvas'); var ctx = c.getContext('2d');";
    match rt.eval(&format!("{prelude} String({expr})")) {
        Ok(lumen_core::JsValue::String(v)) => v,
        other => format!("{other:?}"),
    }
}

#[test]
fn canvas_2d_interfaces_are_globals() {
    // BUG-449: из 14 интерфейсов §4.12.5 в глобальной области было три.
    let rt = v8_runtime_with_dom(make_doc());
    for name in [
        "CanvasRenderingContext2D",
        "ImageData",
        "TextMetrics",
        "CanvasGradient",
        "CanvasPattern",
    ] {
        assert_eq!(
            s(&rt, &format!("typeof {name}")),
            "function",
            "{name} must be a global interface object"
        );
    }
}

#[test]
fn context_is_an_instance_with_members_on_the_prototype() {
    // Раньше прототипом контекста был буквально `Object.prototype`, а все 59
    // членов — собственными свойствами экземпляра.
    let rt = v8_runtime_with_dom(make_doc());
    assert_eq!(s(&rt, "ctx instanceof CanvasRenderingContext2D"), "true");
    assert_eq!(
        s(&rt, "Object.getPrototypeOf(ctx) === CanvasRenderingContext2D.prototype"),
        "true"
    );
    assert_eq!(
        s(&rt, "Object.prototype.hasOwnProperty.call(ctx, 'fillRect')"),
        "false"
    );
    assert_eq!(
        s(&rt, "typeof CanvasRenderingContext2D.prototype.fillRect"),
        "function"
    );
}

#[test]
fn interface_instances_report_their_class_string() {
    // `assert_class_string` в WPT читает Symbol.toStringTag — тег обязан быть
    // на КАЖДОМ классе, унаследованный назвал бы подкласс именем базы.
    let rt = v8_runtime_with_dom(make_doc());
    assert_eq!(
        s(&rt, "Object.prototype.toString.call(ctx)"),
        "[object CanvasRenderingContext2D]"
    );
    assert_eq!(
        s(&rt, "Object.prototype.toString.call(ctx.getImageData(0, 0, 1, 1))"),
        "[object ImageData]"
    );
    assert_eq!(
        s(&rt, "Object.prototype.toString.call(ctx.measureText('x'))"),
        "[object TextMetrics]"
    );
    assert_eq!(
        s(&rt, "Object.prototype.toString.call(ctx.createLinearGradient(0, 0, 1, 1))"),
        "[object CanvasGradient]"
    );
    assert_eq!(s(&rt, "Object.prototype.toString.call(new Path2D())"), "[object Path2D]");
}

#[test]
fn page_can_patch_the_context_prototype() {
    // Приём полифилов и самих тестов WPT: до правки патчить было нечего.
    let rt = v8_runtime_with_dom(make_doc());
    assert_eq!(
        s(
            &rt,
            "(CanvasRenderingContext2D.prototype.__probe = 7, ctx.__probe)"
        ),
        "7"
    );
}

#[test]
fn a_method_on_a_foreign_receiver_throws_type_error() {
    // `2d.imageData.create1.this`: без бренд-проверки метод рисовал бы по nid,
    // подсмотренному у чужого объекта.
    let rt = v8_runtime_with_dom(make_doc());
    for receiver in ["null", "undefined", "{}"] {
        assert_eq!(
            s(
                &rt,
                &format!(
                    "(function(){{ try {{ \
                       CanvasRenderingContext2D.prototype.createImageData.call({receiver}, 1, 1); \
                       return 'no throw'; }} catch (e) {{ return e.name; }} }})()"
                )
            ),
            "TypeError",
            "createImageData.call({receiver})"
        );
    }
}

#[test]
fn image_data_is_constructible_from_dimensions() {
    let rt = v8_runtime_with_dom(make_doc());
    assert_eq!(
        s(&rt, "(function(){ var d = new ImageData(100, 50); \
                 return d.width + ',' + d.height + ',' + d.data.length + ',' \
                   + (d instanceof ImageData) + ',' + d.colorSpace + ',' + d.pixelFormat; })()"),
        "100,50,20000,true,srgb,rgba-unorm8"
    );
}

#[test]
fn image_data_constructor_follows_the_spec_error_table() {
    // Строки взяты из `2d.imageData.object.ctor.basics`: тип ошибки решает,
    // какую из двух перегрузок выбрал разбор аргументов.
    let rt = v8_runtime_with_dom(make_doc());
    let cases: [(&str, &str); 12] = [
        ("ImageData(1, 1)", "TypeError"),              // без `new`
        ("new ImageData(10)", "TypeError"),            // один аргумент
        ("new ImageData(0, 10)", "IndexSizeError"),
        ("new ImageData(10, 0)", "IndexSizeError"),
        ("new ImageData('width', 'height')", "IndexSizeError"),
        ("new ImageData(1 << 31, 1 << 31)", "IndexSizeError"),
        ("new ImageData(new Uint8ClampedArray(0))", "TypeError"),
        ("new ImageData(new Uint8Array(100), 25)", "IndexSizeError"),
        ("new ImageData(new Uint8ClampedArray(27), 2)", "InvalidStateError"),
        ("new ImageData(new Uint8ClampedArray(28), 7, 0)", "IndexSizeError"),
        ("new ImageData(new Uint8ClampedArray(104), 14)", "IndexSizeError"),
        ("new ImageData(null, 4, 4)", "TypeError"),
    ];
    for (expr, want) in cases {
        assert_eq!(
            s(
                &rt,
                &format!("(function(){{ try {{ {expr}; return 'no throw'; }} catch (e) {{ return e.name; }} }})()")
            ),
            want,
            "{expr}"
        );
    }
}

#[test]
fn image_data_from_a_buffer_derives_its_height() {
    let rt = v8_runtime_with_dom(make_doc());
    assert_eq!(
        s(&rt, "new ImageData(new Uint8ClampedArray(28), 7).height"),
        "1"
    );
    // Буфер используется, а не копируется — запись через ImageData видна в нём.
    assert_eq!(
        s(&rt, "(function(){ var buf = new Uint8ClampedArray(16); \
                 var d = new ImageData(buf, 2); d.data[0] = 9; \
                 return buf[0] + ',' + d.width + ',' + d.height + ',' + (d.data === buf); })()"),
        "9,2,2,true"
    );
}

#[test]
fn image_data_dimensions_are_readonly() {
    // `2d.imageData.object.readonly`: присваивание молча ничего не делает.
    let rt = v8_runtime_with_dom(make_doc());
    assert_eq!(
        s(&rt, "(function(){ var d = ctx.getImageData(0, 0, 10, 10); var old = d.data; \
                 d.width = 123; d.height = 123; d.data = [1, 2, 3, 4]; \
                 return d.width + ',' + d.height + ',' + (d.data === old); })()"),
        "10,10,true"
    );
}

#[test]
fn put_image_data_refuses_a_look_alike_object() {
    // `2d.imageData.put.wrongtype`: литерал с полями width/height/data проходил
    // проверку и уезжал в натив.
    let rt = v8_runtime_with_dom(make_doc());
    for arg in [
        "{ width: 1, height: 1, data: [255, 0, 0, 255] }",
        "'cheese'",
        "42",
    ] {
        assert_eq!(
            s(
                &rt,
                &format!("(function(){{ try {{ ctx.putImageData({arg}, 0, 0); return 'no throw'; }} \
                          catch (e) {{ return e.name; }} }})()")
            ),
            "TypeError",
            "putImageData({arg})"
        );
    }
    // Настоящий ImageData по-прежнему принимается.
    assert_eq!(
        s(&rt, "ctx.putImageData(ctx.getImageData(0, 0, 1, 1), 0, 0)"),
        "undefined"
    );
}

#[test]
fn text_metrics_reports_all_twelve_attributes() {
    // Было три из двенадцати; ширина по-прежнему приходит из натива.
    let rt = v8_runtime_with_dom(make_doc());
    assert_eq!(
        s(&rt, "(function(){ var m = ctx.measureText('x'); var names = \
                 ['width','actualBoundingBoxLeft','actualBoundingBoxRight','actualBoundingBoxAscent',\
                  'actualBoundingBoxDescent','fontBoundingBoxAscent','fontBoundingBoxDescent',\
                  'emHeightAscent','emHeightDescent','hangingBaseline','alphabeticBaseline',\
                  'ideographicBaseline']; var bad = []; \
                 for (var i = 0; i < names.length; i++) { \
                   if (typeof m[names[i]] !== 'number') bad.push(names[i]); } \
                 return bad.join('|') + '/' + (m instanceof TextMetrics); })()"),
        "/true"
    );
    assert_eq!(s(&rt, "ctx.measureText('').width"), "0");
}

#[test]
fn gradient_and_pattern_are_instances() {
    let rt = v8_runtime_with_dom(make_doc());
    assert_eq!(
        s(&rt, "(function(){ var g = ctx.createLinearGradient(0, 0, 1, 1); \
                 return (g instanceof CanvasGradient) + ',' \
                   + Object.prototype.hasOwnProperty.call(g, 'addColorStop'); })()"),
        "true,false"
    );
    assert_eq!(
        s(&rt, "(function(){ var p = ctx.createPattern(c, 'repeat'); \
                 return (p instanceof CanvasPattern) + ',' + typeof p.setTransform; })()"),
        "true,function"
    );
}

#[test]
fn add_color_stop_rejects_an_offset_outside_the_unit_range() {
    // §4.12.5.1.4: вне [0, 1] — IndexSizeError, нечисло — TypeError.
    let rt = v8_runtime_with_dom(make_doc());
    assert_eq!(
        s(&rt, "(function(){ try { ctx.createLinearGradient(0, 0, 1, 1).addColorStop(2, 'red'); \
                 return 'no throw'; } catch (e) { return e.name; } })()"),
        "IndexSizeError"
    );
    assert_eq!(
        s(&rt, "(function(){ try { ctx.createLinearGradient(0, 0, 1, 1).addColorStop(NaN, 'red'); \
                 return 'no throw'; } catch (e) { return e.name; } })()"),
        "TypeError"
    );
    assert_eq!(
        s(&rt, "ctx.createLinearGradient(0, 0, 1, 1).addColorStop(0.5, 'red')"),
        "undefined"
    );
}

#[test]
fn set_transform_with_no_arguments_is_the_identity() {
    // Шесть `+undefined` уезжали в натив как шесть NaN и теряли матрицу:
    // после такого сброса заливка не попадала на холст вовсе.
    let rt = v8_runtime_with_dom(make_doc());
    let rt2 = v8_runtime_with_dom(make_doc());
    let _ = rt.eval(
        "var c = document.createElement('canvas');\
         c.setAttribute('width', '4'); c.setAttribute('height', '4');\
         var ctx = c.getContext('2d');\
         ctx.translate(2, 2); ctx.setTransform();\
         ctx.fillStyle = '#00ff00'; ctx.fillRect(0, 0, 4, 4);",
    );
    let updates = rt.flush_canvas_updates();
    assert_eq!(updates.len(), 1, "fillRect after setTransform() paints");
    assert_eq!(updates[0].3[1], 255, "the reset transform put the fill at the origin");
    // Одноаргументная форма принимает DOMMatrix2DInit-словарь.
    assert_eq!(
        s(&rt2, "ctx.setTransform({ a: 1, b: 0, c: 0, d: 1, e: 0, f: 0 })"),
        "undefined"
    );
}

#[test]
fn text_metrics_come_from_the_font_not_from_the_font_size() {
    // BUG-449: девять из двенадцати чисел были производными от размера шрифта.
    // Горизонтальные берутся из bbox глифов, вертикальные — из `hhea`, поэтому
    // у более длинной строки правый край дальше, а у пробела чернил нет вовсе.
    let rt = v8_runtime_with_dom(make_doc());
    assert_eq!(
        s(&rt, "(function(){ ctx.font = '40px sans-serif'; \
                 var a = ctx.measureText('l'), b = ctx.measureText('llll'); \
                 return (b.actualBoundingBoxRight > a.actualBoundingBoxRight) + ',' \
                   + (a.actualBoundingBoxAscent > 0) + ',' \
                   + (a.actualBoundingBoxAscent < a.fontBoundingBoxAscent + 1) + ',' \
                   + (ctx.measureText(' ').actualBoundingBoxRight === 0); })()"),
        "true,true,true,true"
    );
    // Метрики масштабируются с размером шрифта.
    assert_eq!(
        s(&rt, "(function(){ ctx.font = '10px sans-serif'; var small = ctx.measureText('x'); \
                 ctx.font = '20px sans-serif'; var big = ctx.measureText('x'); \
                 return (big.fontBoundingBoxAscent > small.fontBoundingBoxAscent * 1.5) + ',' \
                   + (Math.abs(small.emHeightAscent + small.emHeightDescent - 10) < 0.001); })()"),
        "true,true"
    );
}

#[test]
fn text_metrics_are_measured_from_the_alignment_point_and_baseline() {
    // §4.12.5.1.13: горизонталь отсчитывается от точки выравнивания, вертикаль —
    // от линии textBaseline, а не от начала пера и не от alphabetic.
    let rt = v8_runtime_with_dom(make_doc());
    assert_eq!(
        s(&rt, "(function(){ ctx.font = '40px sans-serif'; ctx.textAlign = 'start'; \
                 var l = ctx.measureText('mmm'); ctx.textAlign = 'right'; \
                 var r = ctx.measureText('mmm'); \
                 return (r.actualBoundingBoxLeft > l.actualBoundingBoxLeft) + ',' \
                   + (r.actualBoundingBoxRight < l.actualBoundingBoxRight); })()"),
        "true,true"
    );
    assert_eq!(
        s(&rt, "(function(){ ctx.font = '40px sans-serif'; ctx.textBaseline = 'alphabetic'; \
                 var a = ctx.measureText('x'); ctx.textBaseline = 'top'; \
                 var t = ctx.measureText('x'); \
                 return (t.actualBoundingBoxAscent < a.actualBoundingBoxAscent) + ',' \
                   + (a.alphabeticBaseline === 0) + ',' + (t.alphabeticBaseline < 0); })()"),
        "true,true,true"
    );
}

#[test]
fn context_reports_its_attributes_and_never_loses_the_bitmap() {
    let rt = v8_runtime_with_dom(make_doc());
    assert_eq!(s(&rt, "ctx.isContextLost()"), "false");
    assert_eq!(
        s(&rt, "(function(){ var a = ctx.getContextAttributes(); \
                 return a.alpha + ',' + a.colorSpace + ',' + a.desynchronized \
                   + ',' + a.willReadFrequently; })()"),
        "true,srgb,false,false"
    );
}

#[test]
fn reset_clears_the_bitmap_and_the_state() {
    // §4.12.5.1.2. Метода не было вовсе.
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var c = document.createElement('canvas');\
         c.setAttribute('width', '4'); c.setAttribute('height', '4');\
         var ctx = c.getContext('2d');\
         ctx.fillStyle = '#00ff00'; ctx.lineWidth = 7; ctx.fillRect(0, 0, 4, 4);\
         ctx.reset();",
    )
    .unwrap();
    let updates = rt.flush_canvas_updates();
    assert_eq!(updates.len(), 1, "the reset marks the canvas dirty");
    assert_eq!(
        updates[0].3.iter().copied().max(),
        Some(0),
        "reset() leaves the bitmap transparent black"
    );
    let state = rt
        .eval("ctx.fillStyle + ',' + ctx.lineWidth + ',' + ctx.textAlign")
        .unwrap();
    assert_eq!(state, lumen_core::JsValue::String("#000000,1,start".into()));
}

#[test]
fn text_shaping_state_accepts_only_its_own_keywords() {
    // Значение вне перечисления игнорируется (§4.12.5.1.12), а не бросает.
    let rt = v8_runtime_with_dom(make_doc());
    assert_eq!(
        s(&rt, "(function(){ ctx.fontKerning = 'none'; ctx.fontKerning = 'bogus'; \
                 ctx.imageSmoothingQuality = 'high'; ctx.textRendering = 'nope'; \
                 ctx.letterSpacing = '3px'; ctx.wordSpacing = 'wide'; \
                 return ctx.fontKerning + ',' + ctx.imageSmoothingQuality + ',' \
                   + ctx.textRendering + ',' + ctx.letterSpacing + ',' + ctx.wordSpacing; })()"),
        "none,high,auto,3px,0px"
    );
    assert_eq!(
        s(&rt, "(function(){ ctx.fontStretch = 'condensed'; ctx.fontVariantCaps = 'small-caps'; \
                 return ctx.fontStretch + ',' + ctx.fontVariantCaps; })()"),
        "condensed,small-caps"
    );
}

#[test]
fn round_rect_paints_a_rounded_rectangle() {
    // Единственный из отсутствовавших членов со своим подкаталогом тестов.
    // Радиус сжимает угол: пиксель (0,0) остаётся пустым, центр — залит.
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var c = document.createElement('canvas');\
         c.setAttribute('width', '16'); c.setAttribute('height', '16');\
         var ctx = c.getContext('2d');\
         ctx.fillStyle = '#00ff00';\
         ctx.beginPath(); ctx.roundRect(0, 0, 16, 16, 8); ctx.fill();",
    )
    .unwrap();
    let updates = rt.flush_canvas_updates();
    assert_eq!(updates.len(), 1);
    let px = &updates[0].3;
    let at = |x: usize, y: usize| px[(y * 16 + x) * 4 + 3];
    assert_eq!(at(0, 0), 0, "the corner is cut away by the radius");
    assert!(at(8, 8) > 200, "the middle is filled");
    assert!(at(8, 0) > 200, "the middle of the top edge is filled");
}

#[test]
fn round_rect_validates_its_radii() {
    let rt = v8_runtime_with_dom(make_doc());
    for (expr, want) in [
        ("ctx.roundRect(0, 0, 8, 8, -1)", "IndexSizeError"),
        ("ctx.roundRect(0, 0, 8, 8, [1, 2, 3, 4, 5])", "RangeError"),
        ("ctx.roundRect(0, 0, 8, 8, NaN)", "TypeError"),
    ] {
        assert_eq!(
            s(
                &rt,
                &format!("(function(){{ try {{ {expr}; return 'no throw'; }} catch (e) {{ return e.name; }} }})()")
            ),
            want,
            "{expr}"
        );
    }
    // Форма без радиуса и форма со списком из четырёх — обе принимаются.
    assert_eq!(s(&rt, "ctx.roundRect(0, 0, 8, 8)"), "undefined");
    assert_eq!(s(&rt, "ctx.roundRect(0, 0, 8, 8, [1, 2, 3, 4])"), "undefined");
}

// ── BUG-451: цветовые атрибуты — разбор, игнорирование, сериализация ───────

/// HTML LS §4.12.5.1.3: валидное значение хранится КАНОНИЧЕСКИ, а не тем
/// текстом, которым его записали. Раньше `fillStyle` был полем-строкой, и
/// геттер отдавал `'#0F0'` на `'#0F0'`.
#[test]
fn paint_style_getter_returns_canonical_serialization() {
    let rt = v8_runtime_with_dom(make_doc());
    for (input, want) in [
        ("#0F0", "#00ff00"),
        ("#fa0", "#ffaa00"),
        ("lime", "#00ff00"),
        ("hsl(120,100%,50%)", "#00ff00"),
        ("rgb(0 255 0)", "#00ff00"),
        ("rgba(255,255,255,0.5)", "rgba(255, 255, 255, 0.5)"),
        ("transparent", "rgba(0, 0, 0, 0)"),
    ] {
        assert_eq!(
            s(&rt, &format!("(ctx.fillStyle = '{input}', ctx.fillStyle)")),
            want,
            "fillStyle = '{input}'"
        );
        assert_eq!(
            s(&rt, &format!("(ctx.strokeStyle = '{input}', ctx.strokeStyle)")),
            want,
            "strokeStyle = '{input}'"
        );
    }
}

/// Невалидное значение ИГНОРИРУЕТСЯ: атрибут сохраняет прежнее. Раньше в него
/// оседал мусор, а рисование продолжалось предыдущим цветом — то есть чтение и
/// рисование расходились.
#[test]
fn paint_style_ignores_invalid_values() {
    let rt = v8_runtime_with_dom(make_doc());
    for bad in ["not-a-color", "", "rgb(", "#gg", "currentColor"] {
        assert_eq!(
            s(
                &rt,
                &format!("(ctx.fillStyle = '#0f0', ctx.fillStyle = '{bad}', ctx.fillStyle)")
            ),
            "#00ff00",
            "fillStyle = '{bad}' обязано быть проигнорировано"
        );
    }
    // Тот же контракт у shadowColor — он тоже <color>, а не произвольная строка.
    assert_eq!(
        s(
            &rt,
            "(ctx.shadowColor = 'lime', ctx.shadowColor = 'not-a-color', ctx.shadowColor)"
        ),
        "#00ff00"
    );
    assert_eq!(s(&rt, "(ctx.shadowColor = '#0F0', ctx.shadowColor)"), "#00ff00");
}

/// Отвергнутое значение не должно доходить и до растеризатора: пиксель обязан
/// остаться от последнего ПРИНЯТОГО цвета.
#[test]
fn invalid_paint_style_does_not_reach_the_rasterizer() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var c = document.createElement('canvas');\
         c.setAttribute('width', '2'); c.setAttribute('height', '2');\
         var ctx = c.getContext('2d');\
         ctx.fillStyle = 'hsl(120, 100%, 50%)';\
         ctx.fillStyle = 'not-a-color';\
         ctx.fillRect(0, 0, 2, 2);",
    )
    .unwrap();
    let updates = rt.flush_canvas_updates();
    assert_eq!(updates.len(), 1);
    // hsl() раньше не разбиралась вовсе — залит был бы прозрачный/предыдущий.
    assert_eq!(&updates[0].3[0..4], &[0, 255, 0, 255]);
}
