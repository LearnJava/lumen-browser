use lumen_core::geom::Size;

use crate::box_tree::LayoutBox;

// ── FIELDSET-LEGEND: rendered legend на верхней границе fieldset (HTML Rendering §15.3.13) ──

fn lay(html: &str, css: &str) -> (LayoutBox, lumen_dom::Document) {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(&format!("body{{margin:0}}{css}"));
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    (root, doc)
}

fn rect_of(root: &LayoutBox, doc: &lumen_dom::Document, id: &str) -> lumen_core::geom::Rect {
    super::find_by_id_all(root, doc, id).unwrap_or_else(|| panic!("нет #{id}")).rect
}

const FS: &str = "fieldset{margin:0;padding:10px;border:solid 10px;width:200px}";

/// Legend выше `border-top`: стоит у верхней кромки, содержимое ниже него, рамка опущена.
#[test]
fn tall_legend_pushes_content_down() {
    let (root, doc) = lay(
        r#"<fieldset id="f"><legend id="l">L</legend><div id="c"></div></fieldset>"#,
        &format!("{FS}legend{{height:30px;width:50px;padding:0}}#c{{height:40px}}"),
    );
    let l = rect_of(&root, &doc, "l");
    assert_eq!((l.x, l.y, l.width, l.height), (20.0, 0.0, 50.0, 30.0), "{l:?}");
    let c = rect_of(&root, &doc, "c");
    assert_eq!((c.x, c.y), (20.0, 40.0), "{c:?}");
    let f = rect_of(&root, &doc, "f");
    assert_eq!(f.height, 30.0 + 10.0 + 40.0 + 10.0 + 10.0, "{f:?}");
    let fb = super::find_by_id_all(&root, &doc, "f").unwrap();
    let fl = fb.fieldset_legend.expect("legend отмечен");
    assert!(fl.placed);
    assert_eq!(fl.border_inset, 10.0);
}

/// Legend ниже `border-top`: центруется на рамке, содержимое от `border-top`, рамка не опущена.
#[test]
fn short_legend_centres_on_border() {
    let (root, doc) = lay(
        r#"<fieldset id="f"><legend id="l">L</legend><div id="c"></div></fieldset>"#,
        "fieldset{margin:0;padding:10px;border:solid 20px;width:200px}legend{height:10px;width:50px;padding:0}#c{height:40px}",
    );
    let l = rect_of(&root, &doc, "l");
    assert_eq!((l.x, l.y), (30.0, 5.0), "{l:?}");
    let c = rect_of(&root, &doc, "c");
    assert_eq!(c.y, 30.0, "{c:?}");
    let fl = super::find_by_id_all(&root, &doc, "f").unwrap().fieldset_legend.unwrap();
    assert_eq!(fl.border_inset, 0.0);
}

/// Legend не обязан идти первым в разметке: первый `<legend>` встаёт на границу, как бы он ни
/// стоял среди детей, остальное содержимое остаётся в потоке под ним.
#[test]
fn legend_after_content_is_still_rendered_first() {
    let (root, doc) = lay(
        r#"<fieldset id="f"><div id="c"></div><legend id="l">L</legend></fieldset>"#,
        &format!("{FS}legend{{height:30px;width:50px;padding:0}}#c{{height:40px}}"),
    );
    assert_eq!(rect_of(&root, &doc, "l").y, 0.0);
    assert_eq!(rect_of(&root, &doc, "c").y, 40.0);
}

/// Плавающий legend — не rendered legend: обычный float внутри содержимого.
#[test]
fn floated_legend_is_not_rendered_legend() {
    let (root, doc) = lay(
        r#"<fieldset id="f"><legend id="l">L</legend><div id="c"></div></fieldset>"#,
        &format!("{FS}legend{{float:left;height:30px;width:50px;padding:0}}#c{{height:40px}}"),
    );
    assert!(super::find_by_id_all(&root, &doc, "f").unwrap().fieldset_legend.is_none());
    assert_eq!(rect_of(&root, &doc, "l").y, 20.0);
}

