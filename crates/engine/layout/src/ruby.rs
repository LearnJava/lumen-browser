//! CSS Ruby (Annotations) L3 — East Asian character annotations.
//!
//! Ruby is a small annotation placed alongside base text. Typical use:
//! - Japanese furigana (phonetic guide for kanji)
//! - Chinese pinyin annotations
//! - Korean ruby text
//!
//! Structure: `<ruby>base text<rt>annotation</rt></ruby>`
//!
//! Phase 0: basic stacking and layout. Phase 1 (done): ruby-align, ruby-merge,
//! ruby-position are parsed into `ComputedStyle` and drive this algorithm via
//! [`RubyBox::from_style`]. Box-tree integration of `<ruby>` elements into the
//! inline flow is deferred (this module has no pipeline callers yet).

use crate::box_tree::{LayoutBox, BoxKind};
use crate::style::ComputedStyle;
use lumen_dom::NodeId;
use lumen_core::geom::Rect;

/// CSS Ruby L1 §4 — `ruby-position`. Inherited. Initial: `over`.
///
/// Position of the annotation relative to base text. The spec's `alternate`
/// and `inter-character` values are not supported (parsed as `over`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RubyPosition {
    /// Annotation above the base text (standard for horizontal writing-mode).
    #[default]
    Over,
    /// Annotation below the base text.
    Under,
}

/// CSS Ruby L1 §4 — `ruby-align`. Inherited. Initial: `space-around`.
///
/// Distributes ruby annotation (or base) content within its column when the
/// other level is wider.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RubyAlign {
    /// `start` — flush with the line-start edge.
    Start,
    /// `center` — centered within the column.
    Center,
    /// `space-between` — extra space distributed between boxes; single box is start-aligned.
    SpaceBetween,
    /// `space-around` — extra space distributed around boxes; single box is centered.
    #[default]
    SpaceAround,
}

/// CSS Ruby L1 §4 — `ruby-merge`. Inherited. Initial: `separate`.
///
/// Controls whether annotations pair with their own base (`separate`) or span
/// all bases as one merged annotation (`merge`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RubyMerge {
    /// `separate` — each annotation is laid out over its own base (paired by index).
    #[default]
    Separate,
    /// `merge` — all annotations form one span across all bases.
    Merge,
    /// `auto` — renderer's choice: pairs when base/annotation counts match, merges otherwise.
    Auto,
}

/// Ruby box: base text with optional annotation.
///
/// Represents a `<ruby>` element with base content and `<rt>` (ruby text) children.
/// Layout stacks base and ruby-text boxes with center alignment.
#[derive(Debug, Clone)]
pub struct RubyBox {
    /// Boxes for the base content (inside `<ruby>` but outside `<rt>`).
    pub base_boxes: Vec<LayoutBox>,
    /// Boxes for the ruby text (each `<rt>` child).
    pub ruby_text_boxes: Vec<LayoutBox>,
    /// Position of ruby text relative to base.
    pub position: RubyPosition,
    /// Alignment/distribution of the narrower level within the ruby column.
    pub align: RubyAlign,
    /// Annotation pairing mode (per-base vs merged span).
    pub merge: RubyMerge,
    /// Inter-character spacing in em units (for horizontal writing-mode).
    pub inter_char_spacing: f32,
}

impl RubyBox {
    /// Create a new Ruby box with default Over positioning.
    pub fn new(
        base_boxes: Vec<LayoutBox>,
        ruby_text_boxes: Vec<LayoutBox>,
    ) -> Self {
        Self {
            base_boxes,
            ruby_text_boxes,
            position: RubyPosition::Over,
            align: RubyAlign::default(),
            merge: RubyMerge::default(),
            inter_char_spacing: 0.0,
        }
    }

    /// Create a Ruby box taking `ruby-position` / `ruby-align` / `ruby-merge`
    /// from the computed style of the `<ruby>` element.
    pub fn from_style(
        style: &ComputedStyle,
        base_boxes: Vec<LayoutBox>,
        ruby_text_boxes: Vec<LayoutBox>,
    ) -> Self {
        Self {
            base_boxes,
            ruby_text_boxes,
            position: style.ruby_position,
            align: style.ruby_align,
            merge: style.ruby_merge,
            inter_char_spacing: 0.0,
        }
    }

    /// Set the ruby text position.
    pub fn with_position(mut self, position: RubyPosition) -> Self {
        self.position = position;
        self
    }

    /// Set the annotation alignment mode.
    pub fn with_align(mut self, align: RubyAlign) -> Self {
        self.align = align;
        self
    }

