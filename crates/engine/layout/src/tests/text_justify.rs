//! `text-align: justify`, `justify-all`, `match-parent`, `text-align-last`, `text-justify`
//! (CSS Text L3 §7.1–7.3, TEXT-JUSTIFY). Шрифт — `Fixed8`: 8px на символ.

use super::*;
use crate::style::TextAlignLast;

fn lines_of(html: &str, css: &str, width: f32) -> Vec<Vec<InlineFrag>> {
    let root = lay_measured(html, css, width);
    let p = first_element_child(&root);
    let run = first_inline_run(p);
    let BoxKind::InlineRun { lines, .. } = &run.kind else { panic!("expected InlineRun") };
    lines.clone()
}

/// Строки всех `InlineRun` блока подряд: `<br>` разрезает поток на несколько прогонов.
fn runs_lines_of(html: &str, css: &str, width: f32) -> Vec<Vec<InlineFrag>> {
    let root = lay_measured(html, css, width);
    let p = first_element_child(&root);
    p.children
        .iter()
        .filter_map(|c| if let BoxKind::InlineRun { lines, .. } = &c.kind { Some(lines.clone()) } else { None })
        .flatten()
        .filter(|l| !l.is_empty())
        .collect()
}

fn right_edge(line: &[InlineFrag]) -> f32 {
    line.last().map_or(0.0, |f| f.x + f.width)
}

/// Строки, кроме последней, растянуты до края, пробелы получают равную добавку.
#[test]
fn justify_stretches_spaces_except_last_line() {
    let lines = lines_of("<p>aa bb cc dd ee ff</p>", "p { text-align: justify; }", 80.0);
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!((right_edge(&lines[0]) - 80.0).abs() < 0.01, "right edge {}", right_edge(&lines[0]));
    assert!((lines[0][0].style.word_spacing - 8.0).abs() < 0.01, "двa пробела делят 16px");
    assert_eq!(lines[1][0].x, 0.0);
    assert_eq!(lines[1][0].style.word_spacing, 0.0, "последняя строка — start");
}

/// `text-align-last: justify` растягивает и последнюю строку.
#[test]
fn text_align_last_justify_stretches_last_line() {
    let lines = lines_of("<p>aa bb</p>", "p { text-align-last: justify; }", 100.0);
    assert!((right_edge(&lines[0]) - 100.0).abs() < 0.01);
}

/// `justify-all` = `justify` + `text-align-last: justify`.
#[test]
fn justify_all_stretches_every_line() {
    let lines = lines_of("<p>aa bb cc dd ee ff</p>", "p { text-align: justify-all; }", 80.0);
    assert_eq!(lines.len(), 2);
    for l in &lines {
        assert!((right_edge(l) - 80.0).abs() < 0.01, "{l:?}");
    }
}

/// Строка перед `<br>` — «последняя» для `text-align-last`.
#[test]
fn line_before_br_uses_text_align_last() {
    let lines = runs_lines_of("<p>aa bb<br>cc dd</p>", "p { text-align: justify; text-align-last: justify; }", 100.0);
    assert_eq!(lines.len(), 2);
    assert!((right_edge(&lines[0]) - 100.0).abs() < 0.01);
    // При `auto` строка перед `<br>` остаётся по `start`.
    let lines = runs_lines_of("<p>aa bb<br>cc dd</p>", "p { text-align: justify; }", 100.0);
    assert!((right_edge(&lines[0]) - 40.0).abs() < 0.01, "{}", right_edge(&lines[0]));
}

/// `text-justify: none` отключает растяжение.
#[test]
fn text_justify_none_disables() {
    let lines = lines_of("<p>aa bb cc dd ee ff</p>", "p { text-align: justify; text-justify: none; }", 80.0);
    assert!((right_edge(&lines[0]) - 64.0).abs() < 0.01);
}

/// `inter-character` растягивает пары символов (`letter-spacing`), а не пробелы.
#[test]
fn text_justify_inter_character_spreads_letters() {
    let lines = lines_of("<p>ab cd ef gh</p>", "p { text-align: justify; text-justify: inter-character; }", 50.0);
    // «ab cd» = 40px, 4 промежутка делят 10px.
    let f = &lines[0][0];
    assert!((f.style.letter_spacing - 2.5).abs() < 0.01, "{}", f.style.letter_spacing);
    assert_eq!(f.style.word_spacing, 0.0);
    assert!((right_edge(&lines[0]) - 50.0).abs() < 0.01);
}

/// Строка без пробелов в `auto`: CJK растягивается между иероглифами.
#[test]
fn auto_justifies_ideographs_without_spaces() {
    let lines = lines_of("<p>東京</p>", "p { text-align-last: justify; }", 100.0);
    assert!((right_edge(&lines[0]) - 100.0).abs() < 0.01, "{}", right_edge(&lines[0]));
}

