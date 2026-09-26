//! CSS Ruby (Annotations) L3 — East Asian character annotations.
//!
//! Ruby is a small annotation placed alongside base text. Typical use:
//! - Japanese furigana (phonetic guide for kanji)
//! - Chinese pinyin annotations
//! - Korean ruby text
//!
//! Structure: `<ruby>base text<rt>annotation</rt></ruby>`
//!
//! `ruby-align`, `ruby-merge` and `ruby-position` are parsed into
//! `ComputedStyle`. The box builder (`box_tree/build.rs`'s `build_ruby_box`)
//! splits a `<ruby>` into segments — bases plus annotation levels (`<rtc>`,
//! or consecutive `<rt>`) — recorded as a [`RubyShape`]; `layout_dispatch`
//! composes them with [`lay_out_ruby_segments`]. A single level goes through
//! [`lay_out_ruby`]; several levels (or `inter-character`) through the
//! multi-level composer (GAP-RUBYBOX-2).

use crate::box_tree::{LayoutBox, BoxKind, BoxOrigin, BoxRole};
use crate::style::ComputedStyle;
use lumen_dom::NodeId;
use lumen_core::geom::Rect;

/// CSS Ruby L1 §3.4 — `ruby-position`. Inherited. Initial: `alternate`
/// (= `alternate over`).
///
/// Applies to ruby annotation containers (`<rtc>`, or the anonymous one
/// wrapping consecutive `<rt>`). `alternate` resolves per annotation level —
/// see [`resolve_level_sides`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RubyPosition {
    /// Annotation above the base text (standard for horizontal writing-mode).
    Over,
    /// Annotation below the base text.
    Under,
    /// `alternate` / `alternate over`: the first annotation level goes over,
    /// each following level on the side opposite to the previous one.
    #[default]
    AlternateOver,
    /// `alternate under`: as [`Self::AlternateOver`], starting under.
    AlternateUnder,
    /// `inter-character`: annotation on the inline-end side of its base.
    /// Horizontal writing mode only; the annotation keeps its own
    /// (horizontal) glyph orientation — the spec's forced `vertical-rl`
    /// annotation writing mode is not implemented.
    InterCharacter,
}

impl RubyPosition {
    /// Parses the `ruby-position` grammar
    /// `[ alternate || [ over | under ] ] | inter-character`.
    pub fn parse(val: &str) -> Option<Self> {
        let tokens: Vec<String> =
            val.split_ascii_whitespace().map(str::to_ascii_lowercase).collect();
        let tokens: Vec<&str> = tokens.iter().map(String::as_str).collect();
        Some(match tokens.as_slice() {
            ["over"] => Self::Over,
            ["under"] => Self::Under,
            ["inter-character"] => Self::InterCharacter,
            ["alternate"] | ["alternate", "over"] | ["over", "alternate"] => Self::AlternateOver,
            ["alternate", "under"] | ["under", "alternate"] => Self::AlternateUnder,
            _ => return None,
        })
    }

    /// Shortest serialization of the computed value (CSSOM).
    pub fn as_css(self) -> &'static str {
        match self {
            Self::Over => "over",
            Self::Under => "under",
            Self::AlternateOver => "alternate",
            Self::AlternateUnder => "alternate under",
            Self::InterCharacter => "inter-character",
        }
    }
}

/// Resolved placement of one annotation level relative to its bases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RubySide {
    /// Above the base row.
    Over,
    /// Below the base row.
    Under,
    /// On the inline-end side of each base (`inter-character`).
    InterCharacter,
}

