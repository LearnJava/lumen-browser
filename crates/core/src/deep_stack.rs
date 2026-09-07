//! Stack reservation for traversals that recurse on DOM depth.
//!
//! Box-tree build, style passes, DOM import and the dump serializers all
//! descend recursively, and the measured cost is ~10.4 KB of stack per level
//! of nesting (BUG-1027, gdb `info frame` on the `build_box_or_reuse` →
//! `build_box` → `build_box_inner` cycle). A thread left on the platform
//! default therefore dies on ordinary pages: 2 MiB (Rust's default for a
//! spawned thread) is exhausted at ~190 levels, 8 MiB (Linux main thread,
//! `ulimit -s`) at ~790. There is no panic and no backtrace — the runtime's
//! stack-overflow handler calls `abort()`, so the process is gone with two
//! lines of stderr.
//!
//! Until every such traversal is iterative ([LAYOUT-1]/[LAYOUT-2] in
//! `ROADMAP.md`), any thread that runs one gets [`DEEP_TREE_STACK_BYTES`].
//! Windows gets the same reserve for its main thread from the linker
//! (`/STACK:` in `crates/shell/build.rs`); on Unix the main thread's size
//! comes from `RLIMIT_STACK` and cannot be set at link time, which is what
//! [`run_on_deep_stack`] is for.

/// Stack reserve for a thread that runs traversals recursive on DOM depth.
///
/// 128 MiB ≈ 12 000 levels of nesting at the measured ~10.4 KB per level. The
/// cost is address space only — pages are committed as they are touched.
pub const DEEP_TREE_STACK_BYTES: usize = 128 * 1024 * 1024;

/// Runs `f` on a scoped thread named `name` with [`DEEP_TREE_STACK_BYTES`] of
/// stack and returns its value.
///
/// The thread is scoped, so `f` may borrow from the caller and the call is a
/// plain synchronous hand-off: control returns only when `f` is done. A panic
/// inside `f` is re-raised in the caller unchanged, so the caller's unwinding
/// behaviour is exactly what it would be without the hop.
///
/// # Errors
/// Returns [`std::io::Error`] if the OS refused to create the thread — the
/// caller decides whether to fall back to running `f` on the current stack.
pub fn run_on_deep_stack<T, F>(name: &str, f: F) -> std::io::Result<T>
where
    F: FnOnce() -> T + Send,
    T: Send,
{
    std::thread::scope(|scope| {
        let handle = std::thread::Builder::new()
            .name(name.to_owned())
            .stack_size(DEEP_TREE_STACK_BYTES)
            .spawn_scoped(scope, f)?;
        match handle.join() {
            Ok(value) => Ok(value),
            // Propagate the original panic payload: `run_on_deep_stack` must be
            // invisible to a caller that installed a panic hook or catches unwind.
            Err(payload) => std::panic::resume_unwind(payload),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_the_closure_value() {
        let out = run_on_deep_stack("test-deep", || 2 + 2);
        assert!(matches!(out, Ok(4)));
    }

    #[test]
    fn borrows_from_the_caller() {
        let owned = String::from("borrowed");
        let len = run_on_deep_stack("test-deep-borrow", || owned.len());
        assert!(matches!(len, Ok(8)));
    }

    /// The whole point of the module: a recursion that overflows a default
    /// 2 MiB thread must survive on the deep stack. Each level keeps its own
    /// 1 KiB buffer (`black_box` stops the optimizer from eliding it), so
    /// 8000 levels ≈ 8 MiB — well over any default, well under the reserve.
    #[test]
    fn survives_recursion_past_the_default_stack() {
        fn descend(n: u32) -> u8 {
            let mut pad = [0u8; 1024];
            pad[0] = n as u8;
            let pad = std::hint::black_box(&mut pad);
            if n == 0 { pad[0] } else { pad[0] ^ descend(n - 1) }
        }
        let out = run_on_deep_stack("test-deep-recurse", || descend(8000));
        assert!(out.is_ok());
    }
}
