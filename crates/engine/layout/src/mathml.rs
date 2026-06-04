//! MathML Core layout (W3C MathML 3 / Core).
//!
//! Phase 0: Basic layout support for common MathML elements with simplified
//! positioning. Full MathML spec is complex (stretchy operators, baseline shifts,
//! scriptlevel, etc.); this stub implements:
//!
//! - Element recognition: `<math>`, `<mrow>`, `<mi>`, `<mn>`, `<mo>`, `<mfrac>`,
//!   `<msqrt>`, `<msup>`, `<msub>`.
//! - `<mfrac>` (fraction): horizontal rule, numerator above, denominator below.
//! - `<msqrt>` (square root): radical symbol, radicand to the right.
//! - `<msup>` / `<msub>`: superscript/subscript offset from baseline.
//!
//! TODO (P4): `math-style` (display/inline), `math-depth` (scriptlevel control).

use lumen_core::geom::Rect;

/// Recognized MathML element types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MathMLElementType {
    /// Root `<math>` element (container).
    Math,
    /// `<mrow>` (group / row).
    MRow,
    /// `<mi>` (identifier).
    Mi,
    /// `<mn>` (number).
    Mn,
    /// `<mo>` (operator).
    Mo,
    /// `<mfrac>` (fraction: numerator / denominator).
    Mfrac,
    /// `<msqrt>` (square root).
    Msqrt,
    /// `<msup>` (superscript).
    Msup,
    /// `<msub>` (subscript).
    Msub,
}

/// A laid-out MathML box: content rect + element type.
#[derive(Debug, Clone)]
pub struct MathMLBox {
    /// Bounding box of the element's content.
    pub content: Rect,
    /// Type of MathML element.
    pub element_type: MathMLElementType,
    /// Child boxes (for composite elements like mfrac, msqrt, msup, msub).
    pub children: Vec<MathMLBox>,
}

impl MathMLBox {
    /// Create a new MathML box.
    pub fn new(content: Rect, element_type: MathMLElementType) -> Self {
        Self {
            content,
            element_type,
            children: Vec::new(),
        }
    }

    /// Add a child box (for mfrac, msqrt, msup, msub).
    pub fn add_child(mut self, child: MathMLBox) -> Self {
        self.children.push(child);
        self
    }
}

/// Recognize MathML element name and return the type.
/// Returns `None` if not a recognized MathML element.
pub fn recognize_mathml_element(tag_name: &str) -> Option<MathMLElementType> {
    match tag_name.to_lowercase().as_str() {
        "math" => Some(MathMLElementType::Math),
        "mrow" => Some(MathMLElementType::MRow),
        "mi" => Some(MathMLElementType::Mi),
        "mn" => Some(MathMLElementType::Mn),
        "mo" => Some(MathMLElementType::Mo),
        "mfrac" => Some(MathMLElementType::Mfrac),
        "msqrt" => Some(MathMLElementType::Msqrt),
        "msup" => Some(MathMLElementType::Msup),
        "msub" => Some(MathMLElementType::Msub),
        _ => None,
    }
}

/// Lay out a fraction (`<mfrac>`): numerator above, denominator below, with
/// a horizontal rule between them.
///
/// # Arguments
/// * `numerator` — laid-out content (top)
/// * `denominator` — laid-out content (bottom)
/// * `rule_thickness` — thickness of the fraction bar
///
/// Returns a `MathMLBox` with stacked layout.
pub fn lay_out_fraction(numerator: MathMLBox, denominator: MathMLBox, rule_thickness: f32) -> MathMLBox {
    let total_height = numerator.content.height + rule_thickness + denominator.content.height;
    let max_width = numerator.content.width.max(denominator.content.width);
    let box_rect = Rect {
        x: 0.0,
        y: 0.0,
        width: max_width,
        height: total_height,
    };
    MathMLBox::new(box_rect, MathMLElementType::Mfrac)
        .add_child(numerator)
        .add_child(denominator)
}

/// Lay out a square root (`<msqrt>`): radical symbol + radicand.
///
/// # Arguments
/// * `radicand` — the content under the root
/// * `radical_width` — width of the radical symbol
///
/// Returns a `MathMLBox` with radicand positioned to the right.
pub fn lay_out_sqrt(radicand: MathMLBox, radical_width: f32) -> MathMLBox {
    let total_width = radical_width + radicand.content.width;
    let total_height = radicand.content.height;
    let box_rect = Rect {
        x: 0.0,
        y: 0.0,
        width: total_width,
        height: total_height,
    };
    MathMLBox::new(box_rect, MathMLElementType::Msqrt).add_child(radicand)
}

