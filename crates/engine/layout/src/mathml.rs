//! MathML Core layout (W3C MathML Core §4) — East Asian and mathematical content.
//!
//! MathML is a markup language for mathematical notation. Structure:
//! - `<math>` — root element
//! - `<mi>` — identifier (variable, symbol)
//! - `<mn>` — number
//! - `<mo>` — operator
//! - `<mrow>` — row grouping
//! - `<mfrac>` — fraction (numerator/denominator stacked)
//! - `<msqrt>` — square root with radicand
//! - `<msup>` — superscript (base + exponent)
//! - `<msub>` — subscript (base + lower index)
//!
//! Phase 0: basic recognition and stacking. Phase 1: math-style, math-depth CSS properties (P4 wiring).

use crate::box_tree::{LayoutBox, BoxKind};
use crate::style::ComputedStyle;
use lumen_core::geom::Rect;
use lumen_dom::NodeId;

/// Represents the type of MathML element and its visual role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MathmlElementKind {
    /// Root `<math>` element container.
    Math,
    /// `<mrow>` — horizontal grouping / row.
    Mrow,
    /// `<mi>` — identifier (variable, symbol).
    Mi,
    /// `<mn>` — number.
    Mn,
    /// `<mo>` — operator or punctuation.
    Mo,
    /// `<mfrac>` — fraction with numerator and denominator.
    Mfrac,
    /// `<msqrt>` — square root, single radicand child.
    Msqrt,
    /// `<msup>` — superscript: base + exponent (second child).
    Msup,
    /// `<msub>` — subscript: base + lower index (second child).
    Msub,
}

/// MathML box: container for mathematical notation.
///
/// Represents a `<math>` element or MathML child with its constituent parts (base, radicand, numerator, etc.).
/// Layout stacks base and annotation boxes per MathML Core spec.
#[derive(Debug, Clone)]
pub struct MathmlBox {
    /// Element type (Math, mrow, mfrac, msqrt, msup, msub, etc.).
    pub kind: MathmlElementKind,
    /// Main content boxes (radicand for msqrt, numerator for mfrac, base for msup/msub, etc.).
    pub main_boxes: Vec<LayoutBox>,
    /// Denominator for mfrac.
    pub denominator_boxes: Option<Vec<LayoutBox>>,
    /// Exponent for msup or lower index for msub.
    pub annotation_boxes: Option<Vec<LayoutBox>>,
    /// Scaling factor for superscript/subscript relative to base (default 0.7 = 70% of base size).
    pub annotation_scale: f32,
}

impl MathmlBox {
    /// Create a new MathML box for a given element type.
    pub fn new(kind: MathmlElementKind, main_boxes: Vec<LayoutBox>) -> Self {
        Self {
            kind,
            main_boxes,
            denominator_boxes: None,
            annotation_boxes: None,
            annotation_scale: 0.7,
        }
    }

    /// Set denominator boxes for mfrac elements.
    pub fn with_denominator(mut self, denominator_boxes: Vec<LayoutBox>) -> Self {
        self.denominator_boxes = Some(denominator_boxes);
        self
    }

    /// Set annotation (exponent/subscript) boxes.
    pub fn with_annotation(mut self, annotation_boxes: Vec<LayoutBox>) -> Self {
        self.annotation_boxes = Some(annotation_boxes);
        self
    }

    /// Set the scaling factor for annotations (superscript/subscript).
    pub fn with_annotation_scale(mut self, scale: f32) -> Self {
        self.annotation_scale = scale.max(0.1).min(1.0);
        self
    }
}

