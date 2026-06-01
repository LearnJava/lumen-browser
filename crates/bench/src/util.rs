//! Shared helpers for the benchmark binary.

use lumen_dom::{Document, NodeData, NodeId};

/// Returns the current process RSS (resident set size) in bytes.
///
/// Uses `getrusage` on Unix and `GetProcessMemoryInfo` on Windows.
/// Returns 0 on unsupported platforms.
pub fn get_rss_bytes() -> u64 {
    #[cfg(unix)]
    // SAFETY: getrusage is async-signal-safe and always succeeds with RUSAGE_SELF.
    unsafe {
        let mut rusage = std::mem::zeroed::<libc::rusage>();
        if libc::getrusage(libc::RUSAGE_SELF, &mut rusage) == 0 {
            #[cfg(target_os = "macos")]
            {
                rusage.ru_maxrss as u64
            }
            #[cfg(not(target_os = "macos"))]
            {
                (rusage.ru_maxrss as u64) * 1024
            }
        } else {
            0
        }
    }
    #[cfg(target_os = "windows")]
    // SAFETY: GetCurrentProcess returns a pseudo-handle that is always valid.
    unsafe {
        use winapi::um::processthreadsapi::GetCurrentProcess;
        use winapi::um::psapi::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};

        let mut pmc = std::mem::zeroed::<PROCESS_MEMORY_COUNTERS>();
        pmc.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
        if GetProcessMemoryInfo(GetCurrentProcess(), &mut pmc, pmc.cb) != 0 {
            pmc.WorkingSetSize as u64
        } else {
            0
        }
    }
    #[cfg(not(any(unix, target_os = "windows")))]
    {
        0
    }
}

/// Concatenates all `<style>` text blocks from the document.
pub fn extract_style_blocks(doc: &Document) -> String {
    let mut out = String::new();
    walk_style_blocks(doc, doc.root(), &mut out);
    out
}

fn walk_style_blocks(doc: &Document, id: NodeId, out: &mut String) {
    let node = doc.get(id);
    if let NodeData::Element { name, .. } = &node.data
        && name.local == "style"
    {
        for &child in &node.children {
            if let NodeData::Text(s) = &doc.get(child).data {
                out.push_str(s);
                out.push('\n');
            }
        }
        return;
    }
    for &child in &node.children {
        walk_style_blocks(doc, child, out);
    }
}
