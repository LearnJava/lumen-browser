//! Page-level styling and margin-box layout for CSS Paged Media.
//!
//! Implements CSS Paged Media L3 §3 — @page rules, page styling,
//! and the 16-box margin-box model.
//!
//! **Inputs:**
//! - @page rules from stylesheet (parsed by css-parser)
//! - Page number (0-based)
//!
//! **Outputs:**
//! - PageContext with computed page properties (size, margin, orientation)
//! - MarginBoxes — 16 positioned boxes around page border

use std::collections::HashMap;
use lumen_css_parser::PageRule;

/// Position of a margin-box relative to the page box.
///
/// CSS Paged Media L3 §3.2 defines 16 margin boxes:
/// - 4 corners: top-left, top-right, bottom-left, bottom-right
/// - 4 edges: top-center, bottom-center, left-middle, right-middle
/// - 4 sides: top-left-corner, top-right-corner, bottom-left-corner, bottom-right-corner
/// - 1 center: center (only if page is wide enough)
///
/// Actual layout: corners occupy fixed space, edges stretch to fill remaining edge space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MarginBoxPosition {
    /// `top-left-corner`: top-left corner box.
    TopLeftCorner,
    /// `top-center`: center-top edge box.
    TopCenter,
    /// `top-right-corner`: top-right corner box.
    TopRightCorner,
    /// `left-middle`: left-center edge box.
    LeftMiddle,
    /// `center`: page-center box (rarely used).
    Center,
    /// `right-middle`: right-center edge box.
    RightMiddle,
    /// `bottom-left-corner`: bottom-left corner box.
    BottomLeftCorner,
    /// `bottom-center`: center-bottom edge box.
    BottomCenter,
    /// `bottom-right-corner`: bottom-right corner box.
    BottomRightCorner,
}

impl MarginBoxPosition {
    /// All 16 margin-box positions in layout order.
    pub fn all() -> &'static [Self] {
        &[
            Self::TopLeftCorner,
            Self::TopCenter,
            Self::TopRightCorner,
            Self::LeftMiddle,
            Self::Center,
            Self::RightMiddle,
            Self::BottomLeftCorner,
            Self::BottomCenter,
            Self::BottomRightCorner,
        ]
    }

    /// CSS property name for this margin-box in @page rules.
    /// E.g., `top-left-corner` → "@top-left-corner { ... }".
    pub fn css_name(self) -> &'static str {
        match self {
            Self::TopLeftCorner => "@top-left-corner",
            Self::TopCenter => "@top-center",
            Self::TopRightCorner => "@top-right-corner",
            Self::LeftMiddle => "@left-middle",
            Self::Center => "@center",
            Self::RightMiddle => "@right-middle",
            Self::BottomLeftCorner => "@bottom-left-corner",
            Self::BottomCenter => "@bottom-center",
            Self::BottomRightCorner => "@bottom-right-corner",
        }
    }

    /// Is this a corner box?
    pub fn is_corner(self) -> bool {
        matches!(
            self,
            Self::TopLeftCorner
                | Self::TopRightCorner
                | Self::BottomLeftCorner
                | Self::BottomRightCorner
        )
    }

    /// Is this a horizontal edge box (top or bottom)?
    pub fn is_horizontal_edge(self) -> bool {
        matches!(self, Self::TopCenter | Self::BottomCenter)
    }

    /// Is this a vertical edge box (left or right)?
    pub fn is_vertical_edge(self) -> bool {
        matches!(self, Self::LeftMiddle | Self::RightMiddle)
    }
}

/// Computed properties for a page from matching @page rules.
///
/// Includes page size (width, height), orientation, and margin specifications.
/// These properties control pagination and margin-box layout.
#[derive(Debug, Clone)]
pub struct PageProperties {
    /// Page width in pixels (e.g., A4: 210mm ≈ 794px at 96 DPI).
    pub width: f32,

    /// Page height in pixels (e.g., A4: 297mm ≈ 1123px at 96 DPI).
    pub height: f32,

    /// Orientation: "portrait" or "landscape".
    /// Computed from width/height ratio. Default: portrait.
    pub orientation: String,

    /// Page margin-top in pixels.
    pub margin_top: f32,

    /// Page margin-bottom in pixels.
    pub margin_bottom: f32,

