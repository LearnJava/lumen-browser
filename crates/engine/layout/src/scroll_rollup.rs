//! BUG-935 срез 75 — the JS-visible scroll-container list, computed in one bottom-up pass.
//!
//! [`crate::scroll_container_into`] publishes an entry for every box that is a scroll container
//! and, since BUG-960, for every `overflow: visible` box whose content sticks out of its padding
//! box. Both need the box's scrollable-overflow extent, and `scrollable_extent_{x,y}` finds it by
//! walking the box's whole subtree — once per axis, once per box. Run for a chain of nested boxes
//! (the spine of an incremental flush: `html > body > …`, each with the document under it) that
//! is `depth × 2 × subtree` box visits per flush: 0.8 ms of a 2.9 ms flush on a 2 200-element
//! page, for a result that almost never changes.
//!
//! The extent of a box is a fold over its *descendants*, reaching through every box that does not
//! clip its own overflow ([`crate::box_clips_own_overflow`]). That is the same set a child's
//! parent folds, one level down — so each box's descendants are folded once, into an
//! [`OverflowRollup`] its parent merges, instead of once per ancestor. What cannot be merged
//! blindly is the one member kind whose contribution depends on the container being asked: an
//! absolutely positioned or fixed box counts only if it is not wholly beyond an unreachable side of the container's padding box
//! ([`crate::contributes_to_scrollable_overflow`]). Those are kept as a list and filtered against
//! each container's own padding box; everything else is four running min/max values.
//!
//! The result is bit-identical to the per-box walks (`min`/`max` are associative, and the padding
//! box origin is subtracted from the folded edge exactly as it was from each edge), which the
//! differential tests in this file compare against [`crate::collect_scroll_containers_inner`].
//!
//! BUG-935 срез 77: the pass is still one visit per box of the document, and on a flush whose dirty
//! root is `body` the document is almost entirely subtrees the plan proved untouched and unmoved
//! ([`crate::scoped_collect::ScopedCollection`]). What such a subtree adds to its parent's extent is a
//! function of its own boxes only, so the rollup computed for it is kept ([`RollupCache`]) and the
//! next flush folds it in instead of walking it.

use lumen_core::geom::Rect;
use lumen_core::id_hash::{IdMap, IdSet};
use lumen_dom::NodeId;

use crate::style::{Overflow, Position};
use crate::{
    box_clips_own_overflow, child_scrollable_bounds, padding_box, LayoutBox, ScrollContainer, UnreachableSides,
};

/// What the descendants of a box (reached through boxes that do not clip) add to the
/// scrollable-overflow extent of whichever box is asked — see the module doc.
#[derive(Clone)]
pub(crate) struct OverflowRollup {
    /// Left-most / right-most / top-most / bottom-most edge of the in-flow members, in the
    /// coordinates of the tree (not relative to any padding box). `±∞` while there are none.
    min_x: f32,
    max_x: f32,
    min_y: f32,
    max_y: f32,
    /// Border boxes (transform-expanded) of the `position: absolute|fixed` members, which count
    /// unless they lie wholly beyond an unreachable side of the container's padding box.
    positioned: Vec<Rect>,
}

impl OverflowRollup {
    fn empty() -> Self {
        Self {
            min_x: f32::INFINITY,
            max_x: f32::NEG_INFINITY,
            min_y: f32::INFINITY,
            max_y: f32::NEG_INFINITY,
            positioned: Vec::new(),
        }
    }

    /// Folds `c`'s own scrollable bounds in (not its descendants — see [`Self::merge`]).
    fn add_member(&mut self, c: &LayoutBox) {
        let bounds = child_scrollable_bounds(c);
        if matches!(c.style.position, Position::Absolute | Position::Fixed) {
            self.positioned.push(bounds);
        } else {
            self.min_x = self.min_x.min(bounds.x);
            self.max_x = self.max_x.max(bounds.x + bounds.width);
            self.min_y = self.min_y.min(bounds.y);
            self.max_y = self.max_y.max(bounds.y + bounds.height);
        }
    }

