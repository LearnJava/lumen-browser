//! Lightweight, zero-dependency hierarchical scope-timer for ad-hoc
//! performance investigation, gated behind `LUMEN_PROFILE_TREE=1`.
//!
//! Replaces one-off `eprintln!`-based timers (added and removed by hand
//! during BUG-284's investigation) with a small reusable utility any
//! downstream crate can call without adding a dependency. For a proper
//! visual, low-overhead profiler with a timeline UI instead of a printed
//! call tree, see [`crate::tracy_zone`] and `docs/plan/security-performance.md`
//! §14.3 — this module is meant for quick, no-GUI-required call-tree dumps,
//! not continuous production profiling.
//!
//! # Usage
//!
//! ```
//! fn layout_measured_hyp() {
//!     let _s = lumen_core::profile::scope("layout_measured_hyp");
//!     {
//!         let _s = lumen_core::profile::scope("precompute_counters");
//!         // ... work ...
//!     }
//!     {
//!         let _s = lumen_core::profile::scope("build_box");
//!         // ... work ...
//!     }
//! }
//! ```
//!
//! With `LUMEN_PROFILE_TREE` unset, [`scope`] is a single relaxed env-var
//! check (cached after the first call) plus a no-op guard — negligible cost
//! even called once per DOM node. With it set, the outermost scope's guard
//! drop prints an indented call tree to stderr:
//!
//! ```text
//! [profile]    623.41ms  layout_measured_hyp
//! [profile]      465.02ms    precompute_counters
//! [profile]      612.88ms    build_box
//! [profile]       22.65ms    lay_out
//! ```
//!
//! Sibling scopes that share a name are **merged** — their times are summed
//! and the call count is printed as `×N`. That is what makes the utility
//! usable inside per-node code (BUG-341 S10 instrumented `compute_style`,
//! which runs thousands of times per layout pass; without merging the dump
//! would be one line per call):
//!
//! ```text
//! [profile]     20.23ms  precompute_counters
//! [profile]      18.90ms    compute_style ×2317
//! [profile]        7.41ms      cs_match ×2317
//! [profile]        4.02ms      cs_init ×2317
//! ```

use std::cell::RefCell;
use std::sync::OnceLock;
use std::time::Instant;

/// Re-exported so the [`crate::tracy_zone`] macro can reach `tracy-client`
/// from downstream crates without them adding their own direct dependency —
/// they only need to declare their own `tracy` feature and forward it to
/// `lumen-core/tracy` (see `docs/plan/security-performance.md` §14.3).
#[cfg(feature = "tracy")]
#[doc(hidden)]
pub use tracy_client;

fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var("LUMEN_PROFILE_TREE").is_ok())
}

/// Whether the fine-grained per-node scopes ([`scope_detail`]) are on.
fn detail_enabled() -> bool {
    static DETAIL: OnceLock<bool> = OnceLock::new();
    *DETAIL.get_or_init(|| enabled() && std::env::var("LUMEN_PROFILE_DETAIL").is_ok())
}

/// The thread that opened the first root scope — the only one whose trees are
/// printed.
///
/// The call tree is thread-local by construction, so a scope opened on a rayon
/// worker (e.g. `build_box`'s parallel per-child cascade) starts a *root* frame
/// there and prints a whole tree of its own on every call. With per-node scopes
/// that is hundreds of trees per layout pass, and the stderr writes land inside
/// the very stage being measured: an instrumented run once reported `build_box`
/// at 288 ms against a true ~10 ms, purely as print overhead. Worker-thread scopes
/// are therefore ignored; their time still shows up in the enclosing stage on
/// the owning thread, which is where it belongs.
fn owns_tree() -> bool {
    static OWNER: OnceLock<std::thread::ThreadId> = OnceLock::new();
    *OWNER.get_or_init(|| std::thread::current().id()) == std::thread::current().id()
}

/// One in-progress scope on the current thread's call stack.
struct Frame {
    name: &'static str,
    start: Instant,
    children: Vec<Node>,
}

/// One completed scope, with its own completed children (call-tree node).
///
/// `count` is the number of same-named sibling scopes folded into this node
/// (see [`merge_into`]); `elapsed_ms` is then their sum.
struct Node {
    name: &'static str,
    elapsed_ms: f64,
    count: usize,
    children: Vec<Node>,
}

/// Folds `node` into `siblings`, summing time and call count with an
/// already-present entry of the same name (recursively, so the merged node's
/// own children stay merged too).
///
/// Without this a scope opened once per DOM node would print one line per
/// call. The lookup is a linear scan because a profiled scope has a handful of
/// distinct children by construction — the wide dimension is the call count,
/// which merging collapses.
fn merge_into(siblings: &mut Vec<Node>, node: Node) {
    if let Some(existing) = siblings.iter_mut().find(|s| s.name == node.name) {
        existing.elapsed_ms += node.elapsed_ms;
        existing.count += node.count;
        for child in node.children {
            merge_into(&mut existing.children, child);
        }
    } else {
        siblings.push(node);
    }
}

thread_local! {
    static STACK: RefCell<Vec<Frame>> = const { RefCell::new(Vec::new()) };
}

/// RAII guard returned by [`scope`]. Records elapsed time into the
/// thread-local call tree when dropped; a no-op when profiling is disabled.
#[must_use = "the scope ends when this guard is dropped — bind it to a name, not `_`"]
pub struct ScopeGuard {
    active: bool,
}

/// Opens a named profiling scope for the current thread. Returns a guard
/// that closes the scope (recording its elapsed time as a child of whatever
/// scope is currently open, or printing the whole call tree if this was the
/// outermost one) when dropped.
///
/// No-op (a single cached env-var check) unless `LUMEN_PROFILE_TREE` is set.
pub fn scope(name: &'static str) -> ScopeGuard {
    if !enabled() {
        return ScopeGuard { active: false };
    }
    open(name)
}