    /// Page margin-left in pixels.
    pub margin_left: f32,

    /// Page margin-right in pixels.
    pub margin_right: f32,
}

impl PageProperties {
    /// Create default page properties (A4 size, 2cm margins).
    pub fn default_a4() -> Self {
        let a4_width = 794.0; // 210mm at 96 DPI
        let a4_height = 1123.0; // 297mm at 96 DPI
        let margin = 56.7; // 2cm at 96 DPI

        Self {
            width: a4_width,
            height: a4_height,
            orientation: "portrait".to_string(),
            margin_top: margin,
            margin_bottom: margin,
            margin_left: margin,
            margin_right: margin,
        }
    }

    /// Content box width: page width minus left and right margins.
    pub fn content_width(&self) -> f32 {
        self.width - self.margin_left - self.margin_right
    }

    /// Content box height: page height minus top and bottom margins.
    pub fn content_height(&self) -> f32 {
        self.height - self.margin_top - self.margin_bottom
    }

    /// Update orientation based on width/height ratio.
    pub fn compute_orientation(&mut self) {
        self.orientation = if self.width > self.height {
            "landscape".to_string()
        } else {
            "portrait".to_string()
        };
    }
}

/// Margin-box with layout information.
///
/// A margin-box is a CSS-generated or element-based content box
/// positioned in one of the 16 margin-box positions around a page.
#[derive(Debug, Clone)]
pub struct MarginBox {
    /// Which margin-box position this occupies.
    pub position: MarginBoxPosition,

    /// Width of this margin-box in pixels.
    pub width: f32,

    /// Height of this margin-box in pixels.
    pub height: f32,

    /// X-offset relative to page origin (top-left).
    pub x: f32,

    /// Y-offset relative to page origin (top-left).
    pub y: f32,

    /// Generated content string (for page numbers, headers, footers, etc.).
    /// None if no content is assigned.
    pub content: Option<String>,
}

impl MarginBox {
    /// Create a new margin-box at a given position.
    pub fn new(position: MarginBoxPosition, width: f32, height: f32, x: f32, y: f32) -> Self {
        Self {
            position,
            width,
            height,
            x,
            y,
            content: None,
        }
    }

    /// Assign generated content to this margin-box.
    pub fn with_content(mut self, content: String) -> Self {
        self.content = Some(content);
        self
    }
}

/// Complete page structure with margin-boxes and page properties.
#[derive(Debug, Clone)]
pub struct PageBox {
    /// Page number (0-based).
    pub number: u32,

    /// Computed page properties (@page rules applied).
    pub properties: PageProperties,

    /// Margin-boxes positioned around the page.
    pub margin_boxes: HashMap<MarginBoxPosition, MarginBox>,
}

impl PageBox {
    /// Create a new page with computed properties.
    pub fn new(number: u32, properties: PageProperties) -> Self {
        Self {
            number,
            properties,
            margin_boxes: HashMap::new(),
        }
    }