    /// Folds in everything a non-clipping child's subtree contributes.
    fn merge(&mut self, mut other: Self) {
        self.min_x = self.min_x.min(other.min_x);
        self.max_x = self.max_x.max(other.max_x);
        self.min_y = self.min_y.min(other.min_y);
        self.max_y = self.max_y.max(other.max_y);
        if self.positioned.is_empty() {
            self.positioned = other.positioned;
        } else {
            self.positioned.append(&mut other.positioned);
        }
    }

    /// `(scrollWidth, scrollHeight)` magnitudes for a box whose padding box is `pb`: what
    /// [`crate::content_width`] / [`crate::content_height`] return, without a walk.
    fn content_size(&self, pb: &Rect, u: UnreachableSides) -> (f32, f32) {
        let (mut min_x, mut max_x) = (0.0_f32, pb.width);
        let (mut min_y, mut max_y) = (0.0_f32, pb.height);
        min_x = min_x.min(self.min_x - pb.x);
        max_x = max_x.max(self.max_x - pb.x);
        min_y = min_y.min(self.min_y - pb.y);
        max_y = max_y.max(self.max_y - pb.y);
        for bounds in &self.positioned {
            if !u.hides(bounds, pb) {
                min_x = min_x.min(bounds.x - pb.x);
                max_x = max_x.max(bounds.x + bounds.width - pb.x);
                min_y = min_y.min(bounds.y - pb.y);
                max_y = max_y.max(bounds.y + bounds.height - pb.y);
            }
        }
        (max_x - min_x, max_y - min_y)
    }
}

/// [`crate::scroll_container_into`] with `include_non_wheel = true`, the extent coming from `r`.
fn publish(b: &LayoutBox, r: &OverflowRollup, out: &mut Vec<ScrollContainer>) {
    let s = &b.style;
    let scrolls = |o: Overflow| matches!(o, Overflow::Scroll | Overflow::Auto | Overflow::Hidden | Overflow::Clip);
    if scrolls(s.overflow_x) || scrolls(s.overflow_y) {
        let clip = padding_box(b);
        let (scroll_width, scroll_height) = r.content_size(&clip, UnreachableSides::of(s));
        out.push(ScrollContainer {
            node: b.node,
            clip_rect: clip,
            scroll_width,
            scroll_height,
            scroll_x: b.scroll_x,
            scroll_y: b.scroll_y,
            overscroll_behavior_x: s.overscroll_behavior_x,
            overscroll_behavior_y: s.overscroll_behavior_y,
        });
    } else if matches!(s.overflow_x, Overflow::Visible) && matches!(s.overflow_y, Overflow::Visible) {
        let clip = padding_box(b);
        let (scroll_width, scroll_height) = r.content_size(&clip, UnreachableSides::of(s));
        if scroll_width > clip.width + 0.01 || scroll_height > clip.height + 0.01 {
            out.push(ScrollContainer {
                node: b.node,
                clip_rect: clip,
                scroll_width,
                scroll_height,
                scroll_x: 0.0,
                scroll_y: 0.0,
                overscroll_behavior_x: s.overscroll_behavior_x,
                overscroll_behavior_y: s.overscroll_behavior_y,
            });
        }
    }
}

/// The boxes a [`Walk`] publishes an entry for.
enum Wanted<'x, 'a> {
    /// Every box under the root.
    All,
    /// The listed boxes — `(box, whole)`, in tree pre-order — and, for a `whole` one, every box
    /// under it. Nothing else: the rest of the tree is walked only to roll its overflow up.
    Items { items: &'x [(&'a LayoutBox, bool)], next: usize, whole_depth: u32 },
}