/// CSS Ruby L1 §3.4: resolves the `ruby-position` of each annotation level of
/// one ruby segment (in document order) to a side. `alternate` puts the first
/// level on its keyword's side and every later level opposite to the level
/// before it — whatever that level's own value was (WPT
/// `css-ruby/ruby-position-alternate.html`: `over`, then two inherited
/// `alternate` levels → over, under, over).
pub fn resolve_level_sides(positions: &[RubyPosition]) -> Vec<RubySide> {
    let mut sides: Vec<RubySide> = Vec::with_capacity(positions.len());
    for &pos in positions {
        let prev = sides.last().copied();
        let side = match pos {
            RubyPosition::Over => RubySide::Over,
            RubyPosition::Under => RubySide::Under,
            RubyPosition::InterCharacter => RubySide::InterCharacter,
            RubyPosition::AlternateOver | RubyPosition::AlternateUnder => match prev {
                Some(RubySide::Over) => RubySide::Under,
                Some(RubySide::Under) => RubySide::Over,
                _ if pos == RubyPosition::AlternateUnder => RubySide::Under,
                _ => RubySide::Over,
            },
        };
        sides.push(side);
    }
    sides
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

    // A lone level resolves `alternate` to its own keyword's side.
    let side = match ruby.position {
        RubyPosition::Under | RubyPosition::AlternateUnder => RubySide::Under,
        RubyPosition::InterCharacter => {
            let level = RubyLevel {
                annotations: ruby.ruby_text_boxes.clone(),
                position: ruby.position,
            };
            return compose_levels(&ruby.base_boxes, &[level], ruby.align, ruby.merge).0;
        }
        RubyPosition::Over | RubyPosition::AlternateOver => RubySide::Over,
    };

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
                    side,
                    ruby.align,
                )
            })
            .collect();
        return stack_boxes_horizontal(&columns);
    }

    compose_column(&ruby.base_boxes, &ruby.ruby_text_boxes, side, ruby.align)
}

/// Compose one ruby column: a base row and an annotation row stacked per
/// `side` (`Over`/`Under`), with the narrower row distributed per `align`.
fn compose_column(
    bases: &[LayoutBox],
    annotations: &[LayoutBox],
    side: RubySide,
    align: RubyAlign,
) -> LayoutBox {
    let mut base_row = stack_boxes_horizontal(bases);
    let mut ruby_row = stack_boxes_horizontal(annotations);
    // Reset both rows to x=0 as whole subtrees — `stack_boxes_horizontal`'s
    // input boxes may carry any absolute x from wherever they were laid out
    // (`build_ruby_group_box` always starts them at 0, but this function has
    // no way to assume that of a future caller), and every descendant inside
    // each row (e.g. an `InlineRun` several levels down) must move with it —
    // GAP-RUBYBOX found this the hard way: a plain `rect.x = 0.0` here left
    // nested text at its stale absolute position while the container row
    // reported x=0, corrupting paint for anything but a single flat child.
    shift_subtree_to_x(&mut base_row, 0.0);
    shift_subtree_to_x(&mut ruby_row, 0.0);

    let col_width = base_row.rect.width.max(ruby_row.rect.width);
    align_row(&mut base_row, bases.len(), col_width, align);
    align_row(&mut ruby_row, annotations.len(), col_width, align);

    let base_height = base_row.rect.height;
    let ruby_height = ruby_row.rect.height;

    let mut column = make_anonymous_box_with_style(base_row.style.clone());
    column.rect.width = col_width;
    column.rect.height = base_height + ruby_height;

    match side {
        RubySide::Over | RubySide::InterCharacter => {
            crate::incremental::translate_subtree(&mut base_row, 0.0, ruby_height);
            column.children.push(ruby_row);
            column.children.push(base_row);
        }
        RubySide::Under => {
            crate::incremental::translate_subtree(&mut ruby_row, 0.0, base_height);
            column.children.push(base_row);
            column.children.push(ruby_row);
        }
    }

    column
}

/// CSSOM geometry of a laid-out `BoxKind::Ruby` box: the union of its base
/// groups (the anonymous blocks the `<ruby>` owns), without the annotations.
/// Browsers report the ruby container's base-level box and let annotations
/// overflow it (WPT `css-ruby/ruby-position-alternate.html` compares
/// `<rt>` rects against it); the layout rect keeps the annotations so the
/// line reserves room for them. `None` when the ruby has no base.
pub(crate) fn ruby_base_rect(ruby: &LayoutBox) -> Option<Rect> {
    let mut acc: Option<Rect> = None;
    let mut stack: Vec<&LayoutBox> = ruby.children.iter().collect();
    while let Some(b) = stack.pop() {
        if b.origin.role == BoxRole::AnonymousBlock && b.origin.node == Some(ruby.node) {
            let r = b.rect;
            acc = Some(match acc {
                None => r,
                Some(a) => {
                    let x = a.x.min(r.x);
                    let y = a.y.min(r.y);
                    let right = (a.x + a.width).max(r.x + r.width);
                    let bottom = (a.y + a.height).max(r.y + r.height);
                    Rect { x, y, width: right - x, height: bottom - y }
                }
            });
            continue;
        }
        stack.extend(b.children.iter());
    }
    acc
}