/// Lay out a superscript (`<msup>`): base + superscript offset above.
///
/// # Arguments
/// * `base` — base element
/// * `superscript` — superscript element
/// * `script_offset_y` — vertical offset above baseline (negative = above)
///
/// Returns a `MathMLBox` with superscript positioned above-right.
pub fn lay_out_superscript(
    base: MathMLBox,
    superscript: MathMLBox,
    script_offset_y: f32,
) -> MathMLBox {
    let total_width = base.content.width + superscript.content.width;
    let total_height = base.content.height + (script_offset_y.abs()).max(0.0);
    let box_rect = Rect {
        x: 0.0,
        y: 0.0,
        width: total_width,
        height: total_height,
    };
    MathMLBox::new(box_rect, MathMLElementType::Msup)
        .add_child(base)
        .add_child(superscript)
}

/// Lay out a subscript (`<msub>`): base + subscript offset below.
///
/// # Arguments
/// * `base` — base element
/// * `subscript` — subscript element
/// * `script_offset_y` — vertical offset below baseline (positive = below)
///
/// Returns a `MathMLBox` with subscript positioned below-right.
pub fn lay_out_subscript(base: MathMLBox, subscript: MathMLBox, script_offset_y: f32) -> MathMLBox {
    let total_width = base.content.width + subscript.content.width;
    let total_height = base.content.height + script_offset_y.max(0.0);
    let box_rect = Rect {
        x: 0.0,
        y: 0.0,
        width: total_width,
        height: total_height,
    };
    MathMLBox::new(box_rect, MathMLElementType::Msub)
        .add_child(base)
        .add_child(subscript)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognize_math_element() {
        assert_eq!(recognize_mathml_element("math"), Some(MathMLElementType::Math));
        assert_eq!(recognize_mathml_element("mrow"), Some(MathMLElementType::MRow));
        assert_eq!(recognize_mathml_element("mi"), Some(MathMLElementType::Mi));
    }

    #[test]
    fn recognize_unknown_element() {
        assert_eq!(recognize_mathml_element("div"), None);
        assert_eq!(recognize_mathml_element("span"), None);
    }

    #[test]
    fn recognize_case_insensitive() {
        assert_eq!(recognize_mathml_element("MATH"), Some(MathMLElementType::Math));
        assert_eq!(recognize_mathml_element("MFrac"), Some(MathMLElementType::Mfrac));
    }

    #[test]
    fn mathml_box_new() {
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            width: 50.0,
            height: 20.0,
        };
        let box_m = MathMLBox::new(rect, MathMLElementType::Mi);
        assert_eq!(box_m.element_type, MathMLElementType::Mi);
        assert_eq!(box_m.content.width, 50.0);
    }

    #[test]
    fn lay_out_fraction_simple() {
        let num = MathMLBox::new(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 30.0,
                height: 15.0,
            },
            MathMLElementType::Mn,
        );
        let denom = MathMLBox::new(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 30.0,
                height: 15.0,
            },
            MathMLElementType::Mn,
        );
        let frac = lay_out_fraction(num, denom, 2.0);
        assert_eq!(frac.element_type, MathMLElementType::Mfrac);
        assert_eq!(frac.content.height, 15.0 + 2.0 + 15.0); // num + rule + denom
        assert_eq!(frac.children.len(), 2);
    }

    #[test]
    fn lay_out_sqrt_simple() {
        let radicand = MathMLBox::new(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 40.0,
                height: 20.0,
            },
            MathMLElementType::Mn,
        );
        let sqrt = lay_out_sqrt(radicand, 15.0);
        assert_eq!(sqrt.element_type, MathMLElementType::Msqrt);
        assert_eq!(sqrt.content.width, 15.0 + 40.0); // radical + radicand
        assert_eq!(sqrt.children.len(), 1);
    }

    #[test]
    fn lay_out_superscript_simple() {
        let base = MathMLBox::new(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 30.0,
                height: 20.0,
            },
            MathMLElementType::Mi,
        );
        let super_s = MathMLBox::new(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            },
            MathMLElementType::Mn,
        );
        let sup = lay_out_superscript(base, super_s, -5.0);
        assert_eq!(sup.element_type, MathMLElementType::Msup);
        assert_eq!(sup.content.width, 30.0 + 10.0);
        assert_eq!(sup.children.len(), 2);
    }

    #[test]
    fn lay_out_subscript_simple() {
        let base = MathMLBox::new(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 30.0,
                height: 20.0,
            },
            MathMLElementType::Mi,
        );
        let sub_s = MathMLBox::new(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            },
            MathMLElementType::Mn,
        );
        let sub = lay_out_subscript(base, sub_s, 5.0);
        assert_eq!(sub.element_type, MathMLElementType::Msub);
        assert_eq!(sub.content.height, 20.0 + 5.0); // base + offset
        assert_eq!(sub.children.len(), 2);
    }
}