/// Collect MathML element structure from a DOM node.
///
/// Scans the MathML tree and returns a structured `MathmlBox` representation.
/// Recognizes `<math>`, `<mrow>`, `<mi>`, `<mn>`, `<mo>`, `<mfrac>`, `<msqrt>`, `<msup>`, `<msub>`.
///
/// # Arguments
/// - `root` — the root LayoutBox (usually representing `<math>`)
///
/// # Returns
/// A `MathmlBox` with properly categorized main/denominator/annotation children.
pub fn collect_mathml_structure(root: &LayoutBox) -> MathmlBox {
    // Determine element type from node (stub: default to Math for unknown).
    let kind = determine_mathml_kind(root);

    match kind {
        MathmlElementKind::Mfrac => collect_mfrac_structure(root),
        MathmlElementKind::Msqrt => collect_msqrt_structure(root),
        MathmlElementKind::Msup => collect_msup_structure(root),
        MathmlElementKind::Msub => collect_msub_structure(root),
        MathmlElementKind::Mrow | MathmlElementKind::Math => {
            // mrow and math are just containers; collect all children as main boxes.
            MathmlBox::new(kind, root.children.clone())
        }
        MathmlElementKind::Mi | MathmlElementKind::Mn | MathmlElementKind::Mo => {
            // Leaf elements: treat as self.
            MathmlBox::new(kind, root.children.clone())
        }
    }
}

/// Layout algorithm for MathML content.
///
/// Positions base content and annotations (numerator/denominator, exponent, etc.)
/// according to MathML Core spec. Fractions are stacked vertically with a horizontal rule.
/// Superscripts and subscripts are positioned relative to the base size and vertical offset.
///
/// # Returns
/// A composed LayoutBox with MathML content laid out per CSS MathML spec.
///
/// // CSS: math-style, math-depth
pub fn lay_out_mathml(mathml: &MathmlBox) -> LayoutBox {
    match mathml.kind {
        MathmlElementKind::Mfrac => lay_out_mfrac(mathml),
        MathmlElementKind::Msqrt => lay_out_msqrt(mathml),
        MathmlElementKind::Msup => lay_out_msup(mathml),
        MathmlElementKind::Msub => lay_out_msub(mathml),
        MathmlElementKind::Mrow | MathmlElementKind::Math => {
            // mrow/math: just stack main boxes horizontally.
            if mathml.main_boxes.is_empty() {
                panic!("lay_out_mathml: math/mrow requires at least one child");
            } else if mathml.main_boxes.len() == 1 {
                mathml.main_boxes[0].clone()
            } else {
                stack_boxes_horizontal(&mathml.main_boxes)
            }
        }
        MathmlElementKind::Mi | MathmlElementKind::Mn | MathmlElementKind::Mo => {
            // Leaf elements: return as-is or compose.
            if mathml.main_boxes.is_empty() {
                panic!("lay_out_mathml: leaf element requires content");
            } else if mathml.main_boxes.len() == 1 {
                mathml.main_boxes[0].clone()
            } else {
                stack_boxes_horizontal(&mathml.main_boxes)
            }
        }
    }
}

/// Determine MathML element type from a LayoutBox (stub).
/// In Phase 0, defaults to Math for unknown types.
fn determine_mathml_kind(_root: &LayoutBox) -> MathmlElementKind {
    // Stub: would inspect node name from DOM. For now, default to Math.
    MathmlElementKind::Math
}

/// Collect structure for `<mfrac>` (fraction) — numerator (first child) + denominator (second child).
fn collect_mfrac_structure(root: &LayoutBox) -> MathmlBox {
    let mut numerator_boxes = Vec::new();
    let mut denominator_boxes = Vec::new();

    for (idx, child) in root.children.iter().enumerate() {
        if idx == 0 {
            // First child is numerator.
            numerator_boxes.push(child.clone());
        } else if idx == 1 {
            // Second child is denominator.
            denominator_boxes.push(child.clone());
        }
    }

    MathmlBox::new(MathmlElementKind::Mfrac, numerator_boxes)
        .with_denominator(denominator_boxes)
}

/// Collect structure for `<msqrt>` (square root) — radicand is the only / first child.
fn collect_msqrt_structure(root: &LayoutBox) -> MathmlBox {
    let radicand_boxes = if !root.children.is_empty() {
        root.children.clone()
    } else {
        Vec::new()
    };

    MathmlBox::new(MathmlElementKind::Msqrt, radicand_boxes)
}

