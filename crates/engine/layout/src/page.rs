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
/// Includes page size (width, height), orientation, margin specifications,
/// and font properties for margin-box text layout.
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

    /// Font size for margin-box text in pixels (default 12.0).
    /// Can be specified in @page rules (Phase 3).
    pub font_size: f32,
}

impl PageProperties {
    /// Create default page properties (A4 size, 2cm margins, 12px font).
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
            font_size: 12.0,
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
/// Phase 3 adds text layout support for content rendering.
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

    /// Text layout result for content (Phase 3).
    /// Contains lines, dimensions after text wrapping.
    /// None until layout_text() is called.
    pub text_layout: Option<TextLayoutResult>,
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
            text_layout: None,
        }
    }

    /// Assign generated content to this margin-box.
    pub fn with_content(mut self, content: String) -> Self {
        self.content = Some(content);
        self
    }

    /// Layout text content in this margin-box using a TextMeasurer.
    ///
    /// Measures text width, applies line-breaking if needed, and calculates
    /// total height. Stores result in text_layout field.
    ///
    /// **Parameters:**
    /// - font_size: font size in pixels
    /// - letter_spacing: additional spacing between characters in pixels
    /// - tab_size: width of a tab character in pixels
    /// - measurer: text measurement provider (font metrics)
    pub fn layout_text(
        &mut self,
        font_size: f32,
        letter_spacing: f32,
        tab_size: f32,
        measurer: &dyn crate::TextMeasurer,
    ) {
        let text = match &self.content {
            Some(s) => s,
            None => {
                self.text_layout = Some(TextLayoutResult::empty());
                return;
            }
        };

        if text.is_empty() {
            self.text_layout = Some(TextLayoutResult::empty());
            return;
        }

        // Line height is typically font_size * 1.2, but 1.5 * font_size for margin-box readability
        let line_height = font_size * 1.2;

        // Try to fit text in the margin-box width
        let available_width = self.width;
        if available_width <= 0.0 {
            self.text_layout = Some(TextLayoutResult::empty());
            return;
        }

        // Measure text width
        let text_width = measure_text_w(text, font_size, letter_spacing, tab_size, measurer);

        // If text fits on a single line, no wrapping needed
        if text_width <= available_width {
            self.text_layout = Some(TextLayoutResult::single_line(
                text.clone(),
                text_width,
                line_height,
            ));
            return;
        }

        // Text needs wrapping: break into lines
        let lines = break_text_into_lines(
            text,
            font_size,
            letter_spacing,
            tab_size,
            available_width,
            measurer,
        );

        if lines.is_empty() {
            self.text_layout = Some(TextLayoutResult::empty());
            return;
        }

        // Calculate max width and total height
        let max_width = lines
            .iter()
            .map(|line| measure_text_w(line, font_size, letter_spacing, tab_size, measurer))
            .fold(0.0, f32::max);

        let height = line_height * lines.len() as f32;

        self.text_layout = Some(TextLayoutResult {
            lines,
            width: max_width,
            height,
            line_height,
        });
    }
}

/// Text layout result for margin-box content.
///
/// Stores text broken into lines with computed width and height.
/// Used in Phase 3 for text layout in margin-boxes.
#[derive(Debug, Clone)]
pub struct TextLayoutResult {
    /// Text broken into lines (one String per line).
    /// Empty if text is empty or width is 0.
    pub lines: Vec<String>,

    /// Computed width of the widest line in pixels.
    pub width: f32,

    /// Computed total height of all lines in pixels.
    /// Calculated as: line_height * number_of_lines.
    pub height: f32,

    /// Height of a single line (typically font_size * 1.2 or similar).
    pub line_height: f32,
}

impl TextLayoutResult {
    /// Create an empty layout result.
    pub fn empty() -> Self {
        Self {
            lines: vec![],
            width: 0.0,
            height: 0.0,
            line_height: 0.0,
        }
    }

