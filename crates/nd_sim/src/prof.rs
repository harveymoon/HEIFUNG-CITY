//! A wall-clock profiler for finding what a big city spends its ticks on.
//! Compiled in only with the `profile` feature (`examples/perf_probe.rs`
//! turns it on); without it [`prof!`](crate::prof) expands to the bare
//! expression, so the simulation never reads a clock and stays
//! deterministic. Timings go to a thread-local table, read with [`take`].

#[cfg(feature = "profile")]
mod imp {
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::time::Instant;

    thread_local! {
        static TABLE: RefCell<BTreeMap<&'static str, Entry>> = const { RefCell::new(BTreeMap::new()) };
    }

    /// Calls, total seconds and the slowest call of one section.
    #[derive(Clone, Copy, Debug, Default)]
    pub struct Entry {
        pub calls: u64,
        pub total: f64,
        pub max: f64,
    }

    /// Times its section until dropped.
    pub struct Guard(&'static str, Instant);

    impl Guard {
        pub fn new(name: &'static str) -> Self {
            Guard(name, Instant::now())
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            let dt = self.1.elapsed().as_secs_f64();
            TABLE.with(|t| {
                let mut t = t.borrow_mut();
                let e = t.entry(self.0).or_default();
                e.calls += 1;
                e.total += dt;
                e.max = e.max.max(dt);
            });
        }
    }

    /// Everything timed since the last call, and reset.
    pub fn take() -> BTreeMap<&'static str, Entry> {
        TABLE.with(|t| std::mem::take(&mut *t.borrow_mut()))
    }
}

#[cfg(feature = "profile")]
pub use imp::{take, Entry, Guard};

/// Time `$e` under `$name` when the `profile` feature is on.
#[macro_export]
macro_rules! prof {
    ($name:expr, $e:expr) => {{
        #[cfg(feature = "profile")]
        let _guard = $crate::prof::Guard::new($name);
        $e
    }};
}

/// Time the rest of the enclosing scope under `$name` when the `profile`
/// feature is on.
#[macro_export]
macro_rules! prof_scope {
    ($name:expr) => {
        #[cfg(feature = "profile")]
        let _prof_guard = $crate::prof::Guard::new($name);
    };
}