/// Без возможностей растяжения `justify` — это `start`, в RTL — прижато вправо.
#[test]
fn single_word_justify_falls_back_to_start() {
    let lines = lines_of("<p>abcd</p>", "p { text-align: justify-all; }", 100.0);
    assert_eq!(lines[0][0].x, 0.0);
    let lines = lines_of("<p>abcd</p>", "p { text-align: justify-all; direction: rtl; }", 100.0);
    assert!((right_edge(&lines[0]) - 100.0).abs() < 0.01);
}

/// RTL: растянутая строка занимает всю ширину, пробелы те же.
#[test]
fn justify_rtl_fills_line() {
    let lines = lines_of("<p>aa bb cc dd ee ff</p>", "p { text-align: justify; direction: rtl; }", 80.0);
    assert!((right_edge(&lines[0]) - 80.0).abs() < 0.01);
    assert!(lines[0][0].x.abs() < 0.01);
}

/// Стык двух inline-фрагментов с пробелом тоже растягивается.
#[test]
fn justify_widens_gap_between_inline_boxes() {
    let lines = lines_of(
        "<p>aa <b>bb</b> cc dd ee</p>",
        "p { text-align: justify; } b { font-weight: bold; }",
        80.0,
    );
    assert!((right_edge(&lines[0]) - 80.0).abs() < 0.01, "{:?}", lines[0].iter().map(|f| (f.x, f.width)).collect::<Vec<_>>());
}

/// `text-align: match-parent` и `text-align-last: match-parent` берут значение родителя
/// с пересчётом `start`/`end` по `direction` родителя.
#[test]
fn match_parent_resolves_against_parent_direction() {
    let root = lay_measured(
        "<div><p>ab</p></div>",
        "div { text-align: start; text-align-last: end; direction: rtl; } p { direction: ltr; text-align: match-parent; text-align-last: match-parent; }",
        100.0,
    );
    let div = first_element_child(&root);
    let p = first_element_child(div);
    assert_eq!(p.style.text_align, TextAlign::Right);
    assert_eq!(p.style.text_align_last, TextAlignLast::Left);
    assert!(p.style.text_extra.align_last_match_parent);
}

/// Перевод строки в `pre-wrap` обрывает строку так же, как `<br>`: она «последняя».
#[test]
fn pre_wrap_newline_line_uses_text_align_last() {
    let css = "p { white-space: pre-wrap; text-align: justify; text-align-last: justify; }";
    let lines = runs_lines_of("<p>aa bb\ncc dd</p>", css, 100.0);
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!((right_edge(&lines[0]) - 100.0).abs() < 0.01, "{}", right_edge(&lines[0]));
    let lines = runs_lines_of("<p>aa bb\ncc dd</p>", "p { white-space: pre-wrap; text-align: justify; }", 100.0);
    assert!((right_edge(&lines[0]) - 40.0).abs() < 0.01, "{}", right_edge(&lines[0]));
}

/// `pre-wrap`: висящий пробел в конце мягко перенесённой строки не растягивается и не входит в
/// ширину — до края доходит последнее слово.
#[test]
fn pre_wrap_hanging_space_is_not_stretched() {
    let lines = lines_of("<p>aa bb cc dd ee ff</p>", "p { text-align: justify; white-space: pre-wrap; }", 80.0);
    let first = &lines[0];
    assert_eq!(first.last().unwrap().text, " ", "{first:?}");
    let body = &first[first.len() - 2];
    assert!((right_edge(std::slice::from_ref(body)) - 80.0).abs() < 0.01, "{first:?}");
    assert!((body.style.word_spacing - 8.0).abs() < 0.01, "{first:?}");
}

/// Табуляция: растягивается только то, что после последней табуляции, иначе позиции табуляции
/// съезжают (WPT `text-align-justify-tabs-*`).
#[test]
fn justify_stretches_only_after_last_tab() {
    let lines = lines_of(
        "<p>a b\tc d e</p>",
        "p { text-align: justify; text-align-last: justify; white-space: pre-wrap; }",
        120.0,
    );
    let line = &lines[0];
    assert_eq!(line.len(), 3, "{line:?}");
    assert_eq!(line[0].style.word_spacing, 0.0, "до табуляции растяжения нет");
    assert_eq!(line[1].x, 24.0, "табуляция остаётся на своём месте");
    assert!((line[2].style.word_spacing - 8.0).abs() < 0.01, "{line:?}");
    assert!((right_edge(line) - 120.0).abs() < 0.01, "{line:?}");
}