/// Horizontal gap between adjacent bases / annotation boxes — the same
/// constant `stack_boxes_horizontal` inserts.
const RUBY_BOX_GAP: f32 = 2.0;

/// Box-tree shape of a `<ruby>` element (GAP-RUBYBOX-2), recorded by
/// `build.rs`'s `build_ruby_box` in [`BoxKind::Ruby`]. The `<ruby>` box's
/// `children` are the group boxes flattened in this exact order: for each
/// segment its `bases` base boxes, then each level's `annotations` boxes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RubyShape {
    /// Ruby segments in document order (CSS Ruby L1 §2.2).
    pub segments: Vec<RubySegmentShape>,
}

/// One ruby segment: a run of bases and the annotation levels over them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RubySegmentShape {
    /// Number of base boxes (one per `<rb>`, or per run of loose base content).
    pub bases: usize,
    /// Annotation levels in document order.
    pub levels: Vec<RubyLevelShape>,
}

/// One annotation level (an `<rtc>`, or consecutive bare `<rt>`s).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RubyLevelShape {
    /// Number of annotation boxes in the level.
    pub annotations: usize,
    /// Computed `ruby-position` of the level's annotation container.
    pub position: RubyPosition,
}

/// A laid-out annotation level handed to [`lay_out_ruby_segments`].
#[derive(Debug, Clone)]
pub struct RubyLevel {
    /// Annotation boxes, index-paired with the segment's bases when the
    /// counts match (and `ruby-merge` allows pairing), otherwise one span.
    pub annotations: Vec<LayoutBox>,
    /// Computed `ruby-position` of the level's annotation container.
    pub position: RubyPosition,
}

/// A laid-out ruby segment handed to [`lay_out_ruby_segments`].
#[derive(Debug, Clone)]
pub struct RubySegment {
    /// Base boxes.
    pub bases: Vec<LayoutBox>,
    /// Annotation levels in document order.
    pub levels: Vec<RubyLevel>,
}

/// Composes every segment of a `<ruby>` and joins them side by side with
/// their base rows on one line (a segment with an `over` level must not push
/// its neighbours' bases down). One segment returns its own composed box, so
/// the single-segment layout is exactly [`lay_out_ruby`]'s.
pub fn lay_out_ruby_segments(
    segments: Vec<RubySegment>,
    style: std::sync::Arc<ComputedStyle>,
    align: RubyAlign,
    merge: RubyMerge,
) -> LayoutBox {
    let mut composed: Vec<(LayoutBox, f32)> = segments
        .into_iter()
        .filter(|seg| {
            !seg.bases.is_empty() || seg.levels.iter().any(|l| !l.annotations.is_empty())
        })
        .map(|seg| compose_segment(seg, align, merge))
        .collect();
    match composed.len() {
        0 => return make_anonymous_box_with_style(style),
        1 => return composed.remove(0).0,
        _ => {}
    }
    let base_top = composed.iter().map(|(_, top)| *top).fold(0.0, f32::max);
    let boxes: Vec<LayoutBox> = composed
        .into_iter()
        .map(|(mut b, top)| {
            crate::incremental::translate_subtree(&mut b, 0.0, base_top - top);
            b
        })
        .collect();
    let mut row = stack_boxes_horizontal(&boxes);
    let top = row.rect.y;
    row.rect.height = row
        .children
        .iter()
        .map(|c| c.rect.y - top + c.rect.height)
        .fold(0.0, f32::max);
    row
}

/// Composes one segment; returns the box and the y of its base row inside it.
fn compose_segment(seg: RubySegment, align: RubyAlign, merge: RubyMerge) -> (LayoutBox, f32) {
    let RubySegment { bases, mut levels } = seg;
    levels.retain(|l| !l.annotations.is_empty());
    if bases.is_empty() {
        let annotations: Vec<LayoutBox> =
            levels.into_iter().flat_map(|l| l.annotations).collect();
        return (stack_boxes_horizontal(&annotations), 0.0);
    }
    if levels.is_empty() {
        return (stack_boxes_horizontal(&bases), 0.0);
    }
    if levels.len() == 1 && levels[0].position != RubyPosition::InterCharacter {
        let level = levels.remove(0);
        let over = matches!(level.position, RubyPosition::Over | RubyPosition::AlternateOver);
        let base_top = if over {
            level.annotations.iter().map(|a| a.rect.height).fold(0.0, f32::max)
        } else {
            0.0
        };
        let ruby = RubyBox::new(bases, level.annotations)
            .with_position(level.position)
            .with_align(align)
            .with_merge(merge);
        return (lay_out_ruby(&ruby), base_top);
    }
    compose_levels(&bases, &levels, align, merge)
}

