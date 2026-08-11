//! Non-blocking stderr sink (BUG-770).
//!
//! Diagnostics are written with plain `eprintln!` from every thread of the
//! process, including the UI thread. When the parent process captures stderr
//! as a pipe (`subprocess.PIPE`) and then stops reading it, the OS pipe buffer
//! — a few KiB on Windows — fills up and the *writing* thread blocks inside
//! `WriteFile` forever. With the UI thread blocked the whole window freezes:
//! automation stops being served, input is not processed, nothing repaints.
//! That is exactly how BUG-770 killed the full graphic-test run at page 24.
//!
//! A browser must not wedge because someone else stopped reading its log, so
//! this module inserts a sink between the process and the real stderr:
//!
//! ```text
//! eprintln!  ──▶  our pipe  ──▶  reader thread  ──▶  ring buffer
//!                                                        │
//!                                     writer thread ◀────┘  ──▶  real stderr
//! ```
//!
//! The reader thread never blocks on the writer: once the ring is full it
//! drops the oldest chunks and counts the lost bytes, so the pipe our own
//! `eprintln!` writes into is always being drained. Diagnostics are lost
//! instead of frames — and the loss is *reported* (a `[diag] …` notice is
//! emitted as soon as the consumer starts reading again), never swallowed:
//! the silent `except Exception: pass` on the harness side is precisely what
//! made this defect look like an engine hang on one specific page.
//!
//! Installed only when stderr is a pipe: a console or a redirected file
//! cannot block indefinitely, so an interactive or file-logged run keeps the
//! previous behaviour bit for bit, with no extra threads and no buffering.
//! Windows-only for now — the hang is measured there and the pipe buffer is
//! smallest there; other platforms keep plain `eprintln!`.

/// Installs the sink if this process's stderr is a pipe. Idempotent; a no-op
/// on non-Windows targets and whenever stderr is a console, a file or absent.
pub fn install() {
    #[cfg(target_os = "windows")]
    imp::install();
}

/// Blocks until everything already written to stderr has reached the real
/// stderr handle, or until `timeout` elapses. Call before the process exits:
/// the writer thread is detached, so without this the tail of the log can be
/// lost when `main` returns. No-op when the sink is not installed.
pub fn flush(timeout: std::time::Duration) {
    #[cfg(target_os = "windows")]
    imp::flush(timeout);
    #[cfg(not(target_os = "windows"))]
    let _ = timeout;
}

#[cfg(target_os = "windows")]
mod imp {
    use std::collections::VecDeque;
    use std::ffi::c_void;
    use std::sync::{Condvar, Mutex, OnceLock};
    use std::time::{Duration, Instant};

    /// `STD_ERROR_HANDLE` from `winbase.h` (`(DWORD)-12`).
    const STD_ERROR_HANDLE: u32 = -12i32 as u32;
    /// `FILE_TYPE_PIPE` from `fileapi.h` — the only case that can block forever.
    const FILE_TYPE_PIPE: u32 = 0x0003;
    /// `INVALID_HANDLE_VALUE` from `handleapi.h`.
    const INVALID_HANDLE: isize = -1;
    /// Upper bound on buffered-but-unwritten stderr, in bytes. Sized to hold a
    /// full graphic-test run's diagnostics (~95 bytes per page, 153 pages)
    /// with three orders of magnitude of headroom, while still being a hard
    /// cap: a consumer that never reads must not grow the browser's heap.
    const RING_CAP: usize = 1 << 20;
    /// Read granularity from our own pipe.
    const READ_CHUNK: usize = 8192;

