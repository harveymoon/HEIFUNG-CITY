//! [sweep-sim] Crash logs. A panic anywhere (the main thread, the autosave
//! writer, the remote API) writes its message, where it happened, a
//! backtrace and what the game was doing (the frame, the sim's day and
//! tick, the last save or load) to `saves/crash.log`, keeping the last
//! few as `crash.1.log` .. `crash.4.log`. Rust's default panic output
//! still goes to stderr as before.
//!
//! A failed allocation does not panic: the standard library prints
//! "memory allocation of N bytes failed" and aborts. The global allocator
//! here passes every call through to the system's and, when one fails,
//! lets go of a reserve held back for this, writes the crash log (size and
//! context; a backtrace when there is room for one) and returns the
//! failure as before, so the process still aborts the usual way.

use bevy::prelude::*;
use std::alloc::{GlobalAlloc, Layout, System};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU64, Ordering};
use std::sync::Mutex;

/// Logs kept: `crash.log` and this many older ones.
const KEEP: usize = 4;
/// Memory held back so the crash log can be written when memory runs out.
const RESERVE: usize = 2 << 20;

static FRAME: AtomicU64 = AtomicU64::new(0);
static TICK: AtomicU64 = AtomicU64::new(u64::MAX);
static LAST_IO: Mutex<Option<String>> = Mutex::new(None);
static RESERVE_PTR: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
static IN_OOM: AtomicBool = AtomicBool::new(false);
static INSTALLED: AtomicBool = AtomicBool::new(false);

/// The system allocator, watched for failures (see the module notes). The
/// game's only `#[global_allocator]`: with the `heap-count` feature it also
/// feeds the counters in `memory::counting`.
pub struct CrashAlloc;

unsafe impl GlobalAlloc for CrashAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(layout) };
        if p.is_null() {
            out_of_memory(layout.size());
        } else {
            #[cfg(feature = "heap-count")]
            crate::memory::counting::alloc(layout.size());
        }
        p
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc_zeroed(layout) };
        if p.is_null() {
            out_of_memory(layout.size());
        } else {
            #[cfg(feature = "heap-count")]
            crate::memory::counting::alloc(layout.size());
        }
        p
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let p = unsafe { System.realloc(ptr, layout, new_size) };
        if p.is_null() {
            out_of_memory(new_size);
        } else {
            #[cfg(feature = "heap-count")]
            {
                crate::memory::counting::free(layout.size());
                crate::memory::counting::alloc(new_size);
            }
        }
        p
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        #[cfg(feature = "heap-count")]
        crate::memory::counting::free(layout.size());
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static GLOBAL: CrashAlloc = CrashAlloc;

fn reserve_layout() -> Layout {
    Layout::from_size_align(RESERVE, 16).expect("a valid layout")
}

/// An allocation of `size` bytes failed: free the reserve, write the log
/// (once; a failure while writing it is not logged again).
fn out_of_memory(size: usize) {
    if !INSTALLED.load(Ordering::Relaxed) || IN_OOM.swap(true, Ordering::SeqCst) {
        return;
    }
    let r = RESERVE_PTR.swap(std::ptr::null_mut(), Ordering::SeqCst);
    if !r.is_null() {
        unsafe { System.dealloc(r, reserve_layout()) };
    }
    let mut text = format!("memory allocation of {size} bytes ({:.1} MiB) failed\n", size as f64 / 1048576.0);
    text += &context();
    // Symbolising a backtrace takes memory of its own: only when the
    // failed request was a big one (there is likely room for small ones).
    if size >= 1 << 20 {
        text += &format!("\nbacktrace:\n{}\n", std::backtrace::Backtrace::force_capture());
    }
    write_log(&text);
}

/// What the game was doing: the frame, the sim's clock, the last save or
/// load.
fn context() -> String {
    let frame = FRAME.load(Ordering::Relaxed);
    let tick = TICK.load(Ordering::Relaxed);
    let clock = if tick == u64::MAX { "not started".to_string() } else { format!("day {} {:02}:{:02} (tick {tick})", tick / 1440 + 1, tick % 1440 / 60, tick % 60) };
    let io = LAST_IO.try_lock().ok().and_then(|g| g.clone()).unwrap_or_else(|| "none yet".to_string());
    format!("frame: {frame}\nsim clock: {clock}\nlast save/load: {io}\n")
}