/// `justify-self` legend ставит его у правого края content box; `text-align` на место не влияет.
#[test]
fn legend_justify_self_end_not_text_align() {
    let (root, doc) = lay(
        r#"<fieldset><legend id="l">L</legend></fieldset><fieldset><legend id="t">L</legend></fieldset>"#,
        &format!("{FS}legend{{height:10px;width:50px;padding:0}}#l{{justify-self:end}}#t{{text-align:right}}"),
    );
    // content box: x от 20 шириной 200 → правый край 220
    assert_eq!(rect_of(&root, &doc, "l").x, 220.0 - 50.0);
    assert_eq!(rect_of(&root, &doc, "t").x, 20.0, "text-align legend не двигает");
}

/// `legend[align]` — это `justify-self`; `text-align` предка legend не наследует.
#[test]
fn legend_align_attribute_and_ancestor_text_align() {
    let (root, doc) = lay(
        r#"<fieldset><legend id="r" align="right">L</legend></fieldset>
           <fieldset><legend id="c" align="CENTER">L</legend></fieldset>
           <fieldset><legend id="j" align="justify">L</legend></fieldset>
           <div style="text-align:center"><fieldset><legend id="d">L</legend></fieldset></div>"#,
        &format!("{FS}legend{{height:10px;width:50px;padding:0}}"),
    );
    assert_eq!(rect_of(&root, &doc, "r").x, 170.0);
    assert_eq!(rect_of(&root, &doc, "c").x, 20.0 + (200.0 - 50.0) / 2.0);
    assert_eq!(rect_of(&root, &doc, "j").x, 20.0, "align=justify недопустим");
    assert_eq!(rect_of(&root, &doc, "d").x, 20.0, "text-align предка не наследуется");
}

/// Auto-поля прижимают legend, как у блока: оба — центр, левое — к правому краю.
#[test]
fn legend_auto_margins_align() {
    let (root, doc) = lay(
        r#"<fieldset><legend id="b">L</legend></fieldset><fieldset><legend id="r">L</legend></fieldset>"#,
        &format!("{FS}legend{{height:10px;width:50px;padding:0}}#b{{margin:0 auto}}#r{{margin-left:auto}}"),
    );
    assert_eq!(rect_of(&root, &doc, "b").x, 20.0 + 75.0);
    assert_eq!(rect_of(&root, &doc, "r").x, 170.0);
}

/// Поля legend: центр = `free/2 + margin-start`, конец = `free + margin-start − margin-end`
/// (сверено с Edge: поля в `free` не входят).
#[test]
fn legend_margins_offset_like_edge() {
    let (root, doc) = lay(
        r#"<fieldset><legend id="c">L</legend></fieldset><fieldset><legend id="e">L</legend></fieldset>"#,
        &format!("{FS}legend{{height:10px;width:50px;padding:0}}#c{{margin:0 10px;justify-self:center}}#e{{margin:0 30px 0 10px;justify-self:end}}"),
    );
    assert_eq!(rect_of(&root, &doc, "c").x, 20.0 + 75.0 + 10.0);
    assert_eq!(rect_of(&root, &doc, "e").x, 20.0 + 150.0 + 10.0 - 30.0);
}

/// Legend с `display: inline` / табличной внутренней — блокифицируется в layout, а
/// `getComputedStyle` по-прежнему отдаёт автор-значение (стиль в кэше каскада не тронут).
#[test]
fn legend_display_is_blockified() {
    for d in ["inline", "inline-block", "table", "inline-table", "table-row", "table-row-group"] {
        let (root, doc) = lay(
            r#"<fieldset id="f"><legend id="l">L</legend><div id="c" style="height:20px"></div></fieldset>"#,
            &format!("{FS}legend{{display:{d};height:30px;width:50px;padding:0}}"),
        );
        let l = rect_of(&root, &doc, "l");
        assert_eq!((l.x, l.y, l.width, l.height), (20.0, 0.0, 50.0, 30.0), "display:{d} {l:?}");
        assert_eq!(rect_of(&root, &doc, "c").y, 40.0, "display:{d}");
    }
}