/// Offset of a single box of `width` inside `container` per `ruby-align`.
fn align_offset(container: f32, width: f32, align: RubyAlign) -> f32 {
    let slack = (container - width).max(0.0);
    match align {
        RubyAlign::Start | RubyAlign::SpaceBetween => 0.0,
        RubyAlign::Center | RubyAlign::SpaceAround => slack / 2.0,
    }
}

/// Moves `b`'s subtree so its rect starts at (`x`, `y`).
fn place_at(b: &mut LayoutBox, x: f32, y: f32) {
    let (dx, dy) = (x - b.rect.x, y - b.rect.y);
    crate::incremental::translate_subtree(b, dx, dy);
}

/// Multi-level composer (CSS Ruby L1 §3.4, GAP-RUBYBOX-2). Levels resolve to
/// sides via [`resolve_level_sides`]; `over` levels stack upward from the base
/// row in document order (first level nearest the base), `under` levels
/// downward. A level whose annotation count equals the base count pairs one
/// annotation per base column (unless `ruby-merge: merge`); otherwise it
/// spans the whole segment as one row. `inter-character` annotations sit on
/// the inline-end side of their base, top-aligned with it. Returns the
/// segment box (children flat: bases, then annotations level by level) and
/// the y of its base row.
fn compose_levels(
    bases: &[LayoutBox],
    levels: &[RubyLevel],
    align: RubyAlign,
    merge: RubyMerge,
) -> (LayoutBox, f32) {
    let n = bases.len();
    let positions: Vec<RubyPosition> = levels.iter().map(|l| l.position).collect();
    let sides = resolve_level_sides(&positions);
    let paired: Vec<bool> = levels
        .iter()
        .map(|l| l.annotations.len() == n && !(merge == RubyMerge::Merge && n > 1))
        .collect();
    let inter_paired = |l: usize| paired[l] && sides[l] == RubySide::InterCharacter;

    // Spanning levels become one row each, normalised to (0,0).
    let rows: Vec<Option<LayoutBox>> = levels
        .iter()
        .zip(&paired)
        .map(|(l, &p)| {
            (!p).then(|| {
                let mut row = stack_boxes_horizontal(&l.annotations);
                place_at(&mut row, 0.0, 0.0);
                row
            })
        })
        .collect();

    // Inline extent of base `i` plus its paired inter-character annotations.
    let inline_w = |i: usize| -> f32 {
        bases[i].rect.width
            + (0..levels.len())
                .filter(|&l| inter_paired(l))
                .map(|l| levels[l].annotations[i].rect.width)
                .sum::<f32>()
    };
    // Column width: that inline extent, or the widest paired over/under annotation.
    let col_w: Vec<f32> = (0..n)
        .map(|i| {
            (0..levels.len())
                .filter(|&l| paired[l] && sides[l] != RubySide::InterCharacter)
                .map(|l| levels[l].annotations[i].rect.width)
                .fold(inline_w(i), f32::max)
        })
        .collect();
    let mut col_x = Vec::with_capacity(n);
    let mut cursor = 0.0;
    for (i, w) in col_w.iter().enumerate() {
        if i > 0 {
            cursor += RUBY_BOX_GAP;
        }
        col_x.push(cursor);
        cursor += w;
    }
    let block_w = cursor;
    let span_w = rows
        .iter()
        .zip(&sides)
        .filter(|(_, s)| **s != RubySide::InterCharacter)
        .filter_map(|(r, _)| r.as_ref().map(|r| r.rect.width))
        .fold(0.0, f32::max);
    let core_w = block_w.max(span_w);
    let block_dx = align_offset(core_w, block_w, align);

    // Vertical extents.
    let base_h = bases.iter().map(|b| b.rect.height).fold(0.0, f32::max);
    let level_h: Vec<f32> = levels
        .iter()
        .zip(&rows)
        .map(|(l, r)| match r {
            Some(r) => r.rect.height,
            None => l.annotations.iter().map(|a| a.rect.height).fold(0.0, f32::max),
        })
        .collect();
    let base_y: f32 = (0..levels.len())
        .filter(|&l| sides[l] == RubySide::Over)
        .map(|l| level_h[l])
        .sum();

    let mut segment = make_anonymous_box_with_style(bases[0].style.clone());
    // Per column: x where the next inter-character annotation starts.
    let mut inter_x = Vec::with_capacity(n);
    for (i, base) in bases.iter().enumerate() {
        let x = block_dx + col_x[i] + align_offset(col_w[i], inline_w(i), align);
        let mut b = base.clone();
        place_at(&mut b, x, base_y);
        inter_x.push(x + base.rect.width);
        segment.children.push(b);
    }

    let mut over_cursor = base_y;
    let mut under_cursor = base_y + base_h;
    let mut inter_span_x = core_w;
    for (l, level) in levels.iter().enumerate() {
        let y = match sides[l] {
            RubySide::Over => {
                over_cursor -= level_h[l];
                over_cursor
            }
            RubySide::Under => {
                let y = under_cursor;
                under_cursor += level_h[l];
                y
            }
            RubySide::InterCharacter => base_y,
        };
        match &rows[l] {
            None => {
                for (i, annotation) in level.annotations.iter().enumerate() {
                    let x = if sides[l] == RubySide::InterCharacter {
                        let x = inter_x[i];
                        inter_x[i] += annotation.rect.width;
                        x
                    } else {
                        block_dx + col_x[i] + align_offset(col_w[i], annotation.rect.width, align)
                    };
                    let mut a = annotation.clone();
                    place_at(&mut a, x, y);
                    segment.children.push(a);
                }
            }
            Some(row) => {
                let mut r = row.clone();
                if sides[l] == RubySide::InterCharacter {
                    place_at(&mut r, inter_span_x, y);
                    inter_span_x += r.rect.width;
                } else {
                    align_row(&mut r, level.annotations.len(), core_w, align);
                    crate::incremental::translate_subtree(&mut r, 0.0, y);
                }
                segment.children.push(r);
            }
        }
    }

    segment.rect.width = inter_span_x;
    segment.rect.height = segment
        .children
        .iter()
        .map(|c| c.rect.y + c.rect.height)
        .fold(base_y + base_h, f32::max);
    (segment, base_y)
}