/// Where the logs go: beside the saves.
fn log_dir() -> PathBuf {
    PathBuf::from(crate::ui::saves::DIR)
}

/// `crash.log`, or the `n`th older one.
fn log_path(dir: &Path, n: usize) -> PathBuf {
    if n == 0 { dir.join("crash.log") } else { dir.join(format!("crash.{n}.log")) }
}

/// Shift the older logs down one (dropping the oldest) and write `body`
/// as the new `crash.log`, headed by the time and the version.
fn write_log(body: &str) {
    let dir = log_dir();
    let _ = std::fs::create_dir_all(&dir);
    rotate(&dir);
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let head = format!("HEIFUNG://CITY {} crash, unix time {secs}\n", env!("CARGO_PKG_VERSION"));
    if let Ok(mut f) = std::fs::File::create(log_path(&dir, 0)) {
        let _ = f.write_all(head.as_bytes());
        let _ = f.write_all(body.as_bytes());
        let _ = f.flush();
    }
    eprintln!("crash log written to {}", log_path(&dir, 0).display());
}

fn rotate(dir: &Path) {
    let _ = std::fs::remove_file(log_path(dir, KEEP));
    for n in (0..KEEP).rev() {
        let from = log_path(dir, n);
        if from.exists() {
            let _ = std::fs::rename(&from, log_path(dir, n + 1));
        }
    }
}

/// The panic's message, from its payload.
fn message(info: &std::panic::PanicHookInfo<'_>) -> String {
    let p = info.payload();
    p.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| p.downcast_ref::<String>().cloned()).unwrap_or_else(|| "(no message)".to_string())
}

/// The crash log for a panic.
fn panic_text(info: &std::panic::PanicHookInfo<'_>) -> String {
    let at = info.location().map_or_else(|| "unknown".to_string(), |l| format!("{}:{}:{}", l.file(), l.line(), l.column()));
    let thread = std::thread::current().name().unwrap_or("unnamed").to_string();
    format!("panic in thread '{thread}' at {at}:\n{}\n\n{}\nbacktrace:\n{}\n", message(info), context(), std::backtrace::Backtrace::force_capture())
}

/// Install the panic hook (the default output still goes to stderr) and
/// set the memory reserve aside. Call first thing in `main`.
pub fn install() {
    let r = unsafe { System.alloc(reserve_layout()) };
    RESERVE_PTR.store(r, Ordering::SeqCst);
    INSTALLED.store(true, Ordering::SeqCst);
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        write_log(&panic_text(info));
        default(info);
    }));
}

/// Note a save or load (`what` "saving", "loaded" and so on) for the next
/// crash log.
pub fn note_io(what: &str, path: &Path) {
    if let Ok(mut g) = LAST_IO.lock() {
        *g = Some(format!("{what} {} (frame {})", path.display(), FRAME.load(Ordering::Relaxed)));
    }
}

/// Count frames and keep the sim's clock for the crash log.
fn track(sim: Res<crate::sim::Sim>) {
    FRAME.fetch_add(1, Ordering::Relaxed);
    TICK.store(sim.world.clock.tick, Ordering::Relaxed);
}

pub struct CrashPlugin;

impl Plugin for CrashPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Last, track);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logs_rotate_keeping_the_last_few() {
        let dir = std::env::temp_dir().join(format!("nd_crash_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for k in 0..(KEEP + 3) {
            rotate(&dir);
            std::fs::write(log_path(&dir, 0), format!("crash {k}")).unwrap();
        }
        assert_eq!(std::fs::read_to_string(log_path(&dir, 0)).unwrap(), format!("crash {}", KEEP + 2));
        assert_eq!(std::fs::read_to_string(log_path(&dir, KEEP)).unwrap(), "crash 2");
        assert!(!log_path(&dir, KEEP + 1).exists(), "only the last few");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_context_names_the_last_save_and_the_clock() {
        note_io("saving", Path::new("saves/x.ron"));
        TICK.store(1440 * 2 + 61, Ordering::Relaxed);
        let c = context();
        assert!(c.contains("saving saves/x.ron"), "{c}");
        assert!(c.contains("day 3 01:01"), "{c}");
    }
}