/// Like [`scope`], but additionally requires `LUMEN_PROFILE_DETAIL=1`.
///
/// For scopes that run once per DOM node (BUG-341 S10 instrumented the phases
/// of `compute_style` and of the pseudo-element cascade). Two `Instant::now()`
/// calls and a thread-local push per node measurably inflate the very stage
/// they sit in — an instrumented `precompute_counters` roughly doubled — so
/// they stay off during an ordinary `LUMEN_PROFILE_TREE=1` stage run, whose
/// absolute numbers must stay comparable with the ones recorded in
/// `bugs/BUG-341-OPEN.md`. Turn detail on to read *shares within* a stage; do
/// not compare its absolute numbers against a stage-only run.
pub fn scope_detail(name: &'static str) -> ScopeGuard {
    if !detail_enabled() {
        return ScopeGuard { active: false };
    }
    open(name)
}

fn open(name: &'static str) -> ScopeGuard {
    // A root frame on a non-owning thread would print a tree of its own on
    // every call — see `owns_tree`. Nested scopes on such a thread see an empty
    // stack too, so they are skipped by the same check.
    let is_root = STACK.with(|s| s.borrow().is_empty());
    if is_root && !owns_tree() {
        return ScopeGuard { active: false };
    }
    STACK.with(|s| {
        s.borrow_mut().push(Frame {
            name,
            start: Instant::now(),
            children: Vec::new(),
        });
    });
    ScopeGuard { active: true }
}

impl Drop for ScopeGuard {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        STACK.with(|s| {
            let mut stack = s.borrow_mut();
            let Some(frame) = stack.pop() else {
                // Stack underflow would mean a guard outlived a `scope()` call
                // from a different thread/stack — cannot happen through the
                // public API, but fail soft rather than panic in a profiling
                // helper.
                return;
            };
            let node = Node {
                name: frame.name,
                elapsed_ms: frame.start.elapsed().as_secs_f64() * 1000.0,
                count: 1,
                children: frame.children,
            };
            match stack.last_mut() {
                Some(parent) => merge_into(&mut parent.children, node),
                None => print_tree(&node),
            }
        });
    }
}

fn print_tree(root: &Node) {
    fn go(node: &Node, depth: usize) {
        let calls = if node.count > 1 {
            format!(" ×{}", node.count)
        } else {
            String::new()
        };
        eprintln!(
            "[profile] {:>9.2}ms  {}{}{}",
            node.elapsed_ms,
            "  ".repeat(depth),
            node.name,
            calls
        );
        for child in &node.children {
            go(child, depth + 1);
        }
    }
    go(root, 0);
}

/// Opens a Tracy zone for the current scope — a real visual, low-overhead
/// profiler viewed live in the separate Tracy GUI app
/// (<https://github.com/wolfpld/tracy>, download + run it first). Compiles to
/// nothing unless the calling crate's own `tracy` Cargo feature is enabled
/// (which must in turn forward to `lumen-core/tracy` — Cargo feature
/// unification means every crate in the dependency chain needs its own
/// `tracy` feature name for this macro's internal `#[cfg(feature = "tracy")]`
/// to evaluate against the *calling* crate, not `lumen-core`).
///
/// Pairs with [`scope`] rather than replacing it: this macro is for a human
/// visually profiling a real session with the Tracy GUI; `scope` is for a
/// quick, no-GUI-required call-tree dump (e.g. from an agent's shell). Use
/// both at the same call site when instrumenting a new hot path — see
/// `docs/plan/security-performance.md` §14.3 for the full setup + usage.
///
/// ```ignore
/// fn layout_measured_hyp() {
///     let _prof = lumen_core::profile::scope("layout_measured_hyp");
///     lumen_core::tracy_zone!("layout_measured_hyp");
///     // ... work ...
/// }
/// ```
#[macro_export]
macro_rules! tracy_zone {
    ($name:literal) => {
        #[cfg(feature = "tracy")]
        let _tracy_zone = $crate::profile::tracy_client::span!($name);
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf(name: &'static str, ms: f64) -> Node {
        Node { name, elapsed_ms: ms, count: 1, children: Vec::new() }
    }

    #[test]
    fn same_named_siblings_merge_with_a_call_count() {
        let mut siblings = Vec::new();
        merge_into(
            &mut siblings,
            Node { name: "compute_style", elapsed_ms: 1.0, count: 1, children: vec![leaf("cs_match", 0.5)] },
        );
        merge_into(
            &mut siblings,
            Node { name: "compute_style", elapsed_ms: 2.0, count: 1, children: vec![leaf("cs_match", 1.5)] },
        );
        merge_into(&mut siblings, leaf("lay_out", 4.0));

        assert_eq!(siblings.len(), 2, "distinct names stay distinct");
        assert_eq!(siblings[0].count, 2);
        assert!((siblings[0].elapsed_ms - 3.0).abs() < 1e-9, "times sum");
        // Merging must recurse: the children of two merged nodes are one child.
        assert_eq!(siblings[0].children.len(), 1);
        assert_eq!(siblings[0].children[0].count, 2);
        assert!((siblings[0].children[0].elapsed_ms - 2.0).abs() < 1e-9);
        assert_eq!(siblings[1].count, 1);
    }

    #[test]
    fn disabled_scope_is_free_of_side_effects() {
        // LUMEN_PROFILE_TREE is unset in the test environment — scope() must
        // not touch the thread-local stack at all.
        let _s = scope("test-scope");
        STACK.with(|s| assert!(s.borrow().is_empty()));
    }
}
