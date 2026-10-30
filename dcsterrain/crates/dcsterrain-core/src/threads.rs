//! The threads work is spread over, and their priority.
//!
//! A large request spreads its work over every core, below normal
//! priority, so a DCS server beside the tool wins the CPU whenever both
//! want it (ADR 0004). `--threads` caps how many.

use std::io;
use std::process::{Child, ExitStatus};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};
#[cfg(any(test, windows, all(unix, target_arch = "x86_64")))]
use std::time::{Duration, Instant};

use rayon::prelude::*;

/// How long a spin measures the rate of the cycle counter.
#[cfg(any(windows, all(unix, target_arch = "x86_64")))]
const CALIBRATION: Duration = Duration::from_millis(20);

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

/// `f` over `items`, on every core when `parallel` and else on the calling
/// thread, the answers in the order of `items`. Every item is answered and
/// the answers then checked in order, so of two that fail the one returned
/// is the first, however the work fell.
pub fn try_map<T, U, E, F>(items: &[T], parallel: bool, f: F) -> Result<Vec<U>, E>
where
    T: Sync,
    U: Send,
    E: Send,
    F: Fn(&T) -> Result<U, E> + Sync + Send,
{
    if parallel {
        let answers: Vec<Result<U, E>> = items.par_iter().map(f).collect();
        answers.into_iter().collect()
    } else {
        items.iter().map(f).collect()
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

/// The CPU cycles this process has spent, over all its threads: set beside
/// the wall clock, how much of the machine a measured step took.
///
/// These are reference cycles on every platform: time on a CPU, counted at
/// the machine's constant counter rate, [`cycle_hz`], not the core's clock.
/// Windows counts them itself; elsewhere the OS's CPU time is scaled by the
/// rate. So a thread waiting for a disk or for a core costs none, but a
/// core running above its base clock does the same work in fewer.
pub fn cpu_cycles() -> u64 {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Threading::GetCurrentProcess;
        use windows_sys::Win32::System::WindowsProgramming::QueryProcessCycleTime;
        let mut c = 0u64;
        // SAFETY: the pseudo-handle of this process, and a u64 to write to.
        unsafe { QueryProcessCycleTime(GetCurrentProcess(), &mut c) };
        c
    }
    #[cfg(unix)]
    {
        let mut t = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        // SAFETY: a clock every Unix here has, and a timespec to write to.
        unsafe { libc::clock_gettime(libc::CLOCK_PROCESS_CPUTIME_ID, &mut t) };
        seconds_to_cycles(t.tv_sec as f64 + t.tv_nsec as f64 * 1e-9)
    }
}

/// How many of [`cpu_cycles`]' cycles make a second of CPU time on this
/// machine.
pub fn cycle_hz() -> f64 {
    static HZ: OnceLock<f64> = OnceLock::new();
    *HZ.get_or_init(measure_cycle_hz)
}

#[cfg(unix)]
fn seconds_to_cycles(s: f64) -> u64 {
    (s * cycle_hz()) as u64
}

/// The rate of a spin on this thread against the wall clock, so nothing
/// else running in the process counts.
#[cfg(windows)]
fn measure_cycle_hz() -> f64 {
    use windows_sys::Win32::System::Threading::GetCurrentThread;
    use windows_sys::Win32::System::WindowsProgramming::QueryThreadCycleTime;
    let thread_cycles = || {
        let mut c = 0u64;
        // SAFETY: the pseudo-handle of this thread, and a u64 to write to.
        unsafe { QueryThreadCycleTime(GetCurrentThread(), &mut c) };
        c
    };
    // A thread preempted during a spin counts too few cycles for its wall
    // clock and never too many, so the fastest spin is the truest.
    (0..5)
        .map(|_| {
            let (t, c) = (Instant::now(), thread_cycles());
            while t.elapsed() < CALIBRATION {
                std::hint::spin_loop();
            }
            (thread_cycles() - c) as f64 / t.elapsed().as_secs_f64()
        })
        .fold(0.0, f64::max)
}

/// The time-stamp counter against the wall clock: it runs at one rate
/// whatever the core's clock, on every x86-64 of the last fifteen years.
#[cfg(all(unix, target_arch = "x86_64"))]
fn measure_cycle_hz() -> f64 {
    // SAFETY: reading the counter has no preconditions on x86-64.
    let tsc = || unsafe { core::arch::x86_64::_rdtsc() };
    let (t, c) = (Instant::now(), tsc());
    while t.elapsed() < CALIBRATION {
        std::hint::spin_loop();
    }
    (tsc() - c) as f64 / t.elapsed().as_secs_f64()
}

/// The generic timer's rate, which the architecture publishes.
#[cfg(all(unix, target_arch = "aarch64"))]
fn measure_cycle_hz() -> f64 {
    let hz: u64;
    // SAFETY: the register is readable from user mode on Linux and macOS.
    unsafe { core::arch::asm!("mrs {}, cntfrq_el0", out(reg) hz) };
    hz as f64
}

/// Nanoseconds, where no counter's rate is known.
#[cfg(all(unix, not(any(target_arch = "x86_64", target_arch = "aarch64"))))]
fn measure_cycle_hz() -> f64 {
    1e9
}

/// What a finished child process cost.
#[derive(Debug, Clone, Copy, Default)]
pub struct ChildCost {
    /// Its [`cpu_cycles`], over all its threads.
    pub cycles: u64,
    /// The most memory it held: private bytes on Windows, resident
    /// elsewhere, where the OS keeps no peak of private bytes. Zero on
    /// Linux where the child stayed under its parent's own peak.
    pub peak_bytes: u64,
}

/// Waits for `child` and says what it cost. Drain its pipes first.
pub fn wait_costed(child: Child) -> io::Result<(ExitStatus, ChildCost)> {
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::ProcessStatus::{
            GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
        };
        use windows_sys::Win32::System::WindowsProgramming::QueryProcessCycleTime;
        let mut child = child;
        let status = child.wait()?;
        let handle = child.as_raw_handle();
        let mut cycles = 0u64;
        // SAFETY: the zeroed struct of integers is valid, its size is passed
        // along with it, and `child` keeps the handle open past its exit.
        let mut memory: PROCESS_MEMORY_COUNTERS = unsafe { std::mem::zeroed() };
        let counted = unsafe {
            QueryProcessCycleTime(handle as _, &mut cycles) != 0
                && GetProcessMemoryInfo(
                    handle as _,
                    &mut memory,
                    size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
                ) != 0
        };
        if !counted {
            return Err(io::Error::last_os_error());
        }
        let peak_bytes = memory.PeakPagefileUsage as u64;
        Ok((status, ChildCost { cycles, peak_bytes }))
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        let mut raw = 0;
        // SAFETY: the zeroed struct of integers is valid to write over. The
        // child is taken by value, so nothing can wait for it a second time.
        let mut used: libc::rusage = unsafe { std::mem::zeroed() };
        while unsafe { libc::wait4(child.id() as i32, &mut raw, 0, &mut used) } < 0 {
            let e = io::Error::last_os_error();
            if e.kind() != io::ErrorKind::Interrupted {
                return Err(e);
            }
        }
        let seconds = |t: libc::timeval| t.tv_sec as f64 + t.tv_usec as f64 * 1e-6;
        // The kilobytes Linux reports, the bytes macOS does.
        let unit = if cfg!(target_os = "macos") { 1 } else { 1024 };
        let mut peak_bytes = used.ru_maxrss as u64 * unit;
        // Linux seeds a spawned child's peak from its parent's, which the
        // child shares memory with until it runs its program, so a peak no
        // higher than the parent's says nothing about the child.
        if cfg!(target_os = "linux") {
            // SAFETY: as above, a zeroed struct of integers to write over.
            let mut own: libc::rusage = unsafe { std::mem::zeroed() };
            if unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut own) } == 0
                && used.ru_maxrss <= own.ru_maxrss
            {
                peak_bytes = 0;
            }
        }
        let cost = ChildCost {
            cycles: seconds_to_cycles(seconds(used.ru_utime) + seconds(used.ru_stime)),
            peak_bytes,
        };
        Ok((ExitStatus::from_raw(raw), cost))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mapping_answers_in_order_and_refuses_with_the_first_failure() {
        let items: Vec<u32> = (0..1000).collect();
        let even = |&i: &u32| if i % 2 == 0 { Ok(i) } else { Err(i) };
        let double = |&i: &u32| Ok::<u32, u32>(i * 2);
        for parallel in [false, true] {
            assert_eq!(try_map(&items, parallel, even), Err(1));
            let doubled = try_map(&items, parallel, double).unwrap();
            assert!(doubled.iter().enumerate().all(|(k, &d)| d == 2 * k as u32));
        }
    }

    #[test]
    fn a_spin_costs_cycles_at_a_believable_rate() {
        assert!((1e6..1e11).contains(&cycle_hz()), "{}", cycle_hz());
        let c = cpu_cycles();
        let t = Instant::now();
        while cpu_cycles() == c {
            assert!(t.elapsed() < Duration::from_secs(5), "no cycles counted");
            std::hint::spin_loop();
        }
    }
}
