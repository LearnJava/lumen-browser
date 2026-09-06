//! Тесты `block-step`/`block-step-size`/`-insert`/`-align`/`-round`
//! (CSS Rhythmic Sizing L1 §3, BUG-517): `crates/engine/layout/src/style/
//! values/misc.rs::{BlockStepInsert, BlockStepAlign, BlockStepRound}` +
//! `ComputedStyle::block_step_size`.
//!
//! Кейсы зеркалят `tests/wpt/css/css-rhythm/parsing/block-step{,-size,
//! -insert,-align,-round}-{valid,invalid,computed}.html`. Layout-эффект
//! (реальное деление на шаги при вёрстке) вне скоупа — Phase 0, тот же класс,
//! что `line-height-step` (BUG-517's sibling property in this module).

use super::*;

// ── initial values ───────────────────────────────────────────────────────

#[test]
fn initial_values() {
    let root = ComputedStyle::root();
    assert_eq!(root.block_step_size, None);
    assert_eq!(root.block_step_insert, BlockStepInsert::MarginBox);
    assert_eq!(root.block_step_align, BlockStepAlign::Auto);
    assert_eq!(root.block_step_round, BlockStepRound::Up);
}

#[test]
fn unset_declarations_keep_initial() {
    let s = style_for("color: red;");
    assert_eq!(s.block_step_size, None);
    assert_eq!(s.block_step_insert, BlockStepInsert::MarginBox);
    assert_eq!(s.block_step_align, BlockStepAlign::Auto);
    assert_eq!(s.block_step_round, BlockStepRound::Up);
}

// ── longhand parsing: block-step-size (parsing/block-step-size-{valid,invalid}.html) ──

#[test]
fn block_step_size_parses_lengths_and_none() {
    let s = style_for("font-size: 40px; block-step-size: 1px;");
    assert_eq!(s.block_step_size, Some(1.0));

    let s = style_for("font-size: 40px; block-step-size: 2em;");
    assert_eq!(s.block_step_size, Some(80.0));

    let s = style_for("font-size: 40px; block-step-size: 0;");
    assert_eq!(s.block_step_size, Some(0.0));

    let s = style_for("font-size: 40px; block-step-size: none;");
    assert_eq!(s.block_step_size, None);

    // calc(10px + 0.5em) at font-size 40px = 10 + 20 = 30px.
    let s = style_for("font-size: 40px; block-step-size: calc(10px + 0.5em);");
    assert_eq!(s.block_step_size, Some(30.0));

    // calc(10px - 0.5em) at font-size 40px = 10 - 20 = -10px, clamped to 0.
    let s = style_for("font-size: 40px; block-step-size: calc(10px - 0.5em);");
    assert_eq!(s.block_step_size, Some(0.0));
}

#[test]
fn block_step_size_rejects_invalid() {
    // A literal negative length, a percentage, and an intrinsic-sizing
    // keyword are all invalid — the declaration is dropped, initial `none`
    // survives.
    for invalid in ["auto", "-1px", "min-content", "10%", "20"] {
        let s = style_for(&format!("block-step-size: {invalid};"));
        assert_eq!(s.block_step_size, None, "expected {invalid:?} to be rejected");
    }
}

// ── longhand parsing: block-step-insert/-align/-round ───────────────────

#[test]
fn block_step_insert_parses_valid_keywords() {
    assert_eq!(BlockStepInsert::parse("margin-box"), Some(BlockStepInsert::MarginBox));
    assert_eq!(BlockStepInsert::parse("padding-box"), Some(BlockStepInsert::PaddingBox));
    assert_eq!(BlockStepInsert::parse("content-box"), Some(BlockStepInsert::ContentBox));
    // `border-box` is not part of this property's grammar.
    assert_eq!(BlockStepInsert::parse("border-box"), None);
    assert_eq!(BlockStepInsert::parse("none"), None);
}

#[test]
fn block_step_align_parses_valid_keywords() {
    assert_eq!(BlockStepAlign::parse("auto"), Some(BlockStepAlign::Auto));
    assert_eq!(BlockStepAlign::parse("center"), Some(BlockStepAlign::Center));
    assert_eq!(BlockStepAlign::parse("start"), Some(BlockStepAlign::Start));
    assert_eq!(BlockStepAlign::parse("end"), Some(BlockStepAlign::End));
    assert_eq!(BlockStepAlign::parse("bogus"), None);
}

#[test]
fn block_step_round_parses_valid_keywords() {
    assert_eq!(BlockStepRound::parse("up"), Some(BlockStepRound::Up));
    assert_eq!(BlockStepRound::parse("down"), Some(BlockStepRound::Down));
    assert_eq!(BlockStepRound::parse("nearest"), Some(BlockStepRound::Nearest));
    assert_eq!(BlockStepRound::parse("none"), None);
}

// ── not inherited ─────────────────────────────────────────────────────────

