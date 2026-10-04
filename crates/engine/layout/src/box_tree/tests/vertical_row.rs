use lumen_core::geom::{Rect, Size};

// ── BUG-1263: ряд inline-block в вертикальном writing-mode ──
//
// Пустые `inline-block` с явными размерами и `line-height:0`: числа считаются руками.

fn rects(body: &str, container_css: &str, ids: &[&str]) -> Vec<Rect> {
    let html = format!(r#"<div id="k">{body}</div>"#);
    let css = format!(
        "body{{margin:0}}#k{{line-height:0;{container_css}}}#a,#b{{display:inline-block;width:15px;height:45px}}"
    );
    let doc = lumen_html_parser::parse(&html);
    let sheet = lumen_css_parser::parse(&css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    ids.iter()
        .map(|id| super::find_by_id_all(&root, &doc, id).unwrap_or_else(|| panic!("нет #{id}")).rect)
        .collect()
}

const PAIR: &str = r#"<span id="a"></span><span id="b"></span>"#;
const PAIR_BR: &str = r#"<span id="a"></span><br><span id="b"></span>"#;

#[test]
fn vertical_rl_inline_blocks_flow_down_one_column() {
    let r = rects(PAIR, "writing-mode:vertical-rl;height:90px;width:100px", &["a", "b"]);
    assert_eq!(r[0].y, 0.0, "{:?}", r[0]);
    assert_eq!(r[1].y, 45.0, "{:?}", r[1]);
    assert_eq!(r[0].x, r[1].x, "одна колонка: {:?} {:?}", r[0], r[1]);
}

#[test]
fn vertical_rl_br_starts_the_next_column_to_the_left() {
    let r = rects(PAIR_BR, "writing-mode:vertical-rl;height:90px;width:100px", &["a", "b"]);
    assert_eq!(r[0].y, 0.0, "{:?}", r[0]);
    assert_eq!(r[1].y, 0.0, "{:?}", r[1]);
    assert_eq!(r[0].x, 85.0, "первая колонка у правой кромки: {:?}", r[0]);
    assert_eq!(r[1].x, 70.0, "{:?}", r[1]);
}

#[test]
fn vertical_lr_br_starts_the_next_column_to_the_right() {
    let r = rects(PAIR_BR, "writing-mode:vertical-lr;height:90px;width:100px", &["a", "b"]);
    assert_eq!((r[0].x, r[0].y), (0.0, 0.0), "{:?}", r[0]);
    assert_eq!((r[1].x, r[1].y), (15.0, 0.0), "{:?}", r[1]);
}

#[test]
fn vertical_rl_overflowing_inline_block_wraps_into_a_new_column() {
    // 45 + 45 не помещаются в 60px: второй блок уходит в соседнюю колонку.
    let r = rects(PAIR, "writing-mode:vertical-rl;height:60px;width:100px", &["a", "b"]);
    assert_eq!(r[0].y, 0.0, "{:?}", r[0]);
    assert_eq!(r[1].y, 0.0, "{:?}", r[1]);
    assert_eq!(r[0].x - r[1].x, 15.0, "{:?} {:?}", r[0], r[1]);
}

#[test]
fn vertical_rl_flex_item_sized_by_content_keeps_its_columns_inside() {
    // css-flexbox-row.html: у flex-элемента ширины нет — его дают две колонки
    // по 15px, и колонки стоят внутри элемента, а не у правого края доступного места.
    let html = r#"<div id="k"><div id="i1"><span id="a"></span><br><span id="b"></span></div></div>"#;
    let css = "body{margin:0}#k{display:flex;writing-mode:vertical-rl;height:90px;line-height:0}\
               #a,#b{display:inline-block;width:15px;height:45px}";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let find = |id: &str| super::find_by_id_all(&root, &doc, id).unwrap_or_else(|| panic!("нет #{id}")).rect;
    let (item, a, b) = (find("i1"), find("a"), find("b"));
    assert_eq!(item.width, 30.0, "{item:?}");
    assert_eq!((a.x, a.y), (item.x + 15.0, item.y), "orange справа: {a:?} в {item:?}");
    assert_eq!((b.x, b.y), (item.x, item.y), "grey слева: {b:?} в {item:?}");
}

// ── BUG-1264: вертикальный текст переносится по inline-size блока ──

struct Em;
impl crate::TextMeasurer for Em {
    /// Каждый символ — 1em в ширину: «aaa» при 10px занимает 30px.
    fn char_width(&self, _: char, size: f32) -> f32 {
        size
    }
    fn line_gap_px(&self, _: f32) -> f32 {
        0.0
    }
}

fn vertical_text(css: &str) -> (crate::box_tree::LayoutBox, crate::box_tree::LayoutBox) {
    let html = r#"<div id="k">aaa bbb ccc</div>"#;
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(&format!("body{{margin:0}}#k{{font:10px/10px X;{css}}}"));
    let root = super::super::layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &Em);
    let k = super::find_by_id_all(&root, &doc, "k").expect("нет #k").clone();
    fn run(b: &crate::box_tree::LayoutBox) -> Option<&crate::box_tree::LayoutBox> {
        if matches!(b.kind, crate::box_tree::BoxKind::InlineRun { .. }) {
            return Some(b);
        }
        b.children.iter().find_map(run)
    }
    let r = run(&k).expect("нет InlineRun").clone();
    (k, r)
}

#[test]
fn vertical_run_wrapped_by_inline_size_spans_one_column_per_line() {
    // height:35px — два слова по 30px подряд не помещаются: три колонки по 10px.
    let (k, r) = vertical_text("writing-mode:vertical-rl;height:35px");
    let crate::box_tree::BoxKind::InlineRun { lines, .. } = &r.kind else { unreachable!() };
    assert_eq!(lines.len(), 3, "строк: {}", lines.len());
    assert_eq!(r.rect.width, 30.0, "{:?}", r.rect);
    assert_eq!(r.rect.height, 30.0, "самая длинная колонка, а не сумма: {:?}", r.rect);
    assert_eq!(k.rect.width, 30.0, "блок шириной в три колонки: {:?}", k.rect);
    assert_eq!(r.rect.x, k.rect.x, "прогон внутри блока: {:?} в {:?}", r.rect, k.rect);
}

#[test]
fn vertical_run_frags_are_not_dragged_along_with_the_box() {
    // `InlineFrag::x` — смещение внутри колонки, а не абсолютная координата:
    // сдвиг блока вправо не должен уносить текст по вертикали.
    let (_, r) = vertical_text("writing-mode:vertical-rl;height:35px;margin-left:100px");
    let crate::box_tree::BoxKind::InlineRun { lines, .. } = &r.kind else { unreachable!() };
    for f in lines.iter().flatten().filter(|f| !f.text.is_empty()) {
        assert!(f.x >= 0.0 && f.x + f.width <= 35.0 + 0.01, "frag вне колонки: x={} w={}", f.x, f.width);
    }
}
