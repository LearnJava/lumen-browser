//! CSS 2.1 §17.4: фон и рамка таблицы принадлежат сетке, а не обёртке с подписью (BUG-1364) —
//! подпись `caption-side: top` лежит выше фона, `bottom` — ниже.

use super::text_and_images::build;
use super::*;

const CSS: &str = "body{margin:0}table{border-spacing:0;background:blue}td{width:100px;height:50px;padding:0}    caption{height:20px;padding:0}";

fn blue_fill(dl: &DisplayList) -> Rect {
    dl.iter()
        .find_map(|c| match c {
            DisplayCommand::FillRect { rect, color } if color.b == 255 && color.r == 0 => Some(*rect),
            _ => None,
        })
        .expect("фон таблицы")
}

#[test]
fn top_caption_stays_above_table_background() {
    let dl = build("<table><caption>c</caption><tr><td></td></tr></table>", CSS);
    let f = blue_fill(&dl);
    assert_eq!((f.y, f.height), (20.0, 50.0), "{f:?}");
}

#[test]
fn bottom_caption_stays_below_table_background() {
    let dl = build("<table><caption style=\"caption-side:bottom\">c</caption><tr><td></td></tr></table>", CSS);
    let f = blue_fill(&dl);
    assert_eq!((f.y, f.height), (0.0, 50.0), "{f:?}");
}