#[test]
fn block_step_properties_are_not_inherited() {
    let doc = lumen_html_parser::parse(
        r#"<div style="block-step-size: 100px; block-step-insert: padding-box; block-step-align: center; block-step-round: down"><p>x</p></div>"#,
    );
    let sheet = lumen_css_parser::parse("");
    let root_style = ComputedStyle::root();
    let div = doc.get(doc.body().unwrap()).children[0];
    let p = doc.get(div).children[0];
    let div_style = compute_style(&doc, div, &sheet, &root_style, Size::new(800.0, 600.0), false);
    assert_eq!(div_style.block_step_size, Some(100.0));
    let p_style = compute_style(&doc, p, &sheet, &div_style, Size::new(800.0, 600.0), false);
    // Not inherited: the child falls back to the initial values, not the
    // parent's.
    assert_eq!(p_style.block_step_size, None);
    assert_eq!(p_style.block_step_insert, BlockStepInsert::MarginBox);
    assert_eq!(p_style.block_step_align, BlockStepAlign::Auto);
    assert_eq!(p_style.block_step_round, BlockStepRound::Up);
}

// ── shorthand: parsing/block-step-{valid,invalid,computed}.html ─────────

fn block_step(css: &str) -> (Option<f32>, BlockStepInsert, BlockStepAlign, BlockStepRound) {
    let s = style_for(&format!("font-size: 40px; block-step: {css};"));
    (s.block_step_size, s.block_step_insert, s.block_step_align, s.block_step_round)
}

#[test]
fn shorthand_all_default_combinations_collapse_to_none() {
    // Every one of these specifies only initial-value keywords, in any
    // combination/order — the shorthand's own computed-value serialization
    // (`block_step_shorthand_computed`) collapses all of them to `"none"`.
    let all_default = [
        "none", "auto", "margin-box", "up",
        "none auto", "none margin-box", "none up",
        "auto none", "auto margin-box", "auto up",
        "margin-box none", "margin-box auto", "margin-box up",
        "up none", "up auto", "up margin-box",
        "auto up margin-box", "none auto up margin-box",
    ];
    for css in all_default {
        let (size, insert, align, round) = block_step(css);
        assert_eq!(size, None, "{css:?}");
        assert_eq!(insert, BlockStepInsert::MarginBox, "{css:?}");
        assert_eq!(align, BlockStepAlign::Auto, "{css:?}");
        assert_eq!(round, BlockStepRound::Up, "{css:?}");
    }
}

#[test]
fn shorthand_non_default_values_set_the_right_slot() {
    assert_eq!(block_step("padding-box").1, BlockStepInsert::PaddingBox);
    assert_eq!(block_step("content-box").1, BlockStepInsert::ContentBox);
    assert_eq!(block_step("100px").0, Some(100.0));
    assert_eq!(block_step("center").2, BlockStepAlign::Center);
    assert_eq!(block_step("start").2, BlockStepAlign::Start);
    assert_eq!(block_step("end").2, BlockStepAlign::End);
    assert_eq!(block_step("down").3, BlockStepRound::Down);
    assert_eq!(block_step("nearest").3, BlockStepRound::Nearest);
}

#[test]
fn shorthand_multi_component_any_order() {
    let (size, insert, align, round) = block_step("100px center down padding-box");
    assert_eq!(size, Some(100.0));
    assert_eq!(insert, BlockStepInsert::PaddingBox);
    assert_eq!(align, BlockStepAlign::Center);
    assert_eq!(round, BlockStepRound::Down);

    // Same four values, different token order — order-independent grammar.
    let (size2, insert2, align2, round2) = block_step("center content-box 100px");
    assert_eq!(size2, Some(100.0));
    assert_eq!(insert2, BlockStepInsert::ContentBox);
    assert_eq!(align2, BlockStepAlign::Center);
    assert_eq!(round2, BlockStepRound::Up);
}

#[test]
fn shorthand_rejects_duplicate_slot() {
    // Two tokens landing in the same slot (align twice, or two `size`-slot
    // tokens) invalidate the whole declaration — nothing is set, not even
    // the tokens that would otherwise have been unambiguous.
    for invalid in [
        "auto auto",
        "start end",
        "center start",
        "none none",
        "300px none",
        "300px start none",
        "300px start border-box padding-box",
    ] {
        let (size, insert, align, round) = block_step(invalid);
        assert_eq!(size, None, "{invalid:?}");
        assert_eq!(insert, BlockStepInsert::MarginBox, "{invalid:?}");
        assert_eq!(align, BlockStepAlign::Auto, "{invalid:?}");
        assert_eq!(round, BlockStepRound::Up, "{invalid:?}");
    }
}

#[test]
fn shorthand_computed_value_serialization() {
    // Mirrors `selector_query::computed_style_to_map`'s `"block-step"` entry.
    let computed = |css: &str| {
        let s = style_for(&format!("font-size: 40px; block-step: {css};"));
        crate::selector_query::computed_style_to_map(&s).get("block-step").cloned()
    };
    assert_eq!(computed("none"), Some("none".to_string()));
    assert_eq!(computed("auto up margin-box"), Some("none".to_string()));
    assert_eq!(computed("padding-box"), Some("padding-box".to_string()));
    assert_eq!(computed("100px"), Some("100px".to_string()));
    assert_eq!(
        computed("content-box 100px up"),
        Some("100px content-box".to_string())
    );
    assert_eq!(
        computed("content-box start 100px"),
        Some("100px content-box start".to_string())
    );
    assert_eq!(
        computed("100px center down padding-box"),
        Some("100px padding-box center down".to_string())
    );
    assert_eq!(computed("start 100px"), Some("100px start".to_string()));
    assert_eq!(computed("end 100px"), Some("100px end".to_string()));
    assert_eq!(
        computed("end nearest 100px"),
        Some("100px end nearest".to_string())
    );
    assert_eq!(
        computed("center content-box 100px"),
        Some("100px content-box center".to_string())
    );
}
