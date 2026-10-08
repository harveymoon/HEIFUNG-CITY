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

#[cfg(feature = "heap-count")]
mod counting {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

    pub static LIVE: AtomicUsize = AtomicUsize::new(0);
    pub static PEAK: AtomicUsize = AtomicUsize::new(0);
    pub static BIGGEST: AtomicUsize = AtomicUsize::new(0);

    struct Counting;

    fn note(n: usize) {
        let live = LIVE.fetch_add(n, Relaxed) + n;
        PEAK.fetch_max(live, Relaxed);
        BIGGEST.fetch_max(n, Relaxed);
    }

    // SAFETY: every call goes straight to the system allocator; only the
    // counters are added.
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, l: Layout) -> *mut u8 {
            note(l.size());
            unsafe { System.alloc(l) }
        }
        unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
            note(l.size());
            unsafe { System.alloc_zeroed(l) }
        }
        unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
            LIVE.fetch_sub(l.size(), Relaxed);
            unsafe { System.dealloc(p, l) }
        }
        unsafe fn realloc(&self, p: *mut u8, l: Layout, new: usize) -> *mut u8 {
            LIVE.fetch_sub(l.size(), Relaxed);
            note(new);
            unsafe { System.realloc(p, l, new) }
        }
    }

    #[global_allocator]
    static A: Counting = Counting;
}