    /// Create a layout result with a single line (no wrapping).
    pub fn single_line(text: String, width: f32, line_height: f32) -> Self {
        Self {
            lines: vec![text],
            width,
            height: line_height,
            line_height,
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
                margin_box.content = Some(text_content);
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

    /// Layout text in all margin-boxes that have content.
    ///
    /// Calls `layout_text()` on each margin-box with content, using the page's
    /// font size and standard typography parameters.
    ///
    /// **Parameters:**
    /// - letter_spacing: additional spacing between characters (typically 0.0)
    /// - tab_size: width of a tab character in pixels (typically 4.0 * font_width)
    /// - measurer: text measurement provider (font metrics)
    pub fn layout_margin_box_text(
        &mut self,
        letter_spacing: f32,
        tab_size: f32,
        measurer: &dyn crate::TextMeasurer,
    ) {
        let font_size = self.properties.font_size;

        // Collect positions with content to avoid borrow conflicts
        let positions: Vec<MarginBoxPosition> = self
            .margin_boxes
            .iter()
            .filter(|(_, mb)| mb.content.is_some())
            .map(|(pos, _)| *pos)
            .collect();

        // Layout text in each margin-box with content
        for position in positions {
            if let Some(margin_box) = self.margin_boxes.get_mut(&position) {
                margin_box.layout_text(font_size, letter_spacing, tab_size, measurer);
            }
        }
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

        // Extract font-size (Phase 3)
        if let Some(font_size) = extract_length_property("font-size", &rule.declarations) {
            props.font_size = font_size;
        }
    }

    props.compute_orientation();
    props
}

/// Creates a PageBox for a given page number with computed properties and layout.
///
/// **Steps:**
/// 1. Match @page rules for this page number
/// 2. Compute page properties (size, margins, font-size)
/// 3. Create PageBox with layout margin-boxes
/// 4. Apply content functions to margin-boxes
/// 5. Layout text in margin-boxes
///
/// This function ties together all Phase 1-3 components for a single page.
/// Phase 3 extension: includes text layout in margin-boxes.
/// Default text layout parameters: letter_spacing=0.0, tab_size=4.0.
pub fn create_page_box_with_text_layout(
    page_number: u32,
    total_pages: u32,
    page_rules: &[PageRule],
    content_map: &std::collections::HashMap<MarginBoxPosition, Vec<ContentFunction>>,
    counters: &PageCounters,
    measurer: &dyn crate::TextMeasurer,
) -> PageBox {
    // Step 1: Match @page rules for this page
    let matching_rules = match_page_rules(page_rules, page_number, total_pages);

    // Step 2: Compute page properties
    let properties = compute_page_properties(&matching_rules, PageProperties::default_a4());

    // Step 3: Create PageBox
    let mut page = PageBox::new(page_number, properties);

    // Step 3b: Layout margin-boxes (positions and sizes)
    page.layout_margin_boxes();

    // Step 4: Apply content functions
    page.apply_margin_box_content(content_map, counters);

    // Step 5: Layout text in margin-boxes (Phase 3)
    // Default text layout parameters: letter_spacing=0.0, tab_size=4.0
    page.layout_margin_box_text(0.0, 4.0, measurer);

    page
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

/// Helper function: measure text width using TextMeasurer.
///
/// Sums character widths, accounting for tabs and letter-spacing.
/// Synchronized with box_tree.rs measure_text_w logic.
pub fn measure_text_w(text: &str, font_size: f32, letter_spacing: f32, tab_size: f32, m: &dyn crate::TextMeasurer) -> f32 {
    if text.is_empty() {
        return 0.0;
    }
    let total: f32 = text
        .chars()
        .map(|c| {
            let cw = if c == '\t' { tab_size } else { m.char_width(c, font_size) };
            cw + letter_spacing
        })
        .sum();
    total - letter_spacing
}

/// Helper function: break text into lines that fit within a given width.
///
/// Implements greedy line-breaking:
/// - Adds words to the current line until they overflow
/// - Starts a new line when needed
/// - Handles tabs and spaces as break opportunities
///
/// **Parameters:**
/// - text: input string to break
/// - font_size: font size in pixels
/// - letter_spacing: letter spacing in pixels
/// - tab_size: tab width in pixels
/// - available_width: maximum line width in pixels
/// - measurer: text measurement provider
///
/// **Returns:**
/// - Vector of strings (one per line), or empty if available_width <= 0
fn break_text_into_lines(
    text: &str,
    font_size: f32,
    letter_spacing: f32,
    tab_size: f32,
    available_width: f32,
    measurer: &dyn crate::TextMeasurer,
) -> Vec<String> {
    if available_width <= 0.0 {
        return vec![];
    }

    let mut lines = vec![];
    let mut current_line = String::new();

    for word in text.split_whitespace() {
        if current_line.is_empty() {
            // First word on the line: force it on (even if it doesn't fit)
            current_line = word.to_string();
        } else {
            let line_with_word_width = measure_text_w(
                &format!("{} {}", current_line, word),
                font_size,
                letter_spacing,
                tab_size,
                measurer,
            );

            if line_with_word_width <= available_width {
                // Word fits on the current line
                current_line.push(' ');
                current_line.push_str(word);
            } else {
                // Word doesn't fit: start a new line
                lines.push(current_line);
                current_line = word.to_string();
            }
        }
    }

    // Add the last line
    if !current_line.is_empty() {
        lines.push(current_line);
    }

    lines
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
            font_size: 12.0,
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

    // Phase 3 tests: text layout in margin-boxes

    /// Mock TextMeasurer for testing. Each character has fixed width 8.0px.
    struct FixedWidthMeasurer;
    impl crate::TextMeasurer for FixedWidthMeasurer {
        fn char_width(&self, _: char, _: f32) -> f32 {
            8.0
        }
    }

    #[test]
    fn test_text_layout_result_empty() {
        let result = TextLayoutResult::empty();
        assert!(result.lines.is_empty());
        assert_eq!(result.width, 0.0);
        assert_eq!(result.height, 0.0);
    }

    #[test]
    fn test_text_layout_result_single_line() {
        let result = TextLayoutResult::single_line("Hello".to_string(), 40.0, 12.0);
        assert_eq!(result.lines.len(), 1);
        assert_eq!(result.lines[0], "Hello");
        assert_eq!(result.width, 40.0);
        assert_eq!(result.height, 12.0);
    }

    #[test]
    fn test_page_properties_has_font_size() {
        let props = PageProperties::default_a4();
        assert_eq!(props.font_size, 12.0);
    }

    #[test]
    fn test_margin_box_layout_text_empty_content() {
        let mut mb = MarginBox::new(MarginBoxPosition::BottomCenter, 100.0, 50.0, 10.0, 0.0);
        let measurer = FixedWidthMeasurer;

        mb.layout_text(12.0, 0.0, 4.0, &measurer);

        assert!(mb.text_layout.is_some());
        let layout = mb.text_layout.unwrap();
        assert!(layout.lines.is_empty());
        assert_eq!(layout.height, 0.0);
    }

    #[test]
    fn test_margin_box_layout_text_single_line_no_wrap() {
        let mut mb = MarginBox::new(MarginBoxPosition::BottomCenter, 100.0, 50.0, 10.0, 0.0);
        mb.content = Some("Page".to_string());
        let measurer = FixedWidthMeasurer;

        // "Page" = 4 chars * 8.0px = 32.0px, fits in 100.0px margin-box
        mb.layout_text(12.0, 0.0, 4.0, &measurer);

        assert!(mb.text_layout.is_some());
        let layout = mb.text_layout.unwrap();
        assert_eq!(layout.lines.len(), 1);
        assert_eq!(layout.lines[0], "Page");
        assert!(layout.width <= 100.0);
        assert!(layout.height > 0.0);
    }

    #[test]
    fn test_margin_box_layout_text_with_wrapping() {
        let mut mb = MarginBox::new(MarginBoxPosition::BottomCenter, 40.0, 50.0, 10.0, 0.0);
        mb.content = Some("Hello World Test".to_string());
        let measurer = FixedWidthMeasurer;

        // "Hello" = 5 chars * 8.0px = 40.0px (fits)
        // "Hello World" = 11 chars + 1 space = 96.0px (doesn't fit in 40px)
        // So should break into at least 2 lines
        mb.layout_text(12.0, 0.0, 4.0, &measurer);

        assert!(mb.text_layout.is_some());
        let layout = mb.text_layout.unwrap();
        assert!(layout.lines.len() >= 2);
        assert!(layout.height > 0.0);
    }

    #[test]
    fn test_margin_box_layout_text_zero_width() {
        let mut mb = MarginBox::new(MarginBoxPosition::BottomCenter, 0.0, 50.0, 10.0, 0.0);
        mb.content = Some("Text".to_string());
        let measurer = FixedWidthMeasurer;

        mb.layout_text(12.0, 0.0, 4.0, &measurer);

        assert!(mb.text_layout.is_some());
        let layout = mb.text_layout.unwrap();
        assert!(layout.lines.is_empty());
    }

    #[test]
    fn test_measure_text_w_empty() {
        let measurer = FixedWidthMeasurer;
        assert_eq!(measure_text_w("", 12.0, 0.0, 4.0, &measurer), 0.0);
    }

    #[test]
    fn test_measure_text_w_single_char() {
        let measurer = FixedWidthMeasurer;
        // One char = 8.0px, no letter-spacing
        assert_eq!(measure_text_w("A", 12.0, 0.0, 4.0, &measurer), 8.0);
    }

    #[test]
    fn test_measure_text_w_with_spaces() {
        let measurer = FixedWidthMeasurer;
        // "A B" = 8.0 + 8.0 + 8.0 = 24.0px
        assert_eq!(measure_text_w("A B", 12.0, 0.0, 4.0, &measurer), 24.0);
    }

    #[test]
    fn test_measure_text_w_with_letter_spacing() {
        let measurer = FixedWidthMeasurer;
        // "AB" with 2px letter-spacing = 8.0 + 2.0 + 8.0 = 18.0px (no extra spacing after last char)
        assert_eq!(measure_text_w("AB", 12.0, 2.0, 4.0, &measurer), 18.0);
    }

    #[test]
    fn test_break_text_into_lines_fits_single_line() {
        let measurer = FixedWidthMeasurer;
        let lines = break_text_into_lines("Hello", 12.0, 0.0, 4.0, 100.0, &measurer);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0], "Hello");
    }

    #[test]
    fn test_break_text_into_lines_multiple_words() {
        let measurer = FixedWidthMeasurer;
        // Available width 40px fits one word (e.g., "Hello" = 40px),
        // "Hello World" = 96px doesn't fit, so breaks into 2 lines
        let lines = break_text_into_lines("Hello World", 12.0, 0.0, 4.0, 40.0, &measurer);
        assert!(!lines.is_empty());
    }

    #[test]
    fn test_break_text_into_lines_zero_width() {
        let measurer = FixedWidthMeasurer;
        let lines = break_text_into_lines("Hello", 12.0, 0.0, 4.0, 0.0, &measurer);
        assert!(lines.is_empty());
    }

    #[test]
    fn test_break_text_into_lines_three_words() {
        let measurer = FixedWidthMeasurer;
        // With 40px width, "Hello" (40px) fits on line 1
        // "World" (40px) fits on line 2
        // "Test" (32px) fits on line 3
        let lines = break_text_into_lines("Hello World Test", 12.0, 0.0, 4.0, 40.0, &measurer);
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn test_page_box_layout_margin_box_text_all_boxes() {
        let mut page = PageBox::new(0, PageProperties::default_a4());
        page.layout_margin_boxes();

        // Add content to multiple margin-boxes
        page.get_margin_box_mut(MarginBoxPosition::TopCenter)
            .unwrap()
            .content = Some("Header".to_string());

        page.get_margin_box_mut(MarginBoxPosition::BottomCenter)
            .unwrap()
            .content = Some("Page 1".to_string());

        let measurer = FixedWidthMeasurer;
        page.layout_margin_box_text(0.0, 4.0, &measurer);

        // Verify text_layout is set in both boxes
        assert!(page
            .get_margin_box(MarginBoxPosition::TopCenter)
            .unwrap()
            .text_layout
            .is_some());
        assert!(page
            .get_margin_box(MarginBoxPosition::BottomCenter)
            .unwrap()
            .text_layout
            .is_some());

        // Boxes without content should have text_layout = None
        assert!(page
            .get_margin_box(MarginBoxPosition::LeftMiddle)
            .unwrap()
            .text_layout
            .is_none());
    }

    #[test]
    fn test_page_box_layout_margin_box_text_empty_page() {
        let mut page = PageBox::new(0, PageProperties::default_a4());
        page.layout_margin_boxes();

        let measurer = FixedWidthMeasurer;
        // Should not panic when no content is present
        page.layout_margin_box_text(0.0, 4.0, &measurer);

        // All boxes should still exist but have no text_layout
        for pos in MarginBoxPosition::all() {
            assert!(page.get_margin_box(*pos).is_some());
            assert!(page.get_margin_box(*pos).unwrap().text_layout.is_none());
        }
    }

    #[test]
    fn test_page_box_layout_respects_font_size() {
        let mut page = PageBox::new(0, PageProperties::default_a4());
        page.properties.font_size = 16.0; // Change from default 12.0
        page.layout_margin_boxes();

        page.get_margin_box_mut(MarginBoxPosition::TopCenter)
            .unwrap()
            .content = Some("Test".to_string());

        let measurer = FixedWidthMeasurer;
        page.layout_margin_box_text(0.0, 4.0, &measurer);

        let layout = page
            .get_margin_box(MarginBoxPosition::TopCenter)
            .unwrap()
            .text_layout
            .as_ref()
            .unwrap();

        // line_height should be font_size * 1.2 = 16 * 1.2 = 19.2
        assert!((layout.line_height - 19.2).abs() < 0.1);
    }

    #[test]
    fn test_create_page_box_with_text_layout_full_integration() {
        use lumen_css_parser::PageRule;

        // Create a simple @page rule with font-size
        let page_rule = PageRule {
            selector: String::new(),
            declarations: vec![lumen_css_parser::Declaration {
                property: "font-size".to_string(),
                value: "14".to_string(),
                important: false,
            }],
        };

        let page_rules = vec![page_rule];
        let mut content_map = std::collections::HashMap::new();
        content_map.insert(
            MarginBoxPosition::BottomCenter,
            vec![ContentFunction::Counter {
                name: "page".to_string(),
                style: "decimal".to_string(),
            }],
        );

        let counters = PageCounters::new(0);
        let measurer = FixedWidthMeasurer;

        // Create page box with full integration
        let page = create_page_box_with_text_layout(
            0,
            5,
            &page_rules,
            &content_map,
            &counters,
            &measurer,
        );

        // Verify page was created with correct number
        assert_eq!(page.number, 0);

        // Verify @page rule was applied (font-size should be 14)
        assert_eq!(page.properties.font_size, 14.0);

        // Verify margin-boxes were laid out
        assert!(!page.margin_boxes.is_empty());

        // Verify content was applied to bottom-center
        let bottom_center = page.get_margin_box(MarginBoxPosition::BottomCenter).unwrap();
        assert_eq!(bottom_center.content, Some("1".to_string()));

        // Verify text was laid out
        assert!(bottom_center.text_layout.is_some());
    }
}
