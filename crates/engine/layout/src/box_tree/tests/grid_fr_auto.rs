use lumen_core::geom::Size;

// ── BUG-1846: fr-дорожка не должна обнулять auto-дорожки шаблона ──
//
// `classify_col_tracks` отбраковывал любой шаблон с fr (`sizing_of(Fr) → None`),
// и сетка падала в старый путь, где auto-дорожка получала ширину только при
// полном отсутствии fr — с fr всё обнулялось (живой случай: композер чата
// portal.nextera.ru, `auto auto minmax(0,1fr) auto auto auto auto`, кнопки
// схлопнулись в ноль; Chrome даёт auto их контентный вклад, fr — остаток).
// По §12.4 не-гибкие дорожки максимизируются до распределения fr (§12.6/§12.7).

fn col_widths(template: &str, total: f32, items: &[(&str, u32, &str)]) -> Vec<f32> {
    let html = format!(
        r#"<div id="g">{}</div>"#,
        items
            .iter()
            .map(|(id, col, style)| format!(r#"<div id="{id}" style="grid-column:{col};{style}"></div>"#))
            .collect::<String>()
    );
    let doc = lumen_html_parser::parse(&html);
    let sheet = lumen_css_parser::parse(&format!(
        "body{{margin:0}}#g{{display:grid;width:{total}px;height:100px;grid-template-columns:{template}}}"
    ));
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    items
        .iter()
        .map(|(id, _, _)| super::find_by_id_all(&root, &doc, id).expect("нет item").rect.width)
        .collect()
}

#[test]
fn fr_takes_the_remainder_after_the_auto_tracks_content() {
    // Композер портала (масштаб 788, gap 0): пустые auto — 0, auto с контентом —
    // контент, minmax(0,1fr) — весь остаток (Chrome на живой странице:
    // 44px 0px 616px 40px 40px 0px 48px).
    let w = col_widths(
        "auto auto minmax(0,1fr) auto auto auto auto",
        788.0,
        &[
            ("a", 1, ""),
            ("b", 2, ""),
            ("c", 3, ""),
            ("d", 4, "width:40px;height:20px"),
            ("e", 5, "width:40px;height:20px"),
            ("f", 6, ""),
            ("h", 7, "width:48px;height:20px"),
        ],
    );
    assert_eq!(w, vec![0.0, 0.0, 660.0, 40.0, 40.0, 0.0, 48.0], "{w:?}");
}

#[test]
fn minmax_zero_auto_track_keeps_its_content_between_fr_tracks() {
    let w = col_widths(
        "1fr minmax(0,auto) 1fr",
        600.0,
        &[("a", 1, ""), ("b", 2, "width:117px;height:20px"), ("c", 3, "")],
    );
    assert_eq!(w, vec![241.5, 117.0, 241.5], "{w:?}");
}

#[test]
fn lone_auto_track_between_fr_tracks_is_its_content() {
    let w = col_widths("1fr auto 1fr", 600.0, &[("a", 1, ""), ("b", 2, "width:117px;height:20px"), ("c", 3, "")]);
    assert_eq!(w, vec![241.5, 117.0, 241.5], "{w:?}");
}

#[test]
fn fr_share_below_base_freezes_the_track_at_its_base() {
    // §12.7: гипотетический fr (30) ниже базы первой дорожки (50) — она
    // замораживается на базе, вторая забирает остаток.
    let w = col_widths("minmax(50px, 1fr) 1fr", 60.0, &[("a", 1, ""), ("b", 2, "")]);
    assert_eq!(w, vec![50.0, 10.0], "{w:?}");
}
