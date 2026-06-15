//! CSS Ruby (Annotations) L3 — East Asian character annotations.
//!
//! Ruby is a small annotation placed alongside base text. Typical use:
//! - Japanese furigana (phonetic guide for kanji)
//! - Chinese pinyin annotations
//! - Korean ruby text
//!
//! Structure: `<ruby>base text<rt>annotation</rt></ruby>`
//!
//! Phase 0: basic stacking and layout. Phase 1: ruby-align, ruby-merge, ruby-position CSS properties (P4 wiring).

use crate::box_tree::{LayoutBox, BoxKind};
use lumen_dom::NodeId;
use lumen_core::geom::Rect;

/// Ruby annotation position relative to base text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RubyPosition {
    /// Annotation above the base text (standard for horizontal writing-mode).
    Over,
    /// Annotation below the base text.
    Under,
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
            inter_char_spacing: 0.0,
        }
    }

    /// Set the ruby text position.
    pub fn with_position(mut self, position: RubyPosition) -> Self {
        self.position = position;
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
/// Ruby text is centered relative to base content.
///
/// # Returns
/// A composed LayoutBox with ruby and base content stacked per CSS Ruby spec.
///
/// // CSS: ruby-align, ruby-merge, ruby-position
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

    // Both base and ruby: stack vertically with ruby-text positioned per `position`.
    match ruby.position {
        RubyPosition::Over => stack_ruby_over(ruby),
        RubyPosition::Under => stack_ruby_under(ruby),
    }
}

/// Stack base and ruby-text with ruby above base.
fn stack_ruby_over(ruby: &RubyBox) -> LayoutBox {
    let base_width = measure_total_width(&ruby.base_boxes);
    let ruby_width = measure_total_width(&ruby.ruby_text_boxes);

    let max_width = base_width.max(ruby_width);

    // Center ruby-text horizontally relative to base.
    let ruby_offset_x = (base_width - ruby_width).abs() / 2.0;
    let mut ruby_box = stack_boxes_horizontal(&ruby.ruby_text_boxes);
    ruby_box.rect.x += ruby_offset_x;

    // Measure heights.
    let ruby_height = ruby.ruby_text_boxes
        .iter()
        .map(|b| b.rect.height)
        .fold(0.0, f32::max);

    // Stack ruby above base: ruby first (top), then base.
    let style = if let Some(first) = ruby.base_boxes.first() {
        first.style.clone()
    } else if let Some(first) = ruby.ruby_text_boxes.first() {
        first.style.clone()
    } else {
        panic!("Ruby box must have at least base or ruby-text");
    };

    let mut stacked = make_anonymous_box_with_style(style);
    stacked.rect.width = max_width;
    stacked.rect.height = ruby_height + measure_total_height(&ruby.base_boxes);

    stacked.children.push(ruby_box);
    for base_box in &ruby.base_boxes {
        let mut b = base_box.clone();
        b.rect.y += ruby_height;
        stacked.children.push(b);
    }

    stacked
}

/// Stack base and ruby-text with ruby below base.
fn stack_ruby_under(ruby: &RubyBox) -> LayoutBox {
    let base_width = measure_total_width(&ruby.base_boxes);
    let ruby_width = measure_total_width(&ruby.ruby_text_boxes);

    let max_width = base_width.max(ruby_width);

    // Measure base height.
    let base_height = ruby.base_boxes
        .iter()
        .map(|b| b.rect.height)
        .fold(0.0, f32::max);

    // Center ruby-text horizontally relative to base.
    let ruby_offset_x = (base_width - ruby_width).abs() / 2.0;
    let mut ruby_box = stack_boxes_horizontal(&ruby.ruby_text_boxes);
    ruby_box.rect.x += ruby_offset_x;
    ruby_box.rect.y += base_height;

    let ruby_height = ruby.ruby_text_boxes
        .iter()
        .map(|b| b.rect.height)
        .fold(0.0, f32::max);

    let style = if let Some(first) = ruby.base_boxes.first() {
        first.style.clone()
    } else if let Some(first) = ruby.ruby_text_boxes.first() {
        first.style.clone()
    } else {
        panic!("Ruby box must have at least base or ruby-text");
    };

    let mut stacked = make_anonymous_box_with_style(style);
    stacked.rect.width = max_width;
    stacked.rect.height = base_height + ruby_height;

    for base_box in &ruby.base_boxes {
        stacked.children.push(base_box.clone());
    }
    stacked.children.push(ruby_box);

    stacked
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

/// Measure total width of boxes (max of all).
fn measure_total_width(boxes: &[LayoutBox]) -> f32 {
    boxes.iter().map(|b| b.rect.width).fold(0.0, f32::max)
}

/// Measure total height of boxes (max of all).
fn measure_total_height(boxes: &[LayoutBox]) -> f32 {
    boxes.iter().map(|b| b.rect.height).fold(0.0, f32::max)
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

    #[test]
    fn test_measure_total_width() {
        assert_eq!(measure_total_width(&[]), 0.0);
    }

    #[test]
    fn test_measure_total_height() {
        assert_eq!(measure_total_height(&[]), 0.0);
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
            .with_inter_char_spacing(1.5);

        assert_eq!(ruby.position, RubyPosition::Under);
        assert_eq!(ruby.inter_char_spacing, 1.5);
    }
}