/// The rollups of the subtrees an incremental flush left alone, by the node of the subtree's top
/// box: what [`Walk`] folds in for a skipped subtree instead of walking it. Kept with the layout tree
/// the subtrees were taken from, so an entry is only ever read against the tree it was computed on.
#[derive(Default)]
pub struct RollupCache(IdMap<NodeId, OverflowRollup>);

impl RollupCache {
    /// How many subtrees have a rollup kept.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether no subtree has a rollup kept.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// The subtrees a walk may take from the cache, and the cache it leaves for the next flush.
pub(crate) struct Skips {
    /// Addresses of the top boxes of the subtrees the plan left alone.
    tops: IdSet<usize>,
    cached: RollupCache,
    kept: RollupCache,
    /// How many tops were served from `cached` / had to be walked.
    pub(crate) hits: u32,
    pub(crate) misses: u32,
}

impl Skips {
    pub(crate) fn new(tops: &[&LayoutBox], cached: RollupCache) -> Self {
        Self {
            tops: tops.iter().map(|b| std::ptr::from_ref::<LayoutBox>(b) as usize).collect(),
            cached,
            kept: RollupCache::default(),
            hits: 0,
            misses: 0,
        }
    }

    pub(crate) fn into_kept(self) -> RollupCache {
        self.kept
    }

    fn is_top(&self, b: &LayoutBox) -> bool {
        !self.tops.is_empty() && self.tops.contains(&(std::ptr::from_ref::<LayoutBox>(b) as usize))
    }
}

struct Walk<'x, 'a> {
    wanted: Wanted<'x, 'a>,
    /// `None` for a walk that visits every box.
    skips: Option<&'x mut Skips>,
    /// Entries with the pre-order index of their box, so the result can be put back in tree order.
    out: Vec<(u32, ScrollContainer)>,
    visited: u32,
}

impl Walk<'_, '_> {
    /// What the descendants of `b` add to the extent of whichever box is asked.
    fn fold_children(&mut self, b: &LayoutBox) -> OverflowRollup {
        let mut rollup = OverflowRollup::empty();
        for c in &b.children {
            rollup.add_member(c);
            let below = self.visit(c);
            if !box_clips_own_overflow(c) {
                rollup.merge(below);
            }
        }
        rollup
    }

    fn visit(&mut self, b: &LayoutBox) -> OverflowRollup {
        let index = self.visited;
        self.visited += 1;
        let mut entered_whole = false;
        let wanted = match &mut self.wanted {
            Wanted::All => true,
            Wanted::Items { items, next, whole_depth } => {
                let mut wanted = *whole_depth > 0;
                if let Some(&(item, whole)) = items.get(*next)
                    && std::ptr::eq(item, b)
                {
                    *next += 1;
                    wanted = true;
                    if whole {
                        *whole_depth += 1;
                        entered_whole = true;
                    }
                }
                wanted
            }
        };
        // A subtree the plan left alone publishes nothing and its extent is what it was.
        if !wanted && self.skips.as_ref().is_some_and(|s| s.is_top(b)) {
            let cached = self.skips.as_mut().and_then(|s| s.cached.0.remove(&b.node));
            let rollup = match cached {
                Some(rollup) => {
                    if let Some(s) = self.skips.as_mut() {
                        s.hits += 1;
                    }
                    rollup
                }
                None => {
                    if let Some(s) = self.skips.as_mut() {
                        s.misses += 1;
                    }
                    self.fold_children(b)
                }
            };
            if let Some(s) = self.skips.as_mut() {
                s.kept.0.insert(b.node, rollup.clone());
            }
            return rollup;
        }
        let rollup = self.fold_children(b);
        if wanted {
            let mut one = Vec::new();
            publish(b, &rollup, &mut one);
            self.out.extend(one.into_iter().map(|c| (index, c)));
        }
        if entered_whole && let Wanted::Items { whole_depth, .. } = &mut self.wanted {
            *whole_depth -= 1;
        }
        rollup
    }