/// Collect structure for `<msup>` (superscript) — base (first child) + exponent (second child).
fn collect_msup_structure(root: &LayoutBox) -> MathmlBox {
    let mut base_boxes = Vec::new();
    let mut exponent_boxes = Vec::new();

    for (idx, child) in root.children.iter().enumerate() {
        if idx == 0 {
            base_boxes.push(child.clone());
        } else if idx == 1 {
            exponent_boxes.push(child.clone());
        }
    }

    MathmlBox::new(MathmlElementKind::Msup, base_boxes)
        .with_annotation(exponent_boxes)
        .with_annotation_scale(0.7)
}

/// Collect structure for `<msub>` (subscript) — base (first child) + lower index (second child).
fn collect_msub_structure(root: &LayoutBox) -> MathmlBox {
    let mut base_boxes = Vec::new();
    let mut index_boxes = Vec::new();

    for (idx, child) in root.children.iter().enumerate() {
        if idx == 0 {
            base_boxes.push(child.clone());
        } else if idx == 1 {
            index_boxes.push(child.clone());
        }
    }

    MathmlBox::new(MathmlElementKind::Msub, base_boxes)
        .with_annotation(index_boxes)
        .with_annotation_scale(0.7)
}

/// Lay out a fraction: numerator and denominator stacked vertically with horizontal rule between them.
fn lay_out_mfrac(mathml: &MathmlBox) -> LayoutBox {
    if mathml.main_boxes.is_empty() {
        panic!("lay_out_mfrac: fraction requires numerator");
    }

    let numerator = if mathml.main_boxes.len() == 1 {
        mathml.main_boxes[0].clone()
    } else {
        stack_boxes_horizontal(&mathml.main_boxes)
    };

    let denominator = if let Some(denom_boxes) = &mathml.denominator_boxes {
        if denom_boxes.is_empty() {
            panic!("lay_out_mfrac: fraction requires denominator");
        } else if denom_boxes.len() == 1 {
            denom_boxes[0].clone()
        } else {
            stack_boxes_horizontal(denom_boxes)
        }
    } else {
        panic!("lay_out_mfrac: fraction requires denominator");
    };

    // Stack numerator + fraction rule + denominator vertically.
    stack_boxes_vertical(&[numerator, denominator])
}

/// Lay out a square root: radicand with radical sign (√) positioned at top-left.
fn lay_out_msqrt(mathml: &MathmlBox) -> LayoutBox {
    if mathml.main_boxes.is_empty() {
        panic!("lay_out_msqrt: sqrt requires radicand");
    }

    let radicand = if mathml.main_boxes.len() == 1 {
        mathml.main_boxes[0].clone()
    } else {
        stack_boxes_horizontal(&mathml.main_boxes)
    };

    // Phase 0: Simply return radicand (sqrt symbol would be added by P4 CSS rendering).
    // In Phase 1, apply sqrt styling and scale.
    radicand
}

/// Lay out a superscript: base with exponent positioned at top-right, scaled by annotation_scale.
fn lay_out_msup(mathml: &MathmlBox) -> LayoutBox {
    if mathml.main_boxes.is_empty() {
        panic!("lay_out_msup: superscript requires base");
    }

    let base = if mathml.main_boxes.len() == 1 {
        mathml.main_boxes[0].clone()
    } else {
        stack_boxes_horizontal(&mathml.main_boxes)
    };

    // If no exponent, just return base.
    if mathml.annotation_boxes.is_none() {
        return base;
    }

    let annot_boxes = mathml.annotation_boxes.as_ref().unwrap();
    if annot_boxes.is_empty() {
        return base;
    }

    let exponent = if annot_boxes.len() == 1 {
        scale_box(&annot_boxes[0], mathml.annotation_scale)
    } else {
        scale_box(&stack_boxes_horizontal(annot_boxes), mathml.annotation_scale)
    };

    // Position exponent at top-right of base (Phase 0: simple horizontal stacking with scaling).
    position_annotation_at_top_right(&base, &exponent)
}

