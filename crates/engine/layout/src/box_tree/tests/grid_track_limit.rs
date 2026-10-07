//! BUG-1320: `grid-template-columns` built from 100 000 × `repeat(1000, Npx)` (100 million tracks)
//! took 20 s and 12 GB; the explicit track count is clamped, so the layout stays cheap.

use std::time::Instant;

use lumen_core::geom::Size;

use super::super::layout;

#[test]
fn huge_repeat_track_list_lays_out_quickly() {
    let columns = (0..100_000).map(|i| format!(" repeat(1000, {i}px)")).collect::<String>();
    let doc = lumen_html_parser::parse("<body><i>x</i><i>y</i></body>");
    let sheet = lumen_css_parser::parse(&format!("body{{display:grid;grid-template-columns:{columns}}}"));
    let start = Instant::now();
    let root = layout(&doc, &sheet, Size::new(800.0, 600.0));
    let elapsed = start.elapsed();
    assert!(!root.children.is_empty());
    assert!(elapsed.as_secs_f32() < 5.0, "layout took {elapsed:?}");
}