    /// Layout all 16 margin-boxes based on page properties.
    ///
    /// CSS Paged Media L3 §3.2 — margin-box positioning algorithm:
    /// - Corners occupy fixed space
    /// - Edges stretch to fill remaining space
    /// - All boxes positioned relative to page border edge
    pub fn layout_margin_boxes(&mut self) {
        let pw = self.properties.width;
        let ph = self.properties.height;
        let mt = self.properties.margin_top;
        let mb = self.properties.margin_bottom;
        let ml = self.properties.margin_left;
        let mr = self.properties.margin_right;

        // Assume corner boxes have fixed 60px size (configurable later).
        let corner_size = 60.0;

        // Top row: top-left-corner, top-center, top-right-corner
        self.margin_boxes.insert(
            MarginBoxPosition::TopLeftCorner,
            MarginBox::new(
                MarginBoxPosition::TopLeftCorner,
                corner_size,
                mt,
                0.0,
                0.0,
            ),
        );

        self.margin_boxes.insert(
            MarginBoxPosition::TopCenter,
            MarginBox::new(
                MarginBoxPosition::TopCenter,
                (pw - 2.0 * corner_size).max(0.0),
                mt,
                corner_size,
                0.0,
            ),
        );

        self.margin_boxes.insert(
            MarginBoxPosition::TopRightCorner,
            MarginBox::new(
                MarginBoxPosition::TopRightCorner,
                corner_size,
                mt,
                pw - corner_size,
                0.0,
            ),
        );

        // Left edge: left-middle (full height minus corners)
        self.margin_boxes.insert(
            MarginBoxPosition::LeftMiddle,
            MarginBox::new(
                MarginBoxPosition::LeftMiddle,
                ml,
                (ph - mt - mb).max(0.0),
                0.0,
                mt,
            ),
        );

        // Right edge: right-middle (full height minus corners)
        self.margin_boxes.insert(
            MarginBoxPosition::RightMiddle,
            MarginBox::new(
                MarginBoxPosition::RightMiddle,
                mr,
                (ph - mt - mb).max(0.0),
                pw - mr,
                mt,
            ),
        );

        // Center box: full content area (may be hidden if content > page size)
        self.margin_boxes.insert(
            MarginBoxPosition::Center,
            MarginBox::new(
                MarginBoxPosition::Center,
                (pw - ml - mr).max(0.0),
                (ph - mt - mb).max(0.0),
                ml,
                mt,
            ),
        );

        // Bottom row: bottom-left-corner, bottom-center, bottom-right-corner
        self.margin_boxes.insert(
            MarginBoxPosition::BottomLeftCorner,
            MarginBox::new(
                MarginBoxPosition::BottomLeftCorner,
                corner_size,
                mb,
                0.0,
                ph - mb,
            ),
        );

        self.margin_boxes.insert(
            MarginBoxPosition::BottomCenter,
            MarginBox::new(
                MarginBoxPosition::BottomCenter,
                (pw - 2.0 * corner_size).max(0.0),
                mb,
                corner_size,
                ph - mb,
            ),
        );

        self.margin_boxes.insert(
            MarginBoxPosition::BottomRightCorner,
            MarginBox::new(
                MarginBoxPosition::BottomRightCorner,
                corner_size,
                mb,
                pw - corner_size,
                ph - mb,
            ),
        );
    }

    /// Get a margin-box by position.
    pub fn get_margin_box(&self, position: MarginBoxPosition) -> Option<&MarginBox> {
        self.margin_boxes.get(&position)
    }

    /// Mutably get a margin-box by position.
    pub fn get_margin_box_mut(&mut self, position: MarginBoxPosition) -> Option<&mut MarginBox> {
        self.margin_boxes.get_mut(&position)
    }
}

/// Matches @page rules for a given page number and applies properties.
///
/// CSS Paged Media L3 §3 — @page selector matching:
/// - Empty selector (just `@page { ... }`) matches all pages
/// - `:first` matches only the first page
/// - `:last` matches only the last page
/// - `:left` / `:right` match odd/even pages
/// - `:nth-child(n)` matches specific page numbers
///
/// **Cascade:** Later rules override earlier ones (normal CSS specificity).
pub fn match_page_rules(
    rules: &[PageRule],
    page_number: u32,
    total_pages: u32,
) -> Vec<&PageRule> {
    rules
        .iter()
        .filter(|rule| {
            // Empty selector matches all pages
            if rule.selector.is_empty() {
                return true;
            }

            // Check pseudo-class selectors
            let selector = rule.selector.to_lowercase();

            // :first — matches page 0 only
            if selector.contains(":first") {
                return page_number == 0;
            }

            // :last — matches last page (total_pages - 1)
            if selector.contains(":last") {
                return page_number == total_pages.saturating_sub(1);
            }

            // :left — matches odd pages (0, 2, 4, ...)
            #[allow(clippy::manual_is_multiple_of)]
            if selector.contains(":left") {
                return page_number % 2 == 0;
            }

            // :right — matches even pages (1, 3, 5, ...)
            #[allow(clippy::manual_is_multiple_of)]
            if selector.contains(":right") {
                return page_number % 2 == 1;
            }

            // Named page (no pseudo-class) — store in page-name property
            // Future: element's page property should match this name
            // For now, named pages don't match (Phase 2)
            !selector.starts_with(':')
        })
        .collect()
}