/// Lay out a subscript: base with subscript positioned at bottom-right, scaled by annotation_scale.
fn lay_out_msub(mathml: &MathmlBox) -> LayoutBox {
    if mathml.main_boxes.is_empty() {
        panic!("lay_out_msub: subscript requires base");
    }

    let base = if mathml.main_boxes.len() == 1 {
        mathml.main_boxes[0].clone()
    } else {
        stack_boxes_horizontal(&mathml.main_boxes)
    };

    // If no subscript, just return base.
    if mathml.annotation_boxes.is_none() {
        return base;
    }

    let annot_boxes = mathml.annotation_boxes.as_ref().unwrap();
    if annot_boxes.is_empty() {
        return base;
    }

    let subscript = if annot_boxes.len() == 1 {
        scale_box(&annot_boxes[0], mathml.annotation_scale)
    } else {
        scale_box(&stack_boxes_horizontal(annot_boxes), mathml.annotation_scale)
    };

    // Position subscript at bottom-right of base (Phase 0: simple horizontal stacking with scaling).
    position_annotation_at_bottom_right(&base, &subscript)
}

/// Create an empty anonymous box for layout stacking with the given style.
fn make_anonymous_box_with_style(style: ComputedStyle) -> LayoutBox {
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
    }
}

/// Stack boxes horizontally (side by side).
fn stack_boxes_horizontal(boxes: &[LayoutBox]) -> LayoutBox {
    if boxes.is_empty() {
        panic!("stack_boxes_horizontal: expected at least one box");
    }

    if boxes.len() == 1 {
        return boxes[0].clone();
    }

    let style = boxes[0].style.clone();
    let mut total_width: f32 = 0.0;
    let mut max_height: f32 = 0.0;

    for box_item in boxes {
        total_width += box_item.rect.width;
        max_height = max_height.max(box_item.rect.height);
    }

    let mut result = make_anonymous_box_with_style(style);
    result.rect = Rect::new(0.0, 0.0, total_width, max_height);
    result.children = boxes.to_vec();
    result
}

/// Stack boxes vertically (one above the other).
fn stack_boxes_vertical(boxes: &[LayoutBox]) -> LayoutBox {
    if boxes.is_empty() {
        panic!("stack_boxes_vertical: expected at least one box");
    }

    if boxes.len() == 1 {
        return boxes[0].clone();
    }

    let style = boxes[0].style.clone();
    let mut max_width: f32 = 0.0;
    let mut total_height: f32 = 0.0;

    for box_item in boxes {
        max_width = max_width.max(box_item.rect.width);
        total_height += box_item.rect.height;
    }

    let mut result = make_anonymous_box_with_style(style);
    result.rect = Rect::new(0.0, 0.0, max_width, total_height);
    result.children = boxes.to_vec();
    result
}

/// Scale a box by the given factor (used for superscript/subscript sizing).
fn scale_box(box_item: &LayoutBox, scale: f32) -> LayoutBox {
    let mut scaled = box_item.clone();
    let new_width = scaled.rect.width * scale;
    let new_height = scaled.rect.height * scale;
    scaled.rect = Rect::new(scaled.rect.x, scaled.rect.y, new_width, new_height);
    scaled
}

/// Position annotation box (exponent) at top-right of base box.
fn position_annotation_at_top_right(base: &LayoutBox, annotation: &LayoutBox) -> LayoutBox {
    let style = base.style.clone();
    let mut result = make_anonymous_box_with_style(style);

    // Total width = base width + annotation width (with small gap).
    let gap = 2.0;
    let total_width = base.rect.width + annotation.rect.width + gap;

    // Height = base height (annotation aligned to top).
    let total_height = base.rect.height.max(annotation.rect.height);

    result.rect = Rect::new(0.0, 0.0, total_width, total_height);
    result.children = vec![base.clone(), annotation.clone()];

    result
}

