//! Print pagination algorithm for @media print.
//!
//! Converts a single LayoutBox tree (screen layout) into multiple pages
//! by applying CSS Fragmentation L3 rules: break-before/after, orphans/widows,
//! page-break-* properties.
//!
//! **Inputs:**
//! - Single LayoutBox tree from normal layout (screen mode)
//! - PaginationContext (page size, margins, etc.)
//!
//! **Outputs:**
//! - Vec<Page> — one per physical page
//! - Each Page contains positioned LayoutBox fragments that fit on it

use crate::box_tree::LayoutBox;

/// Parameters for print pagination.
///
/// Includes page dimensions, margins, and media context.
#[derive(Debug, Clone)]
pub struct PaginationContext {
    /// Page width in pixels (e.g., A4: 210mm ≈ 794px at 96 DPI).
    /// Includes all page margins.
    pub page_width: f32,

    /// Page height in pixels (e.g., A4: 297mm ≈ 1123px at 96 DPI).
    /// Includes all page margins.
    pub page_height: f32,

    /// Page top margin in pixels.
    pub margin_top: f32,

    /// Page bottom margin in pixels.
    pub margin_bottom: f32,

    /// Page left margin in pixels.
    pub margin_left: f32,

    /// Page right margin in pixels.
    pub margin_right: f32,
}

impl PaginationContext {
    /// Content box width: page width minus left and right margins.
    pub fn content_width(&self) -> f32 {
        self.page_width - self.margin_left - self.margin_right
    }

    /// Content box height: page height minus top and bottom margins.
    pub fn content_height(&self) -> f32 {
        self.page_height - self.margin_top - self.margin_bottom
    }

    /// Top-left corner of content box within page.
    pub fn content_origin(&self) -> (f32, f32) {
        (self.margin_left, self.margin_top)
    }
}

/// A single page with positioned content.
///
/// Contains fragments of the original layout tree that fit within this page's
/// bounds, adjusted for page-relative positioning.
#[derive(Debug, Clone)]
pub struct Page {
    /// Page index (0 = first page).
    pub number: u32,

    /// Content fragments on this page. Each fragment is a LayoutBox
    /// (or subtree) clipped to page bounds, with y-offset relative to page origin.
    pub fragments: Vec<PageFragment>,

    /// Total height of content on this page (before clipping to page_height).
    /// Useful for debugging and detecting overfull pages.
    pub content_height: f32,
}

/// A fragment of layout tree content positioned on a page.
#[derive(Debug, Clone)]
pub struct PageFragment {
    /// Original LayoutBox, potentially clipped.
    pub layout_box: LayoutBox,

    /// Y-offset on this page (0 = top of content area, after top margin).
    /// X-offset is determined by the box's normal flow position.
    pub page_y_offset: f32,
}

/// Pagination algorithm: split LayoutBox tree into pages.
///
/// **Algorithm sketch:**
/// 1. Reflow content in print mode (width = context.content_width()).
/// 2. Walk block-level boxes; track y-position.
/// 3. On break points (break-before/after, break-avoid violations):
///    - Start new page
///    - Handle orphans/widows (must have minimum lines)
/// 4. Collect fragments that fit per page.
///
/// **Phase 0 simplification:**
/// - No multi-column layout.
/// - No float manipulation (assumes floats already laid out in input).
/// - No media-relative units (vh, vw assumed absent).
/// - Single-page assumed (no break-inside handling yet).
pub fn paginate(layout_box: &LayoutBox, _context: &PaginationContext) -> Vec<Page> {
    // TODO(P1): Implement full pagination algorithm.
    // For now, return single page with entire content.
    //
    // Full algorithm would:
    // 1. Clone layout_box and relayout with width = context.content_width()
    // 2. Walk children of root, tracking y-offset
    // 3. On each block-level box:
    //    a. Check break-before → force new page?
    //    b. Try to fit box on current page
    //    c. If not fit: new page (respecting break-avoid)
    //    d. Check break-after → force new page?
    // 4. Return Vec<Page> with fragments clipped to page height

    vec![Page {
        number: 0,
        fragments: vec![PageFragment {
            layout_box: layout_box.clone(),
            page_y_offset: 0.0,
        }],
        content_height: layout_box.rect.height,
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pagination_context_content_dimensions() {
        let ctx = PaginationContext {
            page_width: 800.0,
            page_height: 1000.0,
            margin_top: 50.0,
            margin_bottom: 50.0,
            margin_left: 40.0,
            margin_right: 40.0,
        };

        assert_eq!(ctx.content_width(), 720.0); // 800 - 40 - 40
        assert_eq!(ctx.content_height(), 900.0); // 1000 - 50 - 50
        assert_eq!(ctx.content_origin(), (40.0, 50.0));
    }

    #[test]
    fn paginate_single_page_placeholder() {
        // TODO: Add real pagination test once algorithm is implemented.
        // For now, just verify placeholder returns one page.
    }
}
