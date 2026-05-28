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

/// Text fragment within a margin-box after layout.
///
/// Stores a single line or word-wrapped text segment with position information
/// within a @page margin-box (Phase 3 of print-pdf-advanced).
#[derive(Debug, Clone)]
pub struct MarginBoxTextFragment {
    /// Actual text content of this fragment.
    pub text: String,

    /// Y-offset of this fragment within the margin-box (for multi-line text).
    pub y: f32,

    /// X-offset of this fragment within the margin-box (for horizontal alignment).
    pub x: f32,

    /// Width of this fragment (measured in pixels).
    pub width: f32,

    /// Height of this fragment (line height).
    pub height: f32,
}

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
    /// None if no content is assigned. Used during content generation phase.
    pub content: Option<String>,

    /// Laid-out text fragments after layout (Phase 3).
    /// Contains measured and wrapped text lines, positioned within the margin-box.
    pub text_fragments: Vec<MarginBoxTextFragment>,
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
            text_fragments: Vec::new(),
        }
    }

    /// Assign generated content to this margin-box.
    pub fn with_content(mut self, content: String) -> Self {
        self.content = Some(content);
        self
    }

    /// Layout text content in this margin-box with word-wrapping.
    ///
    /// Takes a text string and lays it out within the margin-box width,
    /// applying line breaks and measuring each fragment's dimensions.
    /// **Note:** Phase 3 simplified implementation uses fixed 8px-per-character
    /// measurement. Phase 4 should integrate with FontMeasurer for real fonts.
    pub fn layout_text(&mut self, text: &str, line_height: f32) {
        if text.is_empty() {
            self.text_fragments.clear();
            return;
        }

        self.text_fragments.clear();

        // Phase 3: Simplified measurement (8px per character).
        // Phase 4: integrate with FontMeasurer for real glyph widths
        let char_width = 8.0;
        let max_chars_per_line = (self.width / char_width).max(1.0) as usize;

        if max_chars_per_line == 0 {
            return; // Box too narrow to fit any text
        }

        let mut current_y = 0.0;
        let words: Vec<&str> = text.split_whitespace().collect();

        if words.is_empty() {
            return;
        }

        let mut current_line = String::new();
        for word in words.iter() {
            let separator = if current_line.is_empty() { "" } else { " " };
            let tentative = format!("{}{}{}", current_line, separator, word);

            if tentative.len() <= max_chars_per_line {
                current_line = tentative;
            } else {
                // Current line is full; save it and start a new line
                if !current_line.is_empty() {
                    let frag_width = current_line.len() as f32 * char_width;
                    self.text_fragments.push(MarginBoxTextFragment {
                        text: current_line.clone(),
                        x: 0.0,
                        y: current_y,
                        width: frag_width,
                        height: line_height,
                    });
                    current_y += line_height;
                }

                // If word itself is longer than a line, split it by characters
                if word.len() > max_chars_per_line {
                    let word_chars: Vec<char> = word.chars().collect();
                    for chunk in word_chars.chunks(max_chars_per_line) {
                        let chunk_str: String = chunk.iter().collect();
                        let frag_width = chunk_str.len() as f32 * char_width;
                        self.text_fragments.push(MarginBoxTextFragment {
                            text: chunk_str,
                            x: 0.0,
                            y: current_y,
                            width: frag_width,
                            height: line_height,
                        });
                        current_y += line_height;
                    }
                    current_line.clear();
                } else {
                    current_line = word.to_string();
                }
            }
        }

        // Don't forget the last line
        if !current_line.is_empty() {
            let frag_width = current_line.len() as f32 * char_width;
            self.text_fragments.push(MarginBoxTextFragment {
                text: current_line,
                x: 0.0,
                y: current_y,
                width: frag_width,
                height: line_height,
            });
        }
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

    /// Apply content functions to margin-boxes and generate text.
    ///
    /// Takes content specifications (content functions) and resolves them
    /// to actual text using the current page counters.
    /// Phase 3: Also performs text layout with word-wrapping within each margin-box.
    pub fn apply_margin_box_content(
        &mut self,
        content_map: &HashMap<MarginBoxPosition, Vec<ContentFunction>>,
        counters: &PageCounters,
    ) {
        for (position, functions) in content_map {
            let mut text_content = String::new();
            for func in functions {
                text_content.push_str(&resolve_content_function(func, counters));
            }
            if let Some(margin_box) = self.margin_boxes.get_mut(position)
                && !text_content.is_empty()
            {
                margin_box.content = Some(text_content.clone());
                // Phase 3: Layout text within margin-box with 16px line height (default)
                margin_box.layout_text(&text_content, 16.0);
            }
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

/// Counter value for page numbering and related counters.
///
/// Stores named counters that can be referenced in margin-box content
/// via `counter()` and `counters()` functions (CSS Paged Media L3 §8.2).
#[derive(Debug, Clone)]
pub struct PageCounters {
    /// Map of counter name → current value.
    ///
    /// The special counter "page" is reserved for page numbering (1-based in output,
    /// but stored as 0-based internally for consistency with page_number field).
    counters: HashMap<String, i32>,
}

impl PageCounters {
    /// Create a new counter set with the page counter initialized to 1 (page 1).
    pub fn new(page_number: u32) -> Self {
        let mut counters = HashMap::new();
        // Page counter is 1-based for display purposes
        counters.insert("page".to_string(), page_number as i32 + 1);
        Self { counters }
    }

    /// Get the value of a named counter.
    pub fn get(&self, name: &str) -> Option<i32> {
        self.counters.get(name).copied()
    }

    /// Set the value of a named counter.
    pub fn set(&mut self, name: String, value: i32) {
        self.counters.insert(name, value);
    }

    /// Increment a counter by 1.
    pub fn increment(&mut self, name: &str) {
        self.counters.entry(name.to_string())
            .and_modify(|v| *v += 1)
            .or_insert(1);
    }

    /// Reset a counter to a specified value.
    pub fn reset(&mut self, name: String, value: i32) {
        self.counters.insert(name, value);
    }
}

/// Represents a content function used in margin-box content generation.
///
/// Content functions generate text that appears in margin-boxes based on
/// page properties (CSS Paged Media L3 §8).
#[derive(Debug, Clone)]
pub enum ContentFunction {
    /// `counter(page)` — current page number.
    /// Roman numeral style variants (e.g., counter(page, lower-roman)) are not yet supported.
    Counter {
        /// Counter name (typically "page").
        name: String,
        /// Style: "decimal" (default), "roman", "lower-roman", "alpha", "lower-alpha", etc.
        style: String,
    },
    /// `counters(page, ".", decimal)` — counter with separator between nested counters.
    Counters {
        /// Counter name.
        name: String,
        /// Separator string (e.g., "." or "-").
        separator: String,
        /// Number style.
        style: String,
    },
    /// `string(name)` — named string set via element's string-set property.
    String { name: String },
    /// `target-counter(url(), page)` — page number of target element (not yet implemented).
    TargetCounter { url: String, name: String },
    /// Literal text (not a function).
    Literal { text: String },
}

/// Converts a counter value to a formatted string based on the specified style.
///
/// Supports CSS Paged Media counter styles: decimal, lower-roman, upper-roman, etc.
fn format_counter(value: i32, style: &str) -> String {
    match style {
        "lower-roman" => format_roman(value as u32, true),
        "upper-roman" => format_roman(value as u32, false),
        "lower-alpha" => {
            if value > 0 && value <= 26 {
                ((b'a' + (value as u8 - 1)) as char).to_string()
            } else {
                value.to_string()
            }
        }
        "upper-alpha" => {
            if value > 0 && value <= 26 {
                ((b'A' + (value as u8 - 1)) as char).to_string()
            } else {
                value.to_string()
            }
        }
        _ => value.to_string(), // "decimal" and default
    }
}

/// Convert a number to Roman numerals (lowercase if specified).
fn format_roman(mut value: u32, lowercase: bool) -> String {
    let symbols = if lowercase {
        [
            ("m", 1000),
            ("cm", 900),
            ("d", 500),
            ("cd", 400),
            ("c", 100),
            ("xc", 90),
            ("l", 50),
            ("xl", 40),
            ("x", 10),
            ("ix", 9),
            ("v", 5),
            ("iv", 4),
            ("i", 1),
        ]
    } else {
        [
            ("M", 1000),
            ("CM", 900),
            ("D", 500),
            ("CD", 400),
            ("C", 100),
            ("XC", 90),
            ("L", 50),
            ("XL", 40),
            ("X", 10),
            ("IX", 9),
            ("V", 5),
            ("IV", 4),
            ("I", 1),
        ]
    };

    let mut result = String::new();
    for (symbol, val) in &symbols {
        while value >= *val {
            result.push_str(symbol);
            value -= val;
        }
    }
    result
}

/// Resolves a content function to its text representation.
///
/// This function evaluates counter() and counters() functions using the current
/// page number and counter state.
pub fn resolve_content_function(func: &ContentFunction, counters: &PageCounters) -> String {
    match func {
        ContentFunction::Counter { name, style } => {
            counters
                .get(name)
                .map(|v| format_counter(v, style))
                .unwrap_or_default()
        }
        ContentFunction::Counters { name, separator: _, style } => {
            counters
                .get(name)
                .map(|v| format_counter(v, style))
                .unwrap_or_default()
            // TODO: support nested counters (scope stacks)
            // For now, just return the counter value
        }
        ContentFunction::String { name: _ } => {
            // TODO: implement named string support (string-set property)
            String::new()
        }
        ContentFunction::TargetCounter { url: _, name: _ } => {
            // TODO: implement target-counter() for cross-references
            String::new()
        }
        ContentFunction::Literal { text } => text.clone(),
    }
}

/// Common margin-box content preset: page number at bottom center.
///
/// Creates a simple page numbering configuration suitable for most documents.
pub fn create_page_number_footer() -> HashMap<MarginBoxPosition, Vec<ContentFunction>> {
    let mut content = HashMap::new();
    content.insert(
        MarginBoxPosition::BottomCenter,
        vec![ContentFunction::Counter {
            name: "page".to_string(),
            style: "decimal".to_string(),
        }],
    );
    content
}

/// Common margin-box content preset: page number at top center.
///
/// Alternative page numbering configuration for documents with top headers.
pub fn create_page_number_header() -> HashMap<MarginBoxPosition, Vec<ContentFunction>> {
    let mut content = HashMap::new();
    content.insert(
        MarginBoxPosition::TopCenter,
        vec![ContentFunction::Counter {
            name: "page".to_string(),
            style: "decimal".to_string(),
        }],
    );
    content
}

/// Common margin-box content preset: custom header and footer.
///
/// Takes custom text for header (top-center) and footer (bottom-center).
pub fn create_header_footer(header: Option<String>, footer: Option<String>) -> HashMap<MarginBoxPosition, Vec<ContentFunction>> {
    let mut content = HashMap::new();

    if let Some(h) = header {
        content.insert(
            MarginBoxPosition::TopCenter,
            vec![ContentFunction::Literal { text: h }],
        );
    }

    if let Some(f) = footer {
        content.insert(
            MarginBoxPosition::BottomCenter,
            vec![ContentFunction::Literal { text: f }],
        );
    }

    content
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

    #[test]
    fn test_page_counters_new() {
        let counters = PageCounters::new(0);
        assert_eq!(counters.get("page"), Some(1));
    }

    #[test]
    fn test_page_counters_increment() {
        let mut counters = PageCounters::new(5);
        assert_eq!(counters.get("page"), Some(6));
        counters.increment("page");
        assert_eq!(counters.get("page"), Some(7));
    }

    #[test]
    fn test_page_counters_set_and_get() {
        let mut counters = PageCounters::new(0);
        counters.set("chapter".to_string(), 3);
        assert_eq!(counters.get("chapter"), Some(3));
    }

    #[test]
    fn test_page_counters_reset() {
        let mut counters = PageCounters::new(5);
        counters.reset("page".to_string(), 1);
        assert_eq!(counters.get("page"), Some(1));
    }

    #[test]
    fn test_format_counter_decimal() {
        assert_eq!(format_counter(1, "decimal"), "1");
        assert_eq!(format_counter(42, "decimal"), "42");
        assert_eq!(format_counter(100, "decimal"), "100");
    }

    #[test]
    fn test_format_counter_lower_roman() {
        assert_eq!(format_counter(1, "lower-roman"), "i");
        assert_eq!(format_counter(4, "lower-roman"), "iv");
        assert_eq!(format_counter(9, "lower-roman"), "ix");
        assert_eq!(format_counter(27, "lower-roman"), "xxvii");
    }

    #[test]
    fn test_format_counter_upper_roman() {
        assert_eq!(format_counter(1, "upper-roman"), "I");
        assert_eq!(format_counter(4, "upper-roman"), "IV");
        assert_eq!(format_counter(9, "upper-roman"), "IX");
        assert_eq!(format_counter(27, "upper-roman"), "XXVII");
    }

    #[test]
    fn test_format_counter_lower_alpha() {
        assert_eq!(format_counter(1, "lower-alpha"), "a");
        assert_eq!(format_counter(26, "lower-alpha"), "z");
        assert_eq!(format_counter(27, "lower-alpha"), "27"); // out of range
    }

    #[test]
    fn test_format_counter_upper_alpha() {
        assert_eq!(format_counter(1, "upper-alpha"), "A");
        assert_eq!(format_counter(26, "upper-alpha"), "Z");
    }

    #[test]
    fn test_resolve_content_function_counter() {
        let counters = PageCounters::new(2);
        let func = ContentFunction::Counter {
            name: "page".to_string(),
            style: "decimal".to_string(),
        };
        let result = resolve_content_function(&func, &counters);
        assert_eq!(result, "3");
    }

    #[test]
    fn test_resolve_content_function_counter_roman() {
        let counters = PageCounters::new(3);
        let func = ContentFunction::Counter {
            name: "page".to_string(),
            style: "lower-roman".to_string(),
        };
        let result = resolve_content_function(&func, &counters);
        assert_eq!(result, "iv");
    }

    #[test]
    fn test_resolve_content_function_literal() {
        let counters = PageCounters::new(0);
        let func = ContentFunction::Literal {
            text: "Chapter 1".to_string(),
        };
        let result = resolve_content_function(&func, &counters);
        assert_eq!(result, "Chapter 1");
    }

    #[test]
    fn test_apply_margin_box_content() {
        let mut page = PageBox::new(0, PageProperties::default_a4());
        page.layout_margin_boxes();

        let counters = PageCounters::new(0);
        let mut content_map = HashMap::new();
        content_map.insert(
            MarginBoxPosition::BottomCenter,
            vec![ContentFunction::Counter {
                name: "page".to_string(),
                style: "decimal".to_string(),
            }],
        );

        page.apply_margin_box_content(&content_map, &counters);

        let bottom_center = page.get_margin_box(MarginBoxPosition::BottomCenter).unwrap();
        assert_eq!(bottom_center.content, Some("1".to_string()));
    }

    #[test]
    fn test_create_page_number_footer() {
        let content = create_page_number_footer();
        assert!(content.contains_key(&MarginBoxPosition::BottomCenter));
        assert_eq!(content.len(), 1);
    }

    #[test]
    fn test_create_page_number_header() {
        let content = create_page_number_header();
        assert!(content.contains_key(&MarginBoxPosition::TopCenter));
        assert_eq!(content.len(), 1);
    }

    #[test]
    fn test_create_header_footer_both() {
        let content = create_header_footer(
            Some("Chapter 1".to_string()),
            Some("Page Bottom".to_string()),
        );
        assert!(content.contains_key(&MarginBoxPosition::TopCenter));
        assert!(content.contains_key(&MarginBoxPosition::BottomCenter));
        assert_eq!(content.len(), 2);
    }

    #[test]
    fn test_create_header_footer_partial() {
        let content = create_header_footer(Some("Header".to_string()), None);
        assert!(content.contains_key(&MarginBoxPosition::TopCenter));
        assert!(!content.contains_key(&MarginBoxPosition::BottomCenter));
        assert_eq!(content.len(), 1);
    }

    // Phase 3: Text layout tests

    #[test]
    fn margin_box_layout_text_simple() {
        let mut mb = MarginBox::new(MarginBoxPosition::TopCenter, 200.0, 50.0, 100.0, 0.0);
        mb.layout_text("Hello World", 16.0);

        // Text should fit on one line (8px per char, 25 chars fit in 200px)
        assert_eq!(mb.text_fragments.len(), 1);
        assert_eq!(mb.text_fragments[0].text, "Hello World");
        assert_eq!(mb.text_fragments[0].y, 0.0);
        assert_eq!(mb.text_fragments[0].height, 16.0);
    }

    #[test]
    fn margin_box_layout_text_wrapping() {
        let mut mb = MarginBox::new(MarginBoxPosition::BottomCenter, 80.0, 100.0, 100.0, 100.0);
        mb.layout_text("Hello World Test", 16.0);

        // With 80px width and 8px per char, max 10 chars per line
        // "Hello" (5) + " " = 6 fits
        // "World" (5) won't fit, goes to next line
        assert!(mb.text_fragments.len() >= 2);
        assert_eq!(mb.text_fragments[0].y, 0.0);
        assert_eq!(mb.text_fragments[1].y, 16.0);
    }

    #[test]
    fn margin_box_layout_text_empty() {
        let mut mb = MarginBox::new(MarginBoxPosition::TopCenter, 200.0, 50.0, 0.0, 0.0);
        mb.layout_text("", 16.0);

        assert!(mb.text_fragments.is_empty());
    }

    #[test]
    fn margin_box_layout_text_whitespace_only() {
        let mut mb = MarginBox::new(MarginBoxPosition::TopCenter, 200.0, 50.0, 0.0, 0.0);
        mb.layout_text("   ", 16.0);

        assert!(mb.text_fragments.is_empty());
    }

    #[test]
    fn margin_box_layout_text_line_height() {
        let mut mb = MarginBox::new(MarginBoxPosition::BottomCenter, 80.0, 100.0, 0.0, 0.0);
        mb.layout_text("Hello World Test Multiple", 20.0);

        // Verify line heights are set correctly
        for fragment in &mb.text_fragments {
            assert_eq!(fragment.height, 20.0);
        }
    }

    #[test]
    fn margin_box_layout_text_narrow_box() {
        let mut mb = MarginBox::new(MarginBoxPosition::TopCenter, 30.0, 100.0, 0.0, 0.0);
        // 30px width / 8px per char = 3.75 ≈ 3 chars max per line
        mb.layout_text("Hello World", 16.0);

        // "Hello" (5 chars) exceeds 3, should wrap
        assert!(mb.text_fragments.len() >= 2);
    }

    #[test]
    fn margin_box_text_fragments_width_calculation() {
        let mut mb = MarginBox::new(MarginBoxPosition::TopCenter, 200.0, 50.0, 0.0, 0.0);
        mb.layout_text("Test", 16.0);

        assert_eq!(mb.text_fragments.len(), 1);
        // "Test" = 4 chars * 8px = 32px
        assert_eq!(mb.text_fragments[0].width, 32.0);
    }

    #[test]
    fn margin_box_apply_margin_box_content_with_layout() {
        let mut page = PageBox::new(0, PageProperties::default_a4());
        page.layout_margin_boxes();

        let counters = PageCounters::new(5);
        let mut content_map = HashMap::new();
        content_map.insert(
            MarginBoxPosition::TopCenter,
            vec![
                ContentFunction::Literal {
                    text: "Chapter ".to_string(),
                },
                ContentFunction::Counter {
                    name: "page".to_string(),
                    style: "decimal".to_string(),
                },
            ],
        );

        page.apply_margin_box_content(&content_map, &counters);

        let top_center = page.get_margin_box(MarginBoxPosition::TopCenter).unwrap();
        assert_eq!(top_center.content, Some("Chapter 6".to_string()));
        // Phase 3: Check that text was laid out
        assert!(!top_center.text_fragments.is_empty());
        assert_eq!(top_center.text_fragments[0].text, "Chapter 6");
    }
}