/// Position annotation box (subscript) at bottom-right of base box.
fn position_annotation_at_bottom_right(base: &LayoutBox, annotation: &LayoutBox) -> LayoutBox {
    let style = base.style.clone();
    let mut result = make_anonymous_box_with_style(style);

    // Total width = base width + annotation width (with small gap).
    let gap = 2.0;
    let total_width = base.rect.width + annotation.rect.width + gap;

    // Height = base height + annotation height (with small gap).
    let total_height = base.rect.height + annotation.rect.height + gap;

    result.rect = Rect::new(0.0, 0.0, total_width, total_height);
    result.children = vec![base.clone(), annotation.clone()];

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mathml_element_kind_values() {
        assert_ne!(MathmlElementKind::Math, MathmlElementKind::Mfrac);
        assert_ne!(MathmlElementKind::Msup, MathmlElementKind::Msub);
        assert_ne!(MathmlElementKind::Mi, MathmlElementKind::Mn);
    }

    #[test]
    fn test_mathml_box_new() {
        let mathml = MathmlBox::new(MathmlElementKind::Math, vec![]);

        assert_eq!(mathml.kind, MathmlElementKind::Math);
        assert_eq!(mathml.main_boxes.len(), 0);
        assert!(mathml.denominator_boxes.is_none());
        assert!(mathml.annotation_boxes.is_none());
        assert_eq!(mathml.annotation_scale, 0.7);
    }

    #[test]
    fn test_mathml_box_with_denominator() {
        let mathml = MathmlBox::new(MathmlElementKind::Mfrac, vec![])
            .with_denominator(vec![]);

        assert_eq!(mathml.kind, MathmlElementKind::Mfrac);
        assert!(mathml.denominator_boxes.is_some());
        assert_eq!(mathml.denominator_boxes.as_ref().unwrap().len(), 0);
    }

    #[test]
    fn test_mathml_box_with_annotation() {
        let mathml = MathmlBox::new(MathmlElementKind::Msup, vec![])
            .with_annotation(vec![]);

        assert_eq!(mathml.kind, MathmlElementKind::Msup);
        assert!(mathml.annotation_boxes.is_some());
        assert_eq!(mathml.annotation_boxes.as_ref().unwrap().len(), 0);
    }

    #[test]
    fn test_mathml_with_annotation_scale() {
        let mathml = MathmlBox::new(MathmlElementKind::Msup, vec![])
            .with_annotation_scale(0.5);

        assert_eq!(mathml.annotation_scale, 0.5);
    }

    #[test]
    fn test_mathml_annotation_scale_clamped() {
        let mathml_low = MathmlBox::new(MathmlElementKind::Msup, vec![])
            .with_annotation_scale(0.05);

        assert_eq!(mathml_low.annotation_scale, 0.1);

        let mathml_high = MathmlBox::new(MathmlElementKind::Msup, vec![])
            .with_annotation_scale(1.5);

        assert_eq!(mathml_high.annotation_scale, 1.0);
    }

    #[test]
    fn test_mathml_all_element_kinds() {
        let kinds = [
            MathmlElementKind::Math,
            MathmlElementKind::Mrow,
            MathmlElementKind::Mi,
            MathmlElementKind::Mn,
            MathmlElementKind::Mo,
            MathmlElementKind::Mfrac,
            MathmlElementKind::Msqrt,
            MathmlElementKind::Msup,
            MathmlElementKind::Msub,
        ];

        for kind in &kinds {
            let mathml = MathmlBox::new(*kind, vec![]);
            assert_eq!(mathml.kind, *kind);
        }
    }

    #[test]
    fn test_mathml_builder_chain() {
        let mathml = MathmlBox::new(MathmlElementKind::Mfrac, vec![])
            .with_denominator(vec![])
            .with_annotation_scale(0.8);

        assert_eq!(mathml.kind, MathmlElementKind::Mfrac);
        assert!(mathml.denominator_boxes.is_some());
        assert_eq!(mathml.annotation_scale, 0.8);
    }
}