/// `border: none` у fieldset обнуляет UA-рамку (computed width = 0 при style none).
#[test]
fn border_none_clears_ua_border_width() {
    let (root, doc) = lay(
        r#"<fieldset id="f"><div id="c" style="height:10px"></div></fieldset>"#,
        "fieldset{border:none;padding:0;margin:0}",
    );
    let c = rect_of(&root, &doc, "c");
    assert_eq!((c.x, c.y), (0.0, 0.0), "{c:?}");
    assert_eq!(rect_of(&root, &doc, "f").height, 10.0);
}

/// То же для `border-top: none` — только эта сторона теряет UA-рамку.
#[test]
fn border_side_none_clears_only_that_side() {
    let (root, doc) = lay(
        r#"<fieldset id="f"><div id="c" style="height:10px"></div></fieldset>"#,
        "fieldset{border-top:none;padding:0;margin:0}",
    );
    let c = rect_of(&root, &doc, "c");
    assert_eq!((c.x, c.y), (2.0, 0.0), "{c:?}");
    assert_eq!(rect_of(&root, &doc, "f").height, 10.0 + 2.0);
}

/// Без автора действуют UA-значения: рамка 2px, поля 2px, отступы .35/.625/.75em.
#[test]
fn ua_defaults_for_fieldset() {
    let (root, doc) = lay(
        r#"<fieldset id="f"><div id="c" style="height:10px"></div></fieldset>"#,
        "",
    );
    let f = rect_of(&root, &doc, "f");
    assert_eq!(f.x, 2.0, "{f:?}");
    assert_eq!(f.width, 800.0 - 4.0, "{f:?}");
    let c = rect_of(&root, &doc, "c");
    let em = 16.0;
    assert_eq!((c.x, c.y), (2.0 + 2.0 + 0.75 * em, 2.0 + 0.35 * em), "{c:?}");
    assert_eq!(f.height, 2.0 + 0.35 * em + 10.0 + 0.625 * em + 2.0, "{f:?}");
}

/// Fieldset берёт `writing-mode` от предка: UA-значения (`padding-block`, `min-inline-size`)
/// раскладываются по физическим сторонам по нему.
const VFS: &str = "fieldset{margin:0;padding:10px;border:solid 10px;height:200px}legend{width:30px;height:50px;padding:0}#c{width:40px}";
const VHTML: &str = r#"<div id="w"><fieldset id="f"><legend id="l">L</legend><div id="c"></div></fieldset></div>"#;

/// `vertical-rl`: граница блока-начала — правая, legend ложится на неё, содержимое левее.
#[test]
fn vertical_rl_legend_on_right_border() {
    let (root, doc) = lay(VHTML, &format!("{VFS}#w{{writing-mode:vertical-rl}}"));
    let f = rect_of(&root, &doc, "f");
    assert_eq!(f.width, 100.0, "{f:?}");
    let l = rect_of(&root, &doc, "l");
    assert_eq!((l.x - f.x, l.y - f.y, l.width, l.height), (70.0, 20.0, 30.0, 50.0), "{l:?}");
    let c = rect_of(&root, &doc, "c");
    assert_eq!((c.x - f.x, c.width), (20.0, 40.0), "{c:?}");
    let fl = super::find_by_id_all(&root, &doc, "f").unwrap().fieldset_legend.unwrap();
    assert!(fl.placed);
    assert_eq!(fl.border_inset, 10.0);
}

/// `vertical-lr`: граница блока-начала — левая.
#[test]
fn vertical_lr_legend_on_left_border() {
    let (root, doc) = lay(VHTML, &format!("{VFS}#w{{writing-mode:vertical-lr}}"));
    let f = rect_of(&root, &doc, "f");
    let l = rect_of(&root, &doc, "l");
    assert_eq!((l.x - f.x, l.y - f.y), (0.0, 20.0), "{l:?}");
    let c = rect_of(&root, &doc, "c");
    assert_eq!(c.x - f.x, 40.0, "{c:?}");
    assert_eq!(f.width, 100.0);
}
