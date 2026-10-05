use lumen_core::geom::{Rect, Size};

// ── LAYOUT-VFLOAT: float / clear в вертикальном потоке (CSS 2.1 §9.5 + Writing Modes L3 §7.1) ──
//
// Каждый символ — 1em: «R» при 50px — квадрат 50×50, числа считаются руками.

struct Em;
impl crate::TextMeasurer for Em {
    fn char_width(&self, _: char, size: f32) -> f32 {
        size
    }
    fn line_gap_px(&self, _: f32) -> f32 {
        0.0
    }
}

/// `body` внутри `#w` (100×100, шрифт 50px/50px); `css` дописывается к стилям страницы.
fn rects(mode: &str, body: &str, css: &str, ids: &[&str]) -> Vec<Rect> {
    let html = format!(r#"<div id="w">{body}</div>"#);
    let css = format!(
        "body{{margin:0}}#w{{font:50px/50px X;writing-mode:{mode};width:100px;height:100px}}\
         .l{{float:left}}.r{{float:right}}{css}"
    );
    let doc = lumen_html_parser::parse(&html);
    let sheet = lumen_css_parser::parse(&css);
    let root = super::super::layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &Em);
    ids.iter()
        .map(|id| super::find_by_id_all(&root, &doc, id).unwrap_or_else(|| panic!("нет #{id}")).rect)
        .collect()
}

fn xy(r: &Rect) -> (f32, f32) {
    (r.x, r.y)
}

