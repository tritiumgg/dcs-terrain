//! The threads work is spread over, and their priority.
//!
//! A large request spreads its work over every core, below normal
//! priority, so a DCS server beside the tool wins the CPU whenever both
//! want it (ADR 0004). `--threads` caps how many.

use std::sync::atomic::{AtomicUsize, Ordering};

/// The cap `--threads` set, or 0 for none.
static CAP: AtomicUsize = AtomicUsize::new(0);

/// Starts the pool a request's parallel work runs on: `threads` workers,
/// every core when `None`, each below normal priority. Call it once,
/// before any request; a later call, or one after the pool has started on
/// its own, changes nothing.
pub fn init(threads: Option<usize>) {
    if let Some(n) = threads {
        CAP.store(n.max(1), Ordering::Relaxed);
    }
    let mut b = rayon::ThreadPoolBuilder::new().start_handler(|_| below_normal());
    if let Some(n) = threads {
        b = b.num_threads(n.max(1));
    }
    let _ = b.build_global();
}

/// The most threads any work of this process may take: the `--threads`
/// cap, or every core.
pub fn cap() -> usize {
    match CAP.load(Ordering::Relaxed) {
        0 => std::thread::available_parallelism().map_or(1, |n| n.get()),
        n => n,
    }
}

/// Lowers the calling thread's CPU priority below normal, leaving its I/O
/// at normal: a worker reading blocks for a query should not wait behind
/// every other reader of the disk. DCS runs only on Windows, so elsewhere
/// nothing is lowered.
pub fn below_normal() {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Threading::{
            GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL,
        };
        // SAFETY: the pseudo-handle names the calling thread, and failing
        // only leaves its priority as it was.
        unsafe {
            SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
        }
    }
}

/// Lowers the calling thread's CPU and I/O priority, so a cache build does
/// not take the CPU from a DCS server beside it. DCS runs only on Windows,
/// so elsewhere nothing is lowered.
pub fn low_priority() {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Threading::{
            GetCurrentThread, SetThreadPriority, THREAD_MODE_BACKGROUND_BEGIN,
        };
        // SAFETY: the pseudo-handle names the calling thread, and failing
        // only leaves its priority as it was.
        unsafe {
            SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_BEGIN);
        }
    }
}
