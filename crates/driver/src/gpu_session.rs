//! GPU-enabled session for lumen-shell integration (ADR-006 Phase 4).
//!
//! `GpuSession` extends `BrowserSession` to support:
//! - GPU rendering (to DisplayList for wgpu)
//! - Scroll state management
//! - Streaming loads with intermediate rendering
//! - Window sizing

use lumen_core::geom::Size;
use lumen_core::error::Result;
use lumen_paint::DisplayList;
use lumen_layout::LayoutBox;
use std::sync::Arc;

use crate::BrowserSession;

/// Rendered page result from GpuSession rendering operations.
///
/// Contains all data needed for GPU upload and display in a window.
#[derive(Clone)]
pub struct RenderedPage {
    /// Display list for GPU rendering via wgpu.
    pub display_list: DisplayList,

    /// Page title (from `<title>` tag).
    pub title: Option<String>,

    /// Decoded images ready for GPU upload. Key is the raw `src` attribute value.
    pub images: Vec<(String, lumen_image::Image)>,

    /// Layout tree of the current page.
    pub layout_box: LayoutBox,

    /// Font provider for page-specific @font-face declarations.
    pub font_registry: Arc<dyn lumen_core::FontProvider>,

    /// Navigation request from JS (location.href=, etc) executed during load.
    pub js_navigate: Option<JsNavigateRequest>,
}

impl std::fmt::Debug for RenderedPage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderedPage")
            .field("title", &self.title)
            .field("images_count", &self.images.len())
            .field("js_navigate", &self.js_navigate)
            .finish_non_exhaustive()
    }
}

/// Navigation request initiated by JS code (location.href=, history.pushState, etc).
#[derive(Debug, Clone)]
pub struct JsNavigateRequest {
    /// Target URL
    pub url: String,
    /// Whether to replace history entry instead of pushing
    pub replace: bool,
}

/// Extended `BrowserSession` trait for GPU and streaming operations.
///
/// Bridges WinitSession capabilities with lumen-shell's pipeline requirements.
/// Implements GPU rendering, scroll state management, and streaming loads.
pub trait GpuSession: BrowserSession {
    /// Render current page to display list and return all GPU-related data.
    ///
    /// This is called after layout/paint to prepare rendering for GPU upload.
    /// Typically called after `navigate()` or `click()` when layout changes.
    fn render_to_gpu(&mut self) -> Result<RenderedPage>;

    /// Update scroll position and trigger relayout if needed.
    ///
    /// Returns true if layout was invalidated and needs re-rendering.
    fn set_scroll(&mut self, delta: crate::ScrollDelta) -> Result<bool>;

    /// Get current scroll position in logical pixels.
    fn scroll_position(&self) -> (f32, f32);

    /// Get current viewport size in logical pixels.
    fn viewport_size(&self) -> Size;

    /// Update viewport size (called on window resize).
    ///
    /// May trigger relayout and re-rendering.
    fn set_viewport(&mut self, width: f32, height: f32) -> Result<bool>;

    /// Load page from URL with streaming support.
    ///
    /// Calls `on_chunk` callback for each intermediate rendered frame during loading.
    /// This allows progressive rendering while HTML is still being parsed.
    fn navigate_streaming<F>(&mut self, url: &str, on_chunk: F) -> Result<()>
    where
        F: FnMut(RenderedPage);
}