    /// Set the annotation pairing mode.
    pub fn with_merge(mut self, merge: RubyMerge) -> Self {
        self.merge = merge;
        self
    }

    /// Set inter-character spacing in em units.
    pub fn with_inter_char_spacing(mut self, spacing: f32) -> Self {
        self.inter_char_spacing = spacing;
        self
    }
}

/// Layout algorithm for ruby annotations.
///
/// Stacks base and ruby text vertically (or horizontally depending on writing-mode).
/// `ruby-position` picks the annotation side, `ruby-align` distributes the
/// narrower level within the column, `ruby-merge` chooses between per-base
/// pairing and one merged annotation span.
///
/// # Returns
/// A composed LayoutBox with ruby and base content stacked per CSS Ruby spec.
pub fn lay_out_ruby(ruby: &RubyBox) -> LayoutBox {
    if ruby.base_boxes.is_empty() {
        // No base text: render ruby text alone.
        return compose_ruby_text_only(ruby);
    }

    if ruby.ruby_text_boxes.is_empty() {
        // No ruby text: render base text alone.
        if ruby.base_boxes.len() == 1 {
            return ruby.base_boxes[0].clone();
        }
        return stack_boxes_horizontal(&ruby.base_boxes);
    }

    // `separate`/`auto` pair each annotation with its own base when counts
    // match; `merge` (or a count mismatch) spans one annotation row over all bases.
    let pair = matches!(ruby.merge, RubyMerge::Separate | RubyMerge::Auto)
        && ruby.base_boxes.len() == ruby.ruby_text_boxes.len()
        && ruby.base_boxes.len() > 1;

    if pair {
        let columns: Vec<LayoutBox> = ruby
            .base_boxes
            .iter()
            .zip(ruby.ruby_text_boxes.iter())
            .map(|(base, annotation)| {
                compose_column(
                    std::slice::from_ref(base),
                    std::slice::from_ref(annotation),
                    ruby.position,
                    ruby.align,
                )
            })
            .collect();
        return stack_boxes_horizontal(&columns);
    }

    compose_column(&ruby.base_boxes, &ruby.ruby_text_boxes, ruby.position, ruby.align)
}

/// Compose one ruby column: a base row and an annotation row stacked per
/// `position`, with the narrower row distributed per `align`.
fn compose_column(
    bases: &[LayoutBox],
    annotations: &[LayoutBox],
    position: RubyPosition,
    align: RubyAlign,
) -> LayoutBox {
    let mut base_row = stack_boxes_horizontal(bases);
    let mut ruby_row = stack_boxes_horizontal(annotations);
    base_row.rect.x = 0.0;
    ruby_row.rect.x = 0.0;

    let col_width = base_row.rect.width.max(ruby_row.rect.width);
    align_row(&mut base_row, bases.len(), col_width, align);
    align_row(&mut ruby_row, annotations.len(), col_width, align);

    let base_height = base_row.rect.height;
    let ruby_height = ruby_row.rect.height;

    let mut column = make_anonymous_box_with_style(base_row.style.clone());
    column.rect.width = col_width;
    column.rect.height = base_height + ruby_height;

    match position {
        RubyPosition::Over => {
            base_row.rect.y += ruby_height;
            column.children.push(ruby_row);
            column.children.push(base_row);
        }
        RubyPosition::Under => {
            ruby_row.rect.y += base_height;
            column.children.push(base_row);
            column.children.push(ruby_row);
        }
    }

    column
}

/// Apply `ruby-align` to a row of `n_boxes` boxes inside a column of
/// `container_width`. No-op when the row already fills the column.
fn align_row(row: &mut LayoutBox, n_boxes: usize, container_width: f32, align: RubyAlign) {
    let slack = container_width - row.rect.width;
    if slack <= 0.0 {
        return;
    }
    match align {
        RubyAlign::Start => {}
        RubyAlign::Center => row.rect.x += slack / 2.0,
        RubyAlign::SpaceBetween => {
            // Distribution needs the anonymous row wrapper (n_boxes > 1);
            // a single box stays flush at the start edge.
            if n_boxes > 1 {
                let gap = slack / (n_boxes - 1) as f32;
                for (i, child) in row.children.iter_mut().enumerate() {
                    child.rect.x += gap * i as f32;
                }
                row.rect.width = container_width;
            }
        }
        RubyAlign::SpaceAround => {
            if n_boxes > 1 {
                let gap = slack / n_boxes as f32;
                for (i, child) in row.children.iter_mut().enumerate() {
                    child.rect.x += gap * (i as f32 + 0.5);
                }
                row.rect.width = container_width;
            } else {
                // Single box: centered.
                row.rect.x += slack / 2.0;
            }
        }
    }
}