    // SAFETY-anchor: kernel32 is always loaded in a Windows process; these are
    // the documented Win32 signatures. Declared inline (no `windows-sys` dep)
    // the same way `download.rs` declares `ShellExecuteW`.
    unsafe extern "system" {
        fn GetStdHandle(nStdHandle: u32) -> isize;
        fn SetStdHandle(nStdHandle: u32, hHandle: isize) -> i32;
        fn GetFileType(hFile: isize) -> u32;
        fn CreatePipe(
            hReadPipe: *mut isize,
            hWritePipe: *mut isize,
            lpPipeAttributes: *mut c_void,
            nSize: u32,
        ) -> i32;
        fn ReadFile(
            hFile: isize,
            lpBuffer: *mut u8,
            nNumberOfBytesToRead: u32,
            lpNumberOfBytesRead: *mut u32,
            lpOverlapped: *mut c_void,
        ) -> i32;
        fn WriteFile(
            hFile: isize,
            lpBuffer: *const u8,
            nNumberOfBytesToWrite: u32,
            lpNumberOfBytesWritten: *mut u32,
            lpOverlapped: *mut c_void,
        ) -> i32;
    }

    /// Bounded FIFO of stderr chunks waiting to reach the real handle.
    struct Ring {
        /// Chunks in write order.
        chunks: VecDeque<Vec<u8>>,
        /// Sum of `chunks` lengths — kept incrementally, `RING_CAP` applies to it.
        bytes: usize,
        /// Bytes discarded because the ring was full and the consumer stalled.
        dropped: u64,
        /// A chunk popped by the writer but not yet handed to `WriteFile`.
        /// `flush` must wait for it too, otherwise "ring is empty" would let
        /// the process exit in the middle of the last line.
        in_flight: bool,
    }

    /// The installed sink; `None` until (and unless) `install` succeeds.
    static SINK: OnceLock<(Mutex<Ring>, Condvar)> = OnceLock::new();

    pub fn install() {
        if SINK.get().is_some() {
            return;
        }
        // Escape hatch in the `LUMEN_NO_*` family (docs/automation.md): gives
        // one binary both arms of the BUG-770 A/B — with the sink the run
        // survives a dead stderr consumer, without it the window wedges as
        // before. Also a way out if the sink itself is ever suspected.
        if std::env::var_os("LUMEN_NO_STDERR_SINK").is_some() {
            return;
        }
        // SAFETY: `GetStdHandle`/`GetFileType` take no pointers and are valid
        // for any handle id; the returned handle is only inspected here.
        let real = unsafe { GetStdHandle(STD_ERROR_HANDLE) };
        if real == 0 || real == INVALID_HANDLE {
            return;
        }
        // SAFETY: see above — `real` is a handle owned by this process.
        if unsafe { GetFileType(real) } != FILE_TYPE_PIPE {
            return;
        }

        let mut read_end: isize = 0;
        let mut write_end: isize = 0;
        // SAFETY: both out-pointers reference live stack slots; a null
        // security-attributes pointer and size 0 request kernel defaults.
        let created = unsafe {
            CreatePipe(&mut read_end, &mut write_end, std::ptr::null_mut(), 0)
        };
        if created == 0 {
            return;
        }
        // SAFETY: `write_end` is a valid handle just returned by `CreatePipe`.
        if unsafe { SetStdHandle(STD_ERROR_HANDLE, write_end) } == 0 {
            // Redirect refused — leave stderr exactly as it was. The two pipe
            // handles leak (one pipe, process lifetime); closing them here
            // would need a `CloseHandle` import for a path that cannot happen
            // in practice, and leaking is strictly safer than closing a handle
            // that might already be the live stderr.
            return;
        }
        let _ = SINK.set((
            Mutex::new(Ring {
                chunks: VecDeque::new(),
                bytes: 0,
                dropped: 0,
                in_flight: false,
            }),
            Condvar::new(),
        ));

        std::thread::Builder::new()
            .name("lumen-stderr-reader".into())
            .spawn(move || reader_loop(read_end))
            .ok();
        std::thread::Builder::new()
            .name("lumen-stderr-writer".into())
            .spawn(move || writer_loop(real))
            .ok();
    }