/// Moves `b`'s whole subtree so `b.rect.x` becomes exactly `target_x`,
/// preserving every descendant's offset from it (see `compose_column`'s doc
/// comment for why a bare `rect.x = target_x` assignment is wrong here).
fn shift_subtree_to_x(b: &mut LayoutBox, target_x: f32) {
    let dx = target_x - b.rect.x;
    crate::incremental::translate_subtree(b, dx, 0.0);
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
        RubyAlign::Center => crate::incremental::translate_subtree(row, slack / 2.0, 0.0),
        RubyAlign::SpaceBetween => {
            // Distribution needs the anonymous row wrapper (n_boxes > 1);
            // a single box stays flush at the start edge.
            if n_boxes > 1 {
                let gap = slack / (n_boxes - 1) as f32;
                for (i, child) in row.children.iter_mut().enumerate() {
                    crate::incremental::translate_subtree(child, gap * i as f32, 0.0);
                }
                row.rect.width = container_width;
            }
        }
        RubyAlign::SpaceAround => {
            if n_boxes > 1 {
                let gap = slack / n_boxes as f32;
                for (i, child) in row.children.iter_mut().enumerate() {
                    crate::incremental::translate_subtree(child, gap * (i as f32 + 0.5), 0.0);
                }
                row.rect.width = container_width;
            } else {
                // Single box: centered.
                crate::incremental::translate_subtree(row, slack / 2.0, 0.0);
            }
        }
    }
}

/// Render ruby text only (no base).
fn compose_ruby_text_only(ruby: &RubyBox) -> LayoutBox {
    stack_boxes_horizontal(&ruby.ruby_text_boxes)
}

