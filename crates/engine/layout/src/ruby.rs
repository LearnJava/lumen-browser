//! CSS Ruby Layout (CSS Ruby L1).
//!
//! Ruby is a W3C specification for small, annotative text (often phonetic) placed
//! above or beside base text, commonly used in CJK typography.
//!
//! Phase 0 (this implementation): simplified stacking of ruby text above base text
//! with center-alignment. No BIDI, no ruby-merge collapsing, no ruby-align positioning
//! (all treated as 'center'). Each `<ruby>` element layout:
//! 1. Measure base content.
//! 2. Measure ruby text.
//! 3. Place ruby above/below base, aligned to center.
//! 4. Return a stacked box with total height = base_height + ruby_height.

use lumen_core::geom::Rect;

/// Position of ruby text relative to base: above (`ruby` tag default) or below (`<rp>` alternate).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RubyPosition {
    /// Ruby text placed above base text.
    Over,
    /// Ruby text placed below base text (rare, used with `ruby-position: under`).
    Under,
}

/// A stacked ruby layout box: base text + ruby text positioned relative to it.
///
/// Contains the measured base boxes and ruby text height.
/// `// CSS: ruby-align, ruby-merge, ruby-position` — to be wired by P4.
#[derive(Debug, Clone)]
pub struct RubyBox {
    /// Laid out boxes for the base content (usually inline text).
    pub base_boxes: Vec<Rect>,
    /// Ruby text content (usually a short string).
    pub ruby_text: String,
    /// Ruby position: Over (default) or Under.
    pub position: RubyPosition,
    /// Measured ruby text height (glyph bounding box).
    pub ruby_height: f32,
    /// Measured ruby text width (glyph bounding box).
    pub ruby_width: f32,
}

impl RubyBox {
    /// Create a new ruby box with base content and ruby text.
    pub fn new(
        base_boxes: Vec<Rect>,
        ruby_text: String,
        ruby_height: f32,
        ruby_width: f32,
        position: RubyPosition,
    ) -> Self {
        Self {
            base_boxes,
            ruby_text,
            position,
            ruby_height,
            ruby_width,
        }
    }

    /// Get the bounding box of the full stacked ruby (base + ruby).
    /// Returned box is aligned center-over (ruby centered above base).
    pub fn bounding_box(&self) -> Option<Rect> {
        if self.base_boxes.is_empty() {
            return None;
        }
        let base = self.base_boxes[0];
        let base_width = base.width;
        let total_height = base.height + self.ruby_height;

        let x = base.x;
        let y = match self.position {
            RubyPosition::Over => base.y - self.ruby_height,
            RubyPosition::Under => base.y + base.height,
        };
        Some(Rect {
            x,
            y,
            width: base_width.max(self.ruby_width),
            height: total_height,
        })
    }
}

/// Lay out a ruby element: position base and ruby text vertically, with center-alignment.
///
/// # Arguments
/// * `base_boxes` — pre-laid-out base content (inline boxes)
/// * `ruby_text` — phonetic or annotative text
/// * `ruby_height`, `ruby_width` — glyph metrics of ruby text
/// * `position` — Over/Under
///
/// # Returns
/// A `RubyBox` ready for compositing. Phase 0: no inter-character spacing adjustment.
pub fn lay_out_ruby(
    base_boxes: Vec<Rect>,
    ruby_text: String,
    ruby_height: f32,
    ruby_width: f32,
    position: RubyPosition,
) -> RubyBox {
    RubyBox::new(base_boxes, ruby_text, ruby_height, ruby_width, position)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ruby_box_new() {
        let base = vec![Rect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 20.0,
        }];
        let ruby = RubyBox::new(base, "ゆ".to_string(), 14.0, 10.0, RubyPosition::Over);
        assert_eq!(ruby.ruby_text, "ゆ");
        assert_eq!(ruby.ruby_height, 14.0);
        assert_eq!(ruby.ruby_width, 10.0);
        assert_eq!(ruby.position, RubyPosition::Over);
    }

    #[test]
    fn ruby_position_over() {
        let base = vec![Rect {
            x: 10.0,
            y: 100.0,
            width: 80.0,
            height: 24.0,
        }];
        let ruby = RubyBox::new(base, "ruby".to_string(), 12.0, 30.0, RubyPosition::Over);
        let bbox = ruby.bounding_box().unwrap();
        assert_eq!(bbox.x, 10.0);
        assert_eq!(bbox.y, 100.0 - 12.0); // y = base.y - ruby_height
        assert_eq!(bbox.height, 24.0 + 12.0); // base + ruby height
    }

    #[test]
    fn ruby_position_under() {
        let base = vec![Rect {
            x: 10.0,
            y: 100.0,
            width: 80.0,
            height: 24.0,
        }];
        let ruby = RubyBox::new(base, "ruby".to_string(), 12.0, 30.0, RubyPosition::Under);
        let bbox = ruby.bounding_box().unwrap();
        assert_eq!(bbox.y, 100.0 + 24.0); // y = base.y + base.height
    }

    #[test]
    fn ruby_center_alignment() {
        let base = vec![Rect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 20.0,
        }];
        let ruby = RubyBox::new(base, "X".to_string(), 12.0, 20.0, RubyPosition::Over);
        let bbox = ruby.bounding_box().unwrap();
        // Ruby width 20 < base width 100, so it's centered.
        // Box width should be max(100, 20) = 100.
        assert_eq!(bbox.width, 100.0);
    }

    #[test]
    fn ruby_wider_than_base() {
        let base = vec![Rect {
            x: 0.0,
            y: 0.0,
            width: 50.0,
            height: 20.0,
        }];
        let ruby = RubyBox::new(base, "longer".to_string(), 12.0, 100.0, RubyPosition::Over);
        let bbox = ruby.bounding_box().unwrap();
        // Ruby width 100 > base width 50, so bounding box width = 100.
        assert_eq!(bbox.width, 100.0);
    }

    #[test]
    fn empty_base_boxes() {
        let ruby = RubyBox::new(vec![], "ruby".to_string(), 12.0, 30.0, RubyPosition::Over);
        assert!(ruby.bounding_box().is_none());
    }

    #[test]
    fn lay_out_ruby_function() {
        let base = vec![Rect {
            x: 0.0,
            y: 10.0,
            width: 90.0,
            height: 20.0,
        }];
        let result = lay_out_ruby(base, "rt".to_string(), 14.0, 25.0, RubyPosition::Over);
        assert_eq!(result.ruby_text, "rt");
        assert!(result.bounding_box().is_some());
    }

    #[test]
    fn ruby_box_multiple_base_boxes() {
        let base = vec![
            Rect {
                x: 0.0,
                y: 0.0,
                width: 50.0,
                height: 20.0,
            },
            Rect {
                x: 50.0,
                y: 0.0,
                width: 50.0,
                height: 20.0,
            },
        ];
        let ruby = RubyBox::new(base, "text".to_string(), 12.0, 40.0, RubyPosition::Over);
        // Uses only first base box for positioning.
        let bbox = ruby.bounding_box().unwrap();
        assert_eq!(bbox.x, 0.0);
        assert_eq!(bbox.y, 0.0 - 12.0);
    }
}