/// Render ruby text only (no base).
fn compose_ruby_text_only(ruby: &RubyBox) -> LayoutBox {
    stack_boxes_horizontal(&ruby.ruby_text_boxes)
}

/// Stack boxes horizontally (left-to-right).
fn stack_boxes_horizontal(boxes: &[LayoutBox]) -> LayoutBox {
    if boxes.is_empty() {
        panic!("stack_boxes_horizontal requires at least one box");
    }

    if boxes.len() == 1 {
        return boxes[0].clone();
    }

    let style = boxes[0].style.clone();
    let mut stacked = make_anonymous_box_with_style(style);
    let mut cursor_x = 0.0;

    for (i, box_) in boxes.iter().enumerate() {
        let mut b = box_.clone();
        b.rect.x = cursor_x;
        if i > 0 {
            b.rect.x += 2.0; // Inter-character spacing.
        }
        cursor_x = b.rect.x + b.rect.width;
        stacked.children.push(b);
    }

    stacked.rect.width = cursor_x;
    stacked.rect.height = boxes
        .iter()
        .map(|b| b.rect.height)
        .fold(0.0, f32::max);

    stacked
}

/// Create an anonymous box (no DOM node) with the given style for stacking.
fn make_anonymous_box_with_style(style: crate::style::ComputedStyle) -> LayoutBox {
    LayoutBox {
        node: NodeId::from_index(0),
        rect: Rect::ZERO,
        style,
        kind: BoxKind::Block,
        children: vec![],
        col_span: 1,
        row_span: 1,
        svg_group_transform: None,
        scroll_x: 0.0,
        scroll_y: 0.0,
        dirty: Default::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ruby_position_enum() {
        assert_eq!(RubyPosition::Over, RubyPosition::Over);
        assert_ne!(RubyPosition::Over, RubyPosition::Under);
    }

    #[test]
    fn test_ruby_box_new() {
        let base = vec![];
        let ruby_text = vec![];
        let ruby = RubyBox::new(base, ruby_text);

        assert_eq!(ruby.position, RubyPosition::Over);
        assert_eq!(ruby.align, RubyAlign::SpaceAround);
        assert_eq!(ruby.merge, RubyMerge::Separate);
        assert_eq!(ruby.base_boxes.len(), 0);
        assert_eq!(ruby.ruby_text_boxes.len(), 0);
        assert_eq!(ruby.inter_char_spacing, 0.0);
    }

    #[test]
    fn test_ruby_box_with_position() {
        let ruby = RubyBox::new(vec![], vec![])
            .with_position(RubyPosition::Under);

        assert_eq!(ruby.position, RubyPosition::Under);
    }

    #[test]
    fn test_ruby_box_with_inter_char_spacing() {
        let ruby = RubyBox::new(vec![], vec![])
            .with_inter_char_spacing(2.5);

        assert_eq!(ruby.inter_char_spacing, 2.5);
    }

    /// Build a plain box with the given rect for composition tests.
    fn make_box(x: f32, y: f32, w: f32, h: f32) -> LayoutBox {
        let mut b = make_anonymous_box_with_style(ComputedStyle::root());
        b.rect = Rect { x, y, width: w, height: h };
        b
    }

    #[test]
    fn test_ruby_position_ordering() {
        let over = RubyPosition::Over;
        let under = RubyPosition::Under;

        assert_eq!(over, RubyPosition::Over);
        assert_eq!(under, RubyPosition::Under);
        assert_ne!(over, under);
    }

    #[test]
    fn test_ruby_box_builder_chain() {
        let ruby = RubyBox::new(vec![], vec![])
            .with_position(RubyPosition::Under)
            .with_align(RubyAlign::Start)
            .with_merge(RubyMerge::Merge)
            .with_inter_char_spacing(1.5);

        assert_eq!(ruby.position, RubyPosition::Under);
        assert_eq!(ruby.align, RubyAlign::Start);
        assert_eq!(ruby.merge, RubyMerge::Merge);
        assert_eq!(ruby.inter_char_spacing, 1.5);
    }

    #[test]
    fn test_ruby_box_from_style() {
        let mut style = ComputedStyle::root();
        style.ruby_position = RubyPosition::Under;
        style.ruby_align = RubyAlign::Center;
        style.ruby_merge = RubyMerge::Merge;

        let ruby = RubyBox::from_style(&style, vec![], vec![]);
        assert_eq!(ruby.position, RubyPosition::Under);
        assert_eq!(ruby.align, RubyAlign::Center);
        assert_eq!(ruby.merge, RubyMerge::Merge);
    }

    #[test]
    fn test_align_start_vs_center_single_annotation() {
        let base = make_box(0.0, 0.0, 100.0, 20.0);
        let rt = make_box(0.0, 0.0, 40.0, 10.0);

        let start = lay_out_ruby(
            &RubyBox::new(vec![base.clone()], vec![rt.clone()]).with_align(RubyAlign::Start),
        );
        // Over: children[0] = annotation row, children[1] = base row.
        assert_eq!(start.children[0].rect.x, 0.0);

        let center = lay_out_ruby(
            &RubyBox::new(vec![base.clone()], vec![rt.clone()]).with_align(RubyAlign::Center),
        );
        assert_eq!(center.children[0].rect.x, 30.0);

        // space-around with a single annotation box degenerates to centered,
        // space-between to start-aligned.
        let around = lay_out_ruby(
            &RubyBox::new(vec![base.clone()], vec![rt.clone()]).with_align(RubyAlign::SpaceAround),
        );
        assert_eq!(around.children[0].rect.x, 30.0);

        let between = lay_out_ruby(
            &RubyBox::new(vec![base], vec![rt]).with_align(RubyAlign::SpaceBetween),
        );
        assert_eq!(between.children[0].rect.x, 0.0);
    }

    #[test]
    fn test_position_over_under_row_order() {
        let base = make_box(0.0, 0.0, 50.0, 20.0);
        let rt = make_box(0.0, 0.0, 50.0, 10.0);

        let over = lay_out_ruby(
            &RubyBox::new(vec![base.clone()], vec![rt.clone()])
                .with_position(RubyPosition::Over),
        );
        assert_eq!(over.rect.height, 30.0);
        assert_eq!(over.children[0].rect.y, 0.0); // annotation on top
        assert_eq!(over.children[1].rect.y, 10.0); // base pushed down

        let under = lay_out_ruby(
            &RubyBox::new(vec![base], vec![rt]).with_position(RubyPosition::Under),
        );
        assert_eq!(under.rect.height, 30.0);
        assert_eq!(under.children[0].rect.y, 0.0); // base on top
        assert_eq!(under.children[1].rect.y, 20.0); // annotation below
    }

    #[test]
    fn test_merge_separate_pairs_columns() {
        let bases = vec![make_box(0.0, 0.0, 30.0, 20.0), make_box(0.0, 0.0, 30.0, 20.0)];
        let rts = vec![make_box(0.0, 0.0, 10.0, 8.0), make_box(0.0, 0.0, 10.0, 8.0)];

        // separate (default): two per-base columns joined horizontally.
        let separate = lay_out_ruby(&RubyBox::new(bases.clone(), rts.clone()));
        assert_eq!(separate.children.len(), 2);
        for column in &separate.children {
            assert_eq!(column.rect.height, 28.0); // base 20 + annotation 8
            assert_eq!(column.children.len(), 2); // annotation row + base row
        }

        // merge: one annotation row spanning all bases.
        let merged = lay_out_ruby(
            &RubyBox::new(bases, rts)
                .with_merge(RubyMerge::Merge)
                .with_align(RubyAlign::Start),
        );
        assert_eq!(merged.rect.height, 28.0);
        // children[0] = merged annotation row (width 10+2+10), children[1] = base row.
        assert_eq!(merged.children[0].rect.width, 22.0);
        assert_eq!(merged.children[1].rect.width, 62.0);
    }

    #[test]
    fn test_space_around_distributes_annotation_boxes() {
        let base = make_box(0.0, 0.0, 100.0, 20.0);
        let rts = vec![make_box(0.0, 0.0, 20.0, 8.0), make_box(0.0, 0.0, 20.0, 8.0)];

        let out = lay_out_ruby(
            &RubyBox::new(vec![base], rts)
                .with_merge(RubyMerge::Merge)
                .with_align(RubyAlign::SpaceAround),
        );
        let annotation_row = &out.children[0];
        // Row of two 20px boxes (2px inter-spacing) = 42px in a 100px column:
        // slack 58, gap 29 → first box shifted by 14.5, second by 43.5.
        assert_eq!(annotation_row.rect.width, 100.0);
        assert_eq!(annotation_row.children[0].rect.x, 14.5);
        assert_eq!(annotation_row.children[1].rect.x, 22.0 + 43.5);
    }
}