#[test]
fn vertical_lr_left_float_goes_to_the_top_and_text_follows_below_it() {
    let r = rects("vertical-lr", r#"<div id="f" class="l">L</div><span id="t">A</span>"#, "", &["f", "w"]);
    assert_eq!(xy(&r[0]), (0.0, 0.0), "float у верхней (line-left) кромки: {:?}", r[0]);
    assert_eq!((r[0].width, r[0].height), (50.0, 50.0), "{:?}", r[0]);
}

#[test]
fn vertical_lr_right_float_goes_to_the_bottom() {
    let r = rects("vertical-lr", r#"<div id="f" class="r">R</div>"#, "", &["f"]);
    assert_eq!(xy(&r[0]), (0.0, 50.0), "float у нижней (line-right) кромки: {:?}", r[0]);
}

#[test]
fn vertical_rl_floats_start_at_the_right_edge() {
    let r = rects(
        "vertical-rl",
        r#"<div id="a" class="l">L</div><div id="b" class="r">R</div>"#,
        "",
        &["a", "b"],
    );
    assert_eq!(xy(&r[0]), (50.0, 0.0), "первая колонка — у правого края: {:?}", r[0]);
    assert_eq!(xy(&r[1]), (50.0, 50.0), "right-float рядом, у нижней кромки: {:?}", r[1]);
}

#[test]
fn vertical_text_beside_a_float_keeps_to_the_free_inline_range() {
    // float снизу занимает [50,100) первой колонки; «A» (50px) встаёт сверху.
    let (_, run) = {
        let html = r#"<div id="w"><div class="r">R</div>A</div>"#;
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(
            "body{margin:0}#w{font:50px/50px X;writing-mode:vertical-lr;width:100px;height:100px}.r{float:right}",
        );
        let root = super::super::layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &Em);
        let w = super::find_by_id_all(&root, &doc, "w").expect("нет #w").clone();
        let run = w.children.iter().find(|c| matches!(c.kind, crate::box_tree::BoxKind::InlineRun { .. }))
            .expect("нет InlineRun").clone();
        (w, run)
    };
    let crate::box_tree::BoxKind::InlineRun { lines, .. } = &run.kind else { unreachable!() };
    let frag = lines.iter().flatten().find(|f| f.text == "A").expect("нет «A»");
    assert_eq!(frag.x, 0.0, "{frag:?}");
    assert_eq!(run.rect.x, 0.0, "{:?}", run.rect);
}

#[test]
fn vertical_float_after_text_joins_its_last_column() {
    // «A» занимает верх первой колонки; float, идущий за ним, встаёт в ту же колонку.
    let r = rects("vertical-lr", r#"A<div id="f" class="r">R</div>"#, "", &["f"]);
    assert_eq!(xy(&r[0]), (0.0, 50.0), "{:?}", r[0]);
}

#[test]
fn vertical_second_float_that_does_not_fit_drops_to_the_next_column() {
    // Два left-float по 60px не помещаются в 100px по inline-оси рядом.
    let r = rects(
        "vertical-lr",
        r#"<div id="a" class="l" style="height:60px;width:30px"></div><div id="b" class="l" style="height:60px;width:30px"></div>"#,
        "",
        &["a", "b"],
    );
    assert_eq!(xy(&r[0]), (0.0, 0.0), "{:?}", r[0]);
    assert_eq!(xy(&r[1]), (30.0, 0.0), "вторая колонка, снова у верхней кромки: {:?}", r[1]);
}

#[test]
fn vertical_clear_moves_the_block_past_the_float() {
    let r = rects(
        "vertical-lr",
        r#"<div id="a" class="l" style="height:50px;width:40px"></div><div id="c" style="clear:left;width:10px"></div>"#,
        "",
        &["c"],
    );
    assert_eq!(r[0].x, 40.0, "clear:left уводит блок за правую (по block-оси) кромку float: {:?}", r[0]);
}

#[test]
fn vertical_auto_width_box_encloses_its_floats() {
    let html = r#"<div id="o" style="float:left;writing-mode:vertical-lr;height:100px"><div id="a" class="l" style="height:50px;width:40px"></div></div>"#;
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("body{margin:0}.l{float:left}");
    let root = super::super::layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &Em);
    let o = super::find_by_id_all(&root, &doc, "o").expect("нет #o").rect;
    assert_eq!(o.width, 40.0, "ширина охватывает float: {o:?}");
}

#[test]
fn vertical_rl_auto_width_box_keeps_its_float_on_its_own_right_edge() {
    let html = r#"<div id="o" style="float:left;writing-mode:vertical-rl;height:100px"><div id="a" class="l" style="height:50px;width:40px"></div></div>"#;
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("body{margin:0}.l{float:left}");
    let root = super::super::layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &Em);
    let find = |id: &str| super::find_by_id_all(&root, &doc, id).unwrap_or_else(|| panic!("нет #{id}")).rect;
    let (o, a) = (find("o"), find("a"));
    assert_eq!(o.width, 40.0, "{o:?}");
    assert_eq!(a.x, o.x, "float внутри коробки, не у правого края доступного места: {a:?} в {o:?}");
}

#[test]
fn vertical_left_float_after_text_pushes_the_text_past_itself() {
    // «A» стоит наверху колонки; следующий за ним left-float тоже в этой колонке,
    // у верхней кромки, а текст съезжает под него.
    let html = r#"<div id="w">A<div id="f" class="l">L</div></div>"#;
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(
        "body{margin:0}#w{font:50px/50px X;writing-mode:vertical-lr;width:100px;height:100px}.l{float:left}",
    );
    let root = super::super::layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &Em);
    let w = super::find_by_id_all(&root, &doc, "w").expect("нет #w").clone();
    let f = super::find_by_id_all(&root, &doc, "f").expect("нет #f").rect;
    let run = w.children.iter().find(|c| matches!(c.kind, crate::box_tree::BoxKind::InlineRun { .. }))
        .expect("нет InlineRun");
    let crate::box_tree::BoxKind::InlineRun { lines, .. } = &run.kind else { unreachable!() };
    let frag = lines.iter().flatten().find(|f| f.text == "A").expect("нет «A»");
    assert_eq!(xy(&f), (0.0, 0.0), "{f:?}");
    assert_eq!(frag.x, 50.0, "{frag:?}");
}