    /// Drains our pipe as fast as the OS delivers, so that no `eprintln!`
    /// anywhere in the process can ever block on a full pipe buffer.
    fn reader_loop(read_end: isize) {
        let Some((ring, cv)) = SINK.get() else { return };
        let mut buf = vec![0u8; READ_CHUNK];
        loop {
            let mut got: u32 = 0;
            // SAFETY: `buf` is a live allocation of `READ_CHUNK` bytes and
            // `got` a live stack slot; the handle is owned by this thread.
            let ok = unsafe {
                ReadFile(read_end, buf.as_mut_ptr(), READ_CHUNK as u32, &mut got, std::ptr::null_mut())
            };
            if ok == 0 || got == 0 {
                // Write end closed or pipe broken: nothing more can arrive.
                return;
            }
            let Ok(mut ring) = ring.lock() else { return };
            ring.bytes += got as usize;
            ring.chunks.push_back(buf[..got as usize].to_vec());
            while ring.bytes > RING_CAP {
                match ring.chunks.pop_front() {
                    Some(old) => {
                        ring.bytes -= old.len();
                        ring.dropped += old.len() as u64;
                    }
                    None => break,
                }
            }
            drop(ring);
            cv.notify_all();
        }
    }

    /// Writes buffered chunks to the real stderr. This thread is the only one
    /// allowed to block on it — the UI thread never does.
    fn writer_loop(real: isize) {
        let Some((ring, cv)) = SINK.get() else { return };
        loop {
            let (chunk, dropped) = {
                let Ok(mut guard) = ring.lock() else { return };
                while guard.chunks.is_empty() {
                    match cv.wait(guard) {
                        Ok(g) => guard = g,
                        Err(_) => return,
                    }
                }
                let dropped = std::mem::take(&mut guard.dropped);
                let chunk = guard.chunks.pop_front().unwrap_or_default();
                guard.bytes -= chunk.len();
                guard.in_flight = true;
                (chunk, dropped)
            };
            if dropped > 0 {
                // Report the loss instead of hiding it: a gap in the log that
                // nothing announces is what turned BUG-770 into a week-old
                // mystery. If this notice itself cannot be written we are
                // about to bail out below anyway.
                let notice = format!(
                    "[diag] потеряно {dropped} байт stderr — потребитель не читал трубу\n"
                );
                if !write_all(real, notice.as_bytes()) {
                    return;
                }
            }
            let alive = write_all(real, &chunk);
            if let Ok(mut guard) = ring.lock() {
                guard.in_flight = false;
            }
            cv.notify_all();
            if !alive {
                // Real stderr is gone (closed pipe, dead parent). Keep draining
                // so the reader thread never stalls, but stop writing.
                discard_forever();
                return;
            }
        }
    }

    /// Consumes the ring without writing anywhere — used once the real stderr
    /// is unusable, so that `install`'s guarantee (the reader never stalls)
    /// survives a dead parent process.
    fn discard_forever() {
        let Some((ring, cv)) = SINK.get() else { return };
        loop {
            let Ok(mut guard) = ring.lock() else { return };
            guard.chunks.clear();
            guard.bytes = 0;
            guard.dropped = 0;
            guard.in_flight = false;
            cv.notify_all();
            match cv.wait(guard) {
                Ok(_) => {}
                Err(_) => return,
            }
        }
    }

    /// `WriteFile` until the whole buffer is out. `false` — the handle died.
    fn write_all(handle: isize, mut buf: &[u8]) -> bool {
        while !buf.is_empty() {
            let mut written: u32 = 0;
            // SAFETY: `buf` is a live slice, `written` a live stack slot, and
            // `handle` the stderr handle captured at install time.
            let ok = unsafe {
                WriteFile(handle, buf.as_ptr(), buf.len() as u32, &mut written, std::ptr::null_mut())
            };
            if ok == 0 || written == 0 {
                return false;
            }
            buf = &buf[written as usize..];
        }
        true
    }

    pub fn flush(timeout: Duration) {
        let Some((ring, cv)) = SINK.get() else { return };
        let deadline = Instant::now() + timeout;
        let Ok(mut guard) = ring.lock() else { return };
        while !guard.chunks.is_empty() || guard.in_flight {
            let Some(left) = deadline.checked_duration_since(Instant::now()) else {
                return;
            };
            match cv.wait_timeout(guard, left) {
                Ok((g, _)) => guard = g,
                Err(_) => return,
            }
        }
    }
}