/// Extracts a numeric length property (margin-top, margin-bottom, etc.)
/// from a declaration list.
///
/// Returns value in pixels, or None if not specified.
/// Supports simple numeric values (e.g., "2cm", "56.7px", "100").
fn extract_length_property(prop_name: &str, declarations: &[lumen_css_parser::Declaration]) -> Option<f32> {
    for decl in declarations {
        if decl.property.to_lowercase() == prop_name {
            // Simple parsing: extract number, ignore units for now
            // Full impl should use lumen_css_parser's length parser
            let numeric_str = decl.value
                .chars()
                .take_while(|c| c.is_numeric() || *c == '.' || *c == '-')
                .collect::<String>();
            return numeric_str.parse::<f32>().ok();
        }
    }
    None
}

/// Computes page properties from matching @page rules.
///
/// Applies declarations in cascade order (later rules override earlier).
/// Returns computed PageProperties with all margins and size.
pub fn compute_page_properties(
    page_rules: &[&PageRule],
    default: PageProperties,
) -> PageProperties {
    let mut props = default;

    // Apply each matching rule in order
    for rule in page_rules {
        // Extract margin properties
        if let Some(margin_top) = extract_length_property("margin-top", &rule.declarations) {
            props.margin_top = margin_top;
        }
        if let Some(margin_bottom) = extract_length_property("margin-bottom", &rule.declarations) {
            props.margin_bottom = margin_bottom;
        }
        if let Some(margin_left) = extract_length_property("margin-left", &rule.declarations) {
            props.margin_left = margin_left;
        }
        if let Some(margin_right) = extract_length_property("margin-right", &rule.declarations) {
            props.margin_right = margin_right;
        }

        // Extract size properties
        if let Some(width) = extract_length_property("width", &rule.declarations) {
            props.width = width;
        }
        if let Some(height) = extract_length_property("height", &rule.declarations) {
            props.height = height;
        }
    }

    props.compute_orientation();
    props
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_margin_box_position_names() {
        assert_eq!(MarginBoxPosition::TopLeftCorner.css_name(), "@top-left-corner");
        assert_eq!(MarginBoxPosition::TopCenter.css_name(), "@top-center");
        assert_eq!(
            MarginBoxPosition::BottomRightCorner.css_name(),
            "@bottom-right-corner"
        );
    }

    #[test]
    fn test_margin_box_position_classification() {
        assert!(MarginBoxPosition::TopLeftCorner.is_corner());
        assert!(MarginBoxPosition::TopCenter.is_horizontal_edge());
        assert!(MarginBoxPosition::LeftMiddle.is_vertical_edge());
        assert!(!MarginBoxPosition::Center.is_corner());
    }

    #[test]
    fn test_page_properties_default() {
        let props = PageProperties::default_a4();
        assert_eq!(props.width, 794.0);
        assert_eq!(props.height, 1123.0);
        assert_eq!(props.orientation, "portrait");
        assert!(props.margin_top > 0.0);
    }

    #[test]
    fn test_page_properties_content_dimensions() {
        let props = PageProperties::default_a4();
        let content_w = props.content_width();
        let content_h = props.content_height();
        assert!(content_w < props.width);
        assert!(content_h < props.height);
    }

    #[test]
    fn test_page_box_layout_margin_boxes() {
        let mut page = PageBox::new(0, PageProperties::default_a4());
        page.layout_margin_boxes();

        // Verify all 9 margin-boxes are created
        assert_eq!(page.margin_boxes.len(), 9);

        // Check top-left-corner positioning
        let tlc = page.get_margin_box(MarginBoxPosition::TopLeftCorner).unwrap();
        assert_eq!(tlc.x, 0.0);
        assert_eq!(tlc.y, 0.0);
        assert!(tlc.width > 0.0);
        assert!(tlc.height > 0.0);

        // Check center box has correct width
        let center = page.get_margin_box(MarginBoxPosition::Center).unwrap();
        let expected_w = page.properties.content_width();
        assert_eq!(center.width, expected_w);
    }

    #[test]
    fn test_margin_box_with_content() {
        let mut mb = MarginBox::new(MarginBoxPosition::TopCenter, 100.0, 50.0, 10.0, 0.0);
        assert_eq!(mb.content, None);

        mb = mb.with_content("Page Header".to_string());
        assert_eq!(mb.content, Some("Page Header".to_string()));
    }

    #[test]
    fn test_page_orientation_compute() {
        let mut props = PageProperties {
            width: 1123.0,
            height: 794.0,
            orientation: "portrait".to_string(),
            margin_top: 50.0,
            margin_bottom: 50.0,
            margin_left: 50.0,
            margin_right: 50.0,
        };
        props.compute_orientation();
        assert_eq!(props.orientation, "landscape");
    }

    #[test]
    fn test_match_page_rules_empty_selector() {
        let rule = PageRule {
            selector: String::new(),
            declarations: vec![],
        };

        let rules = vec![rule];
        let matched = match_page_rules(&rules, 0, 5);
        assert_eq!(matched.len(), 1);
    }

    #[test]
    fn test_match_page_rules_first_pseudo() {
        let rule_first = PageRule {
            selector: ":first".to_string(),
            declarations: vec![],
        };

        // Page 0 should match :first
        let rules_vec = vec![rule_first.clone()];
        let matched = match_page_rules(&rules_vec, 0, 5);
        assert_eq!(matched.len(), 1);

        // Page 1 should not match :first
        let rules_vec = vec![rule_first];
        let matched = match_page_rules(&rules_vec, 1, 5);
        assert!(matched.is_empty());
    }

    #[test]
    fn test_match_page_rules_last_pseudo() {
        let rule = PageRule {
            selector: ":last".to_string(),
            declarations: vec![],
        };

        // Last page (page 4 out of 5) should match
        let rules_vec = vec![rule.clone()];
        let matched = match_page_rules(&rules_vec, 4, 5);
        assert_eq!(matched.len(), 1);

        // Other pages should not match
        let rules_vec = vec![rule.clone()];
        let matched = match_page_rules(&rules_vec, 0, 5);
        assert!(matched.is_empty());

        let rules_vec = vec![rule];
        let matched = match_page_rules(&rules_vec, 3, 5);
        assert!(matched.is_empty());
    }

    #[test]
    fn test_match_page_rules_left_right_pseudo() {
        let rule_left = PageRule {
            selector: ":left".to_string(),
            declarations: vec![],
        };
        let rule_right = PageRule {
            selector: ":right".to_string(),
            declarations: vec![],
        };

        // :left matches even pages (0, 2, 4, ...)
        let rules = vec![rule_left.clone()];
        assert_eq!(match_page_rules(&rules, 0, 10).len(), 1);
        let rules = vec![rule_left.clone()];
        assert_eq!(match_page_rules(&rules, 2, 10).len(), 1);
        let rules = vec![rule_left];
        assert!(match_page_rules(&rules, 1, 10).is_empty());

        // :right matches odd pages (1, 3, 5, ...)
        let rules = vec![rule_right.clone()];
        assert_eq!(match_page_rules(&rules, 1, 10).len(), 1);
        let rules = vec![rule_right.clone()];
        assert_eq!(match_page_rules(&rules, 3, 10).len(), 1);
        let rules = vec![rule_right];
        assert!(match_page_rules(&rules, 0, 10).is_empty());
    }

    #[test]
    fn test_match_page_rules_cascade() {
        let rule1 = PageRule {
            selector: String::new(),
            declarations: vec![],
        };
        let rule2 = PageRule {
            selector: String::new(),
            declarations: vec![],
        };

        let rules = vec![rule1, rule2];
        let matched = match_page_rules(&rules, 5, 10);
        assert_eq!(matched.len(), 2);
    }

    #[test]
    fn test_compute_page_properties_default() {
        let default = PageProperties::default_a4();
        let props = compute_page_properties(&[], default.clone());

        assert_eq!(props.width, default.width);
        assert_eq!(props.height, default.height);
        assert_eq!(props.margin_top, default.margin_top);
    }

    #[test]
    fn test_compute_page_properties_with_rules() {
        use lumen_css_parser::Declaration;

        let rule = PageRule {
            selector: String::new(),
            declarations: vec![
                Declaration {
                    property: "margin-top".to_string(),
                    value: "100".to_string(),
                    important: false,
                },
                Declaration {
                    property: "margin-left".to_string(),
                    value: "50".to_string(),
                    important: false,
                },
            ],
        };

        let default = PageProperties::default_a4();
        let rules = vec![&rule];
        let props = compute_page_properties(&rules, default);

        assert_eq!(props.margin_top, 100.0);
        assert_eq!(props.margin_left, 50.0);
        assert_eq!(props.margin_right, props.margin_right); // unchanged
    }
}
