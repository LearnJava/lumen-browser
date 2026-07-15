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

/// One in-progress scope on the current thread's call stack.
struct Frame {
    name: &'static str,
    start: Instant,
    children: Vec<Node>,
}

/// One completed scope, with its own completed children (call-tree node).
struct Node {
    name: &'static str,
    elapsed_ms: f64,
    children: Vec<Node>,
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
                children: frame.children,
            };
            match stack.last_mut() {
                Some(parent) => parent.children.push(node),
                None => print_tree(&node),
            }
        });
    }
}

fn print_tree(root: &Node) {
    fn go(node: &Node, depth: usize) {
        eprintln!(
            "[profile] {:>9.2}ms  {}{}",
            node.elapsed_ms,
            "  ".repeat(depth),
            node.name
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

    #[test]
    fn disabled_scope_is_free_of_side_effects() {
        // LUMEN_PROFILE_TREE is unset in the test environment — scope() must
        // not touch the thread-local stack at all.
        let _s = scope("test-scope");
        STACK.with(|s| assert!(s.borrow().is_empty()));
    }
}
