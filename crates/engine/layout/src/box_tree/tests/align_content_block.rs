//! `align-content` of block containers and table cells (CSS Box Alignment L3 §«Block Containers»,
//! BUG-1527): the in-flow content moves along the block axis as one piece. Expectations are
//! `data-offset-y` attributes checked by the `align_self_wpt` emulation of `check-layout-th.js`.

use super::align_self_wpt::check_html;

const HEAD: &str = "<!DOCTYPE html><style>
  html, body { margin: 0; padding: 0; font: 10px/1 Ahem; }
  .wrapper { position: relative; }
  .test { height: 50px; background: black; }
  .in-flow { margin-top: 10px; background: orange; height: 10px }
</style><div class=\"wrapper\">";

fn check(name: &str, body: &str) {
    let html = format!("{HEAD}{body}</div>");
    let bad = check_html(name, &html);
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

#[test]
fn start_center_end_split_the_free_space() {
    // Content: 10 px margin + 10 px box = 20 px of a 50 px box; the margin stays inside.
    check("start", r#"<div class="test" style="align-content: start"><div class="in-flow" data-offset-y="10"></div></div>"#);
    check("center", r#"<div class="test" style="align-content: center"><div class="in-flow" data-offset-y="25"></div></div>"#);
    check("end", r#"<div class="test" style="align-content: end"><div class="in-flow" data-offset-y="40"></div></div>"#);
    check("flex-end", r#"<div class="test" style="align-content: flex-end"><div class="in-flow" data-offset-y="40"></div></div>"#);
    check("last baseline", r#"<div class="test" style="align-content: last baseline"><div class="in-flow" data-offset-y="40"></div></div>"#);
}

#[test]
fn normal_leaves_the_content_and_its_margin_collapse_alone() {
    // Without an alignment the first child's margin escapes through the container (and through
    // the wrapper): the child sits at the top edge of both.
    check("normal", r#"<div class="test" style="align-content: normal"><div class="in-flow" data-offset-y="0"></div></div>"#);
}

#[test]
fn distributions_fall_back() {
    check("space-between", r#"<div class="test" style="align-content: space-between"><div class="in-flow" data-offset-y="10"></div></div>"#);
    check("stretch", r#"<div class="test" style="align-content: stretch"><div class="in-flow" data-offset-y="10"></div></div>"#);
    check("space-evenly", r#"<div class="test" style="align-content: space-evenly"><div class="in-flow" data-offset-y="25"></div></div>"#);
    check("space-around", r#"<div class="test" style="align-content: space-around"><div class="in-flow" data-offset-y="25"></div></div>"#);
}

#[test]
fn overflowing_content_stays_at_the_start_unless_unsafe() {
    let small = "height: 10px;";
    check("center", &format!(r#"<div class="test" style="{small} align-content: center"><div class="in-flow" data-offset-y="10"></div></div>"#));
    check("safe end", &format!(r#"<div class="test" style="{small} align-content: safe end"><div class="in-flow" data-offset-y="10"></div></div>"#));
    // 20 px of content in a 10 px box: `unsafe end` moves it 10 px up, `unsafe center` 5 px.
    check("unsafe end", &format!(r#"<div class="test" style="{small} align-content: unsafe end"><div class="in-flow" data-offset-y="0"></div></div>"#));
    check("unsafe center", &format!(r#"<div class="test" style="{small} align-content: unsafe center"><div class="in-flow" data-offset-y="5"></div></div>"#));
}

#[test]
fn floats_are_part_of_the_content() {
    check(
        "float",
        r#"<div class="test" style="align-content: end"><div style="float: right; height: 20px" data-offset-y="30"></div></div>"#,
    );
}

#[test]
fn inline_block_and_flow_root_align_too() {
    check("inline-block", r#"<div class="test" style="display: inline-block; width: 50px; align-content: center"><div class="in-flow" data-offset-y="25"></div></div>"#);
    check("flow-root", r#"<div class="test" style="display: flow-root; align-content: end"><div class="in-flow" data-offset-y="40"></div></div>"#);
}

#[test]
fn table_cell_follows_align_content_instead_of_vertical_align() {
    let table = |style: &str, offset: u32| {
        format!(
            r#"<table style="border-spacing: 0"><tr><td style="padding: 0; height: 50px; {style}"><div style="height: 10px" data-offset-y="{offset}"></div></td></tr></table>"#
        )
    };
    check("td default", &table("", 20));
    check("td start", &table("align-content: start", 0));
    check("td end", &table("align-content: end", 40));
    check("td center", &table("align-content: center", 20));
    check("td end beats vertical-align", &table("align-content: end; vertical-align: top", 40));
    check("td normal keeps vertical-align", &table("vertical-align: bottom", 40));
}

#[test]
fn empty_first_child_margin_joins_the_sibling_margin_instead_of_stacking() {
    // CSS 2.1 §8.3.1: the empty box's margins collapse through it and with the next sibling's,
    // all of them with the container's own — one 10 px gap above both children, not two.
    check(
        "empty first child",
        r#"<div class="test" style="height: auto"><div class="in-flow" style="height: 0" data-offset-y="0"></div><div class="in-flow" data-offset-y="0"></div></div>"#,
    );
}