/// Stack boxes horizontally (left-to-right).
// Тот же случай, что и в `mathml.rs` (docs/lint-policy.md §10): паника на пустом
// списке в коде, который сегодня не вызывается конвейером («`<ruby>` box-tree
// integration — deferred»), но экспортируется из крейта.
#[allow(clippy::panic)]
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
        let mut new_x = cursor_x;
        if i > 0 {
            new_x += 2.0; // Inter-character spacing.
        }
        // Move the WHOLE subtree, not just this box's own rect — `b` may be
        // a composed multi-level column (the `pair` branch in `lay_out_ruby`
        // stacks per-base `compose_column` results side by side), and every
        // descendant's x is relative to `b`'s own stale position. A bare
        // `b.rect.x = new_x` here left every column after the first painting
        // its base/annotation content at the FIRST column's x (GAP-RUBYBOX
        // multi-pair `ruby-merge: separate` regression, found via
        // `--dump-layout` on `<ruby>b1<rt>a1</rt>b2<rt>a2</rt></ruby>`).
        let dx = new_x - b.rect.x;
        crate::incremental::translate_subtree(&mut b, dx, 0.0);
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
fn make_anonymous_box_with_style(style: std::sync::Arc<crate::style::ComputedStyle>) -> LayoutBox {
    LayoutBox {
        node: NodeId::from_index(0),
        rect: Rect::ZERO,
        used_line_height: style.font_size * style.line_height,
        style,
        kind: BoxKind::Block,
        children: vec![],
        col_span: 1,
        row_span: 1,
        svg_group_transform: None,
        scroll_x: 0.0,
        scroll_y: 0.0,
        dirty: Default::default(),
        origin: BoxOrigin { node: None, role: BoxRole::AnonymousBlock },
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
        let mut b = make_anonymous_box_with_style(std::sync::Arc::new(ComputedStyle::root()));
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

    #[test]
    fn test_ruby_position_parse_and_serialize() {
        assert_eq!(RubyPosition::parse("alternate"), Some(RubyPosition::AlternateOver));
        assert_eq!(RubyPosition::parse("under  ALTERNATE"), Some(RubyPosition::AlternateUnder));
        assert_eq!(RubyPosition::parse("inter-character"), Some(RubyPosition::InterCharacter));
        assert_eq!(RubyPosition::parse("over under"), None);
        assert_eq!(RubyPosition::parse("alternate alternate"), None);
        assert_eq!(RubyPosition::parse("alternate inter-character"), None);
        assert_eq!(RubyPosition::default(), RubyPosition::AlternateOver);
        assert_eq!(RubyPosition::AlternateOver.as_css(), "alternate");
        assert_eq!(RubyPosition::AlternateUnder.as_css(), "alternate under");
    }

    /// The cases of WPT `css-ruby/ruby-position-alternate.html`.
    #[test]
    fn test_resolve_level_sides_alternation() {
        use RubyPosition::{AlternateOver as AO, AlternateUnder as AU, Over, Under};
        use RubySide as S;
        assert_eq!(resolve_level_sides(&[AO, AO, AO]), [S::Over, S::Under, S::Over]);
        assert_eq!(resolve_level_sides(&[AU, AU, AU]), [S::Under, S::Over, S::Under]);
        assert_eq!(resolve_level_sides(&[Under, AO, AO]), [S::Under, S::Over, S::Under]);
        assert_eq!(resolve_level_sides(&[Over, AO, AO]), [S::Over, S::Under, S::Over]);
        assert_eq!(resolve_level_sides(&[AO, Under, AO]), [S::Over, S::Under, S::Over]);
        assert_eq!(
            resolve_level_sides(&[RubyPosition::InterCharacter, AU]),
            [S::InterCharacter, S::Under]
        );
    }

    fn level(annotations: Vec<LayoutBox>, position: RubyPosition) -> RubyLevel {
        RubyLevel { annotations, position }
    }

    fn style() -> std::sync::Arc<ComputedStyle> {
        std::sync::Arc::new(ComputedStyle::root())
    }

    #[test]
    fn test_two_alternate_levels_go_over_then_under() {
        let seg = RubySegment {
            bases: vec![make_box(0.0, 0.0, 40.0, 20.0)],
            levels: vec![
                level(vec![make_box(0.0, 0.0, 40.0, 10.0)], RubyPosition::AlternateOver),
                level(vec![make_box(0.0, 0.0, 40.0, 8.0)], RubyPosition::AlternateOver),
            ],
        };
        let out = lay_out_ruby_segments(vec![seg], style(), RubyAlign::Start, RubyMerge::Separate);
        // children: base, level 1 (over), level 2 (under).
        assert_eq!(out.children[0].rect.y, 10.0);
        assert_eq!(out.children[1].rect.y, 0.0);
        assert_eq!(out.children[2].rect.y, 30.0);
        assert_eq!(out.rect.height, 38.0);
    }

    #[test]
    fn test_two_over_levels_stack_outward() {
        let seg = RubySegment {
            bases: vec![make_box(0.0, 0.0, 40.0, 20.0)],
            levels: vec![
                level(vec![make_box(0.0, 0.0, 40.0, 10.0)], RubyPosition::Over),
                level(vec![make_box(0.0, 0.0, 40.0, 6.0)], RubyPosition::Over),
            ],
        };
        let out = lay_out_ruby_segments(vec![seg], style(), RubyAlign::Start, RubyMerge::Separate);
        assert_eq!(out.children[0].rect.y, 16.0); // base below both levels
        assert_eq!(out.children[1].rect.y, 6.0); // first level nearest the base
        assert_eq!(out.children[2].rect.y, 0.0); // second level outermost
    }

    #[test]
    fn test_paired_and_spanning_levels() {
        // Two bases; level 1 pairs (2 annotations), level 2 spans (1 annotation).
        let seg = RubySegment {
            bases: vec![make_box(0.0, 0.0, 30.0, 20.0), make_box(0.0, 0.0, 30.0, 20.0)],
            levels: vec![
                level(
                    vec![make_box(0.0, 0.0, 10.0, 8.0), make_box(0.0, 0.0, 10.0, 8.0)],
                    RubyPosition::Over,
                ),
                level(vec![make_box(0.0, 0.0, 20.0, 8.0)], RubyPosition::Under),
            ],
        };
        let out = lay_out_ruby_segments(vec![seg], style(), RubyAlign::Center, RubyMerge::Separate);
        assert_eq!(out.rect.width, 62.0);
        // Paired annotations centred over their own base column.
        assert_eq!(out.children[2].rect.x, 10.0);
        assert_eq!(out.children[3].rect.x, 42.0);
        // Spanning annotation centred under the whole segment.
        assert_eq!(out.children[4].rect.x, 21.0);
        assert_eq!(out.children[4].rect.y, 28.0);
    }

    #[test]
    fn test_inter_character_sits_after_its_base() {
        let seg = RubySegment {
            bases: vec![make_box(0.0, 0.0, 30.0, 20.0), make_box(0.0, 0.0, 30.0, 20.0)],
            levels: vec![level(
                vec![make_box(0.0, 0.0, 8.0, 12.0), make_box(0.0, 0.0, 8.0, 12.0)],
                RubyPosition::InterCharacter,
            )],
        };
        let out = lay_out_ruby_segments(vec![seg], style(), RubyAlign::Start, RubyMerge::Separate);
        let (b0, b1, a0, a1) = (&out.children[0], &out.children[1], &out.children[2], &out.children[3]);
        assert_eq!(a0.rect.x, b0.rect.x + 30.0);
        assert_eq!(a0.rect.y, b0.rect.y);
        assert_eq!(b1.rect.x, 40.0); // column 0 is base + annotation wide, plus the gap
        assert_eq!(a1.rect.x, 70.0);
        assert_eq!(out.rect.height, 20.0); // no block-axis growth
    }

    #[test]
    fn test_segments_share_base_line() {
        let annotated = RubySegment {
            bases: vec![make_box(0.0, 0.0, 30.0, 20.0)],
            levels: vec![level(vec![make_box(0.0, 0.0, 30.0, 10.0)], RubyPosition::Over)],
        };
        let bare = RubySegment { bases: vec![make_box(0.0, 0.0, 30.0, 20.0)], levels: vec![] };
        let out = lay_out_ruby_segments(
            vec![annotated, bare],
            style(),
            RubyAlign::Start,
            RubyMerge::Separate,
        );
        assert_eq!(out.children.len(), 2);
        let annotated_base_y = out.children[0].children[1].rect.y;
        assert_eq!(out.children[1].rect.y, annotated_base_y);
        assert_eq!(out.rect.height, 30.0);
    }

    #[test]
    fn test_empty_segments_do_not_panic() {
        let out = lay_out_ruby_segments(vec![], style(), RubyAlign::Start, RubyMerge::Separate);
        assert!(out.children.is_empty());
    }
}
