//! Data parallelism that exists only where threads do: native builds, and the browser build with the `threads`
//! feature (a worker pool). Elsewhere the same calls run serially.

/// How many threads to use on a machine with `cores` cores: half of them, and always at least one core is left free.
pub fn thread_budget(cores: usize) -> usize {
    (cores / 2).clamp(1, cores.saturating_sub(1).max(1))
}

/// Sizes the global pool: `requested` threads (still capped below the core count), or the default budget. A
/// `RAYON_NUM_THREADS` set by the user wins. Native only; the browser sizes its pool itself.
#[cfg(not(target_arch = "wasm32"))]
pub fn init_threads(requested: Option<usize>) {
    if std::env::var_os("RAYON_NUM_THREADS").is_some() {
        return;
    }
    let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
    let n = requested
        .map(|r| r.clamp(1, cores.saturating_sub(1).max(1)))
        .unwrap_or_else(|| thread_budget(cores));
    let _ = rayon::ThreadPoolBuilder::new()
        .num_threads(n)
        .build_global();
}

/// Runs the closure over every item.
pub fn each_mut<T: Send, F: Fn(&mut T) + Sync + Send>(items: &mut [T], f: F) {
    #[cfg(any(not(target_arch = "wasm32"), feature = "threads"))]
    {
        use rayon::prelude::*;
        items.par_iter_mut().for_each(f);
    }
    #[cfg(all(target_arch = "wasm32", not(feature = "threads")))]
    items.iter_mut().for_each(f);
}


/// Maps every item, keeping the order.
pub fn map<T: Sync, R: Send, F: Fn(&T) -> R + Sync + Send>(items: &[T], f: F) -> Vec<R> {
    #[cfg(any(not(target_arch = "wasm32"), feature = "threads"))]
    {
        use rayon::prelude::*;
        items.par_iter().map(f).collect()
    }
    #[cfg(all(target_arch = "wasm32", not(feature = "threads")))]
    items.iter().map(f).collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_budget_never_takes_every_core() {
        for (cores, want) in [(1, 1), (2, 1), (4, 2), (8, 4), (16, 8), (7, 3)] {
            assert_eq!(super::thread_budget(cores), want);
        }
    }
}
