//! HTML Rendering §15.3.13: рамка и фон `<fieldset>` с rendered legend начинаются с границы
//! блока-начала, опущенной под legend; позади legend рамка не рисуется (FIELDSET-LEGEND).

use super::text_and_images::build;
use super::*;

const CSS: &str = "body{margin:0}fieldset{margin:0;padding:10px;border:solid 10px blue;width:200px;background:#fed}\
    legend{width:50px;height:30px;padding:0}";

/// `DrawBorder` fieldset'а вместе с обрамляющими `PushClipRect`: `(clip, border rect)`.
fn clipped_borders(dl: &DisplayList) -> Vec<(Rect, Rect)> {
    let mut out = Vec::new();
    let mut clip = None;
    for c in dl.iter() {
        match c {
            DisplayCommand::PushClipRect { rect } => clip = Some(*rect),
            DisplayCommand::PopClip => clip = None,
            DisplayCommand::DrawBorder { rect, .. } => {
                if let Some(cl) = clip {
                    out.push((cl, *rect));
                }
            }
            _ => {}
        }
    }
    out
}

/// Legend выше рамки: фон и рамка опущены на полразницы (30 − 10) / 2, рамка режется на три
/// куска — слева от legend, справа и под ним.
#[test]
fn tall_legend_shifts_border_and_background_and_cuts_a_gap() {
    let dl = build(
        r#"<fieldset><legend>L</legend><div style="height:40px"></div></fieldset>"#,
        CSS,
    );
    // Высота fieldset: 30 (legend) + 10 + 40 + 10 + 10 = 100; опущена рамка на 10.
    let fill = dl
        .iter()
        .find_map(|c| match c {
            DisplayCommand::FillRect { rect, .. } if rect.width > 100.0 => Some(*rect),
            _ => None,
        })
        .expect("фон fieldset");
    assert_eq!((fill.y, fill.height), (10.0, 90.0), "{fill:?}");
    let b = clipped_borders(&dl);
    assert_eq!(b.len(), 3, "{b:?}");
    assert!(b.iter().all(|(_, r)| (r.y, r.height) == (10.0, 90.0)), "{b:?}");
    // legend занимает x 20..70 → куски: [0..20], [70..], [20..70] ниже вырезa (под legend).
    let (left, right, under) = (b[0].0, b[1].0, b[2].0);
    assert_eq!((left.x, left.width), (0.0, 20.0), "{left:?}");
    assert_eq!((right.x, right.right()), (70.0, 240.0), "{right:?}");
    assert_eq!((under.x, under.width), (20.0, 50.0), "{under:?}");
    assert_eq!(under.y, 30.0, "вырез идёт до нижней кромки legend: {under:?}");
}

/// Без legend рамка целая: один `DrawBorder`, без клипов.
#[test]
fn fieldset_without_legend_paints_whole_border() {
    let dl = build(r#"<fieldset><div style="height:40px"></div></fieldset>"#, CSS);
    assert!(clipped_borders(&dl).is_empty());
    let borders = dl.iter().filter(|c| matches!(c, DisplayCommand::DrawBorder { .. })).count();
    assert_eq!(borders, 1);
}