    fn finish(mut self) -> Vec<ScrollContainer> {
        self.out.sort_by_key(|(index, _)| *index);
        self.out.into_iter().map(|(_, c)| c).collect()
    }
}

/// [`crate::collect_scroll_containers_inner`] with `include_non_wheel = true` over each of
/// `roots`, in one pass per root.
pub(crate) fn collect_for_js_state(roots: &[&LayoutBox]) -> Vec<ScrollContainer> {
    let mut all = Vec::new();
    for root in roots {
        let mut walk = Walk { wanted: Wanted::All, skips: None, out: Vec::new(), visited: 0 };
        walk.visit(root);
        all.extend(walk.finish());
    }
    all
}

/// The entries for the planned `items` of an incremental flush — a `whole` item and everything
/// under it, a spine item only itself — computed in one walk from the topmost item. `items` must
/// be in tree pre-order, as [`crate::scoped_collect::ScopedCollection`] builds them.
pub(crate) fn collect_for_items(items: &[(&LayoutBox, bool)], mut skips: Option<&mut Skips>) -> Vec<ScrollContainer> {
    let mut all = Vec::new();
    let mut next = 0;
    while let Some(&(top, _)) = items.get(next) {
        let mut walk = Walk {
            wanted: Wanted::Items { items, next, whole_depth: 0 },
            skips: skips.as_deref_mut(),
            out: Vec::new(),
            visited: 0,
        };
        walk.visit(top);
        // The walk consumed `top` at least; items it did not meet (not under `top`, or out of
        // order) get a walk of their own.
        next = match walk.wanted {
            Wanted::Items { next, .. } => next,
            Wanted::All => items.len(),
        };
        all.extend(walk.finish());
    }
    all
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lay_full;
    use crate::{collect_scroll_containers_inner, scroll_container_into};

    /// Every field, floats by their bits.
    fn key(c: &ScrollContainer) -> String {
        let r = c.clip_rect;
        format!(
            "{:?} clip={:x},{:x},{:x},{:x} sw={:x} sh={:x} sx={:x} sy={:x} ob={:?}/{:?}",
            c.node,
            r.x.to_bits(),
            r.y.to_bits(),
            r.width.to_bits(),
            r.height.to_bits(),
            c.scroll_width.to_bits(),
            c.scroll_height.to_bits(),
            c.scroll_x.to_bits(),
            c.scroll_y.to_bits(),
            c.overscroll_behavior_x,
            c.overscroll_behavior_y,
        )
    }

    fn keys(v: &[ScrollContainer]) -> Vec<String> {
        v.iter().map(key).collect()
    }

    /// Page shapes whose scrollable overflow is easy to get subtly wrong: a clipping box between
    /// two visible ones, positioned boxes that overlap the container only for some ancestors,
    /// a transform that pushes a box out, negative margins, a nested scroller.
    const FIXTURES: &[(&str, &str)] = &[
        ("<div id=a><div id=b><p>x</p><p>y</p></div></div>", ""),
        (
            "<div id=a><div id=b style=\"overflow:hidden;width:50px;height:20px\"><div id=c style=\"width:400px;height:300px\"></div></div></div>",
            "",
        ),
        (
            "<div id=a style=\"width:100px;height:60px\"><div id=b style=\"position:relative;width:80px;height:40px\">\
             <div id=c style=\"position:absolute;left:70px;top:30px;width:200px;height:200px\"></div>\
             <div id=d style=\"position:absolute;left:900px;top:900px;width:10px;height:10px\"></div></div></div>",
            "",
        ),
        (
            "<div id=a style=\"width:100px;height:60px\"><div id=b style=\"width:40px;height:40px;transform:translate(300px,10px)\"></div>\
             <div id=c style=\"margin-left:-50px;width:20px;height:20px\"></div></div>",
            "",
        ),
        (
            "<div id=a style=\"overflow:auto;width:100px;height:60px\"><div id=b><div id=c style=\"overflow:scroll;width:60px;height:30px\">\
             <div style=\"width:500px;height:500px\"></div></div></div></div>",
            "",
        ),
        (
            "<div id=a style=\"width:100px;height:60px\"><div id=b style=\"position:fixed;left:10px;top:10px;width:300px;height:5px\"></div>\
             <div id=c><div id=d><div id=e style=\"position:absolute;left:5px;top:5px;width:700px;height:9px\"></div></div></div></div>",
            "",
        ),
        (
            "<div id=a><div id=b style=\"height:2000px\"></div><div id=c style=\"position:absolute;top:3000px;left:0;width:10px;height:10px\"></div></div>",
            "html, body { overflow: visible }",
        ),
    ];

    /// Pre-order boxes with the ancestor chain of each.
    fn preorder(root: &LayoutBox) -> Vec<(&LayoutBox, Vec<usize>)> {
        fn go<'a>(b: &'a LayoutBox, path: &mut Vec<usize>, out: &mut Vec<(&'a LayoutBox, Vec<usize>)>) {
            let me = out.len();
            out.push((b, path.clone()));
            path.push(me);
            for c in &b.children {
                go(c, path, out);
            }
            path.pop();
        }
        let mut out = Vec::new();
        go(root, &mut Vec::new(), &mut out);
        out
    }

    #[test]
    fn the_rollup_publishes_what_the_per_box_walks_did() {
        for (html, css) in FIXTURES {
            let root = lay_full(html, css);
            let mut old = Vec::new();
            collect_scroll_containers_inner(&root, &mut old, true);
            assert_eq!(keys(&collect_for_js_state(&[&root])), keys(&old), "{html}");
        }
    }

    /// The planned-items walk against the item-by-item collection it replaced, for every choice of
    /// one or two whole items (their ancestors are the spine, as in a flush).
    #[test]
    fn the_items_walk_publishes_what_the_item_by_item_collection_did() {
        for (html, css) in FIXTURES {
            let root = lay_full(html, css);
            let boxes = preorder(&root);
            let mut compared = 0;
            for x in 0..boxes.len() {
                for y in x..boxes.len() {
                    // Two whole items must not nest; the plan never produces that.
                    if x != y && boxes[y].1.contains(&x) {
                        continue;
                    }
                    let whole = [x, y];
                    let spine: Vec<usize> = boxes[x].1.iter().chain(&boxes[y].1).copied().collect();
                    let items: Vec<(&LayoutBox, bool)> = boxes
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| whole.contains(i) || spine.contains(i))
                        .map(|(i, (b, _))| (*b, whole.contains(&i)))
                        .collect();
                    let mut old = Vec::new();
                    for &(b, whole) in &items {
                        if whole {
                            collect_scroll_containers_inner(b, &mut old, true);
                        } else {
                            scroll_container_into(b, &mut old, true);
                        }
                    }
                    assert_eq!(keys(&collect_for_items(&items, None)), keys(&old), "{html} whole={whole:?}");
                    compared += 1;
                }
            }
            assert!(compared > 10, "{html}: only {compared} item sets");
        }
    }

    /// The point of the pass: a box's subtree is folded once, not once per ancestor.
    #[test]
    fn a_deep_spine_is_walked_once() {
        let depth = 40;
        let html = format!("{}<p>x</p>{}", "<div>".repeat(depth), "</div>".repeat(depth));
        let root = lay_full(&html, "");
        let boxes = preorder(&root);
        let items: Vec<(&LayoutBox, bool)> = boxes.iter().map(|(b, _)| (*b, false)).collect();
        let mut walk = Walk { wanted: Wanted::Items { items: &items, next: 0, whole_depth: 0 }, skips: None, out: Vec::new(), visited: 0 };
        walk.visit(items[0].0);
        assert_eq!(walk.visited as usize, boxes.len(), "every box visited exactly once for {} items", items.len());
    }
}
