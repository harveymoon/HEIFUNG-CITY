//! [memory] The process's own memory counters, for the remote `memory`
//! command: (working set, peak working set, commit charge) in bytes.

#[cfg(windows)]
pub fn process_memory() -> (usize, usize, usize) {
    #[repr(C)]
    #[derive(Default)]
    struct Counters {
        cb: u32,
        page_faults: u32,
        peak_working_set: usize,
        working_set: usize,
        quota_peak_paged: usize,
        quota_paged: usize,
        quota_peak_non_paged: usize,
        quota_non_paged: usize,
        pagefile: usize,
        peak_pagefile: usize,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> isize;
        fn K32GetProcessMemoryInfo(process: isize, counters: *mut Counters, cb: u32) -> i32;
    }
    let mut c = Counters { cb: std::mem::size_of::<Counters>() as u32, ..Default::default() };
    // SAFETY: a pseudo-handle for this process and a buffer of the size we say.
    let ok = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) };
    if ok == 0 {
        return (0, 0, 0);
    }
    (c.working_set, c.peak_working_set, c.pagefile)
}

#[cfg(not(windows))]
pub fn process_memory() -> (usize, usize, usize) {
    (0, 0, 0)
}

/// [memory] Heap counters with the `heap-count` feature: bytes live, the
/// peak and the biggest single block asked for (all zero without it).
pub fn heap() -> (usize, usize, usize) {
    #[cfg(feature = "heap-count")]
    {
        use std::sync::atomic::Ordering::Relaxed;
        (counting::LIVE.load(Relaxed), counting::PEAK.load(Relaxed), counting::BIGGEST.load(Relaxed))
    }
    #[cfg(not(feature = "heap-count"))]
    {
        (0, 0, 0)
    }
}

/// [memory] The counters behind `heap()`. There is one global allocator
/// (`crash::CrashAlloc`); with the feature on it calls these after each
/// successful allocation, free and reallocation.
#[cfg(feature = "heap-count")]
pub(crate) mod counting {
    use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

    pub static LIVE: AtomicUsize = AtomicUsize::new(0);
    pub static PEAK: AtomicUsize = AtomicUsize::new(0);
    pub static BIGGEST: AtomicUsize = AtomicUsize::new(0);

    /// `n` bytes were handed out.
    pub fn alloc(n: usize) {
        let live = LIVE.fetch_add(n, Relaxed) + n;
        PEAK.fetch_max(live, Relaxed);
        BIGGEST.fetch_max(n, Relaxed);
    }

    /// `n` bytes were given back.
    pub fn free(n: usize) {
        LIVE.fetch_sub(n, Relaxed);
    }
}
